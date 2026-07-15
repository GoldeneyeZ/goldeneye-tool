use std::collections::{BTreeMap, BTreeSet};

use goldeneye_domain::{
    EdgeKind, Generation, GraphEdge, GraphNode, GraphProperties, LanguageId, NodeId, ProjectId,
    ProjectRelativePath,
};
use goldeneye_ports::{
    IndexExtractedCall as ExtractedCall, IndexExtractedImport as ExtractedImport,
    IndexExtractedRelation as ExtractedRelation,
};
use serde_json::{Value, json};

use crate::IndexError;

const MAX_RESOLUTION_PASSES: usize = 3;
const MAX_PROJECT_PENDING_FACTS: usize = 100_000;

#[derive(Debug, Clone)]
struct DefinitionRef {
    id: NodeId,
    name: String,
    qualified_name: String,
    label: String,
    language: LanguageId,
    module_qn: String,
    owner_type: Option<String>,
}

#[derive(Debug)]
struct DefinitionIndex {
    definitions: Vec<DefinitionRef>,
    by_name: BTreeMap<String, Vec<usize>>,
    by_qn: BTreeMap<String, usize>,
    file_modules: BTreeMap<ProjectRelativePath, String>,
}

#[derive(Debug, Clone, Copy)]
enum ResolutionStrategy {
    ReceiverType,
    ImportMap,
    SameContainer,
    QualifiedSuffix,
    UniqueName,
    JvmTail,
}

impl ResolutionStrategy {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ReceiverType => "hybrid_receiver_type",
            Self::ImportMap => "hybrid_import_map",
            Self::SameContainer => "hybrid_same_container",
            Self::QualifiedSuffix => "hybrid_qualified_suffix",
            Self::UniqueName => "hybrid_unique_name",
            Self::JvmTail => "hybrid_jvm_tail",
        }
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) fn resolve_project(
    project: &ProjectId,
    nodes: &[GraphNode],
    edges: &mut Vec<GraphEdge>,
    mut calls: Vec<ExtractedCall>,
    mut relations: Vec<ExtractedRelation>,
    mut imports: Vec<ExtractedImport>,
) -> Result<(), IndexError> {
    calls.sort_by(|left, right| {
        (&left.file, left.start_byte, &left.callee_name).cmp(&(
            &right.file,
            right.start_byte,
            &right.callee_name,
        ))
    });
    calls.dedup_by(|left, right| {
        left.file == right.file
            && left.start_byte == right.start_byte
            && left.callee_name == right.callee_name
    });
    relations.sort();
    relations.dedup();
    imports.sort();
    imports.dedup();
    calls.truncate(MAX_PROJECT_PENDING_FACTS);
    relations.truncate(MAX_PROJECT_PENDING_FACTS);
    imports.truncate(MAX_PROJECT_PENDING_FACTS);

    // Extraction performs a cheap same-file pass. For LSP-wired languages, replace those
    // provisional edges with the project-wide result so ambiguous member/builtin matches do not
    // survive merely because a similarly named local declaration exists.
    let call_sites = calls
        .iter()
        .filter(|call| is_lsp_wired(call.language.as_str()))
        .map(|call| (call.source.clone(), call.start_byte.to_string()))
        .collect::<BTreeSet<_>>();
    let relation_sites = relations
        .iter()
        .filter(|relation| {
            is_lsp_wired(relation.language.as_str()) || relation.language.as_str() == "graphql"
        })
        .map(|relation| {
            (
                relation.source.clone(),
                relation.kind.to_owned(),
                normalize_name(&relation.target_name),
            )
        })
        .collect::<BTreeSet<_>>();
    edges.retain(|edge| {
        let provisional_call = edge.kind.as_str() == "CALLS"
            && call_sites.contains(&(edge.source.clone(), edge.discriminator.as_str().to_owned()));
        let provisional_relation = relation_sites.contains(&(
            edge.source.clone(),
            edge.kind.as_str().to_owned(),
            normalize_name(edge.discriminator.as_str()),
        ));
        !provisional_call && !provisional_relation
    });

    let index = DefinitionIndex::build(nodes);
    let imports_by_file = imports.into_iter().fold(
        BTreeMap::<ProjectRelativePath, Vec<ExtractedImport>>::new(),
        |mut by_file, import| {
            by_file.entry(import.file.clone()).or_default().push(import);
            by_file
        },
    );
    let mut identities = edges
        .iter()
        .map(|edge| {
            (
                edge.source.clone(),
                edge.target.clone(),
                edge.kind.as_str().to_owned(),
                edge.discriminator.as_str().to_owned(),
            )
        })
        .collect::<BTreeSet<_>>();

    let mut unresolved_calls = calls;
    let mut unresolved_relations = relations;
    for _ in 0..MAX_RESOLUTION_PASSES {
        let mut progress = 0usize;
        let mut next_calls = Vec::new();
        for call in unresolved_calls {
            let file_imports = imports_by_file
                .get(&call.file)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let Some((target, strategy)) = index.resolve_call(&call, file_imports) else {
                next_calls.push(call);
                continue;
            };
            if call.source == target.id {
                continue;
            }
            let discriminator = call.start_byte.to_string();
            let identity = (
                call.source.clone(),
                target.id.clone(),
                "CALLS".to_owned(),
                discriminator.clone(),
            );
            if identities.insert(identity) {
                let mut properties = GraphProperties::new();
                properties.insert("callee".into(), Value::String(call.callee_name.clone()));
                properties.insert("line".into(), json!(call.line));
                properties.insert(
                    "resolved_qn".into(),
                    Value::String(target.qualified_name.clone()),
                );
                properties.insert(
                    "strategy".into(),
                    Value::String(strategy.as_str().to_owned()),
                );
                edges.push(graph_edge(
                    project,
                    call.source.clone(),
                    target.id.clone(),
                    "CALLS",
                    Some(discriminator),
                    properties,
                )?);
                progress += 1;
            }
        }
        unresolved_calls = next_calls;

        let mut next_relations = Vec::new();
        for relation in unresolved_relations {
            let file_imports = imports_by_file
                .get(&relation.file)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let Some(target) = index.resolve_relation(&relation, file_imports) else {
                next_relations.push(relation);
                continue;
            };
            if relation.source == target.id {
                continue;
            }
            let discriminator = normalize_name(&relation.target_name);
            let identity = (
                relation.source.clone(),
                target.id.clone(),
                relation.kind.to_owned(),
                discriminator.clone(),
            );
            if identities.insert(identity) {
                edges.push(graph_edge(
                    project,
                    relation.source.clone(),
                    target.id.clone(),
                    relation.kind,
                    Some(discriminator),
                    GraphProperties::new(),
                )?);
                progress += 1;
            }
        }
        unresolved_relations = next_relations;

        if progress == 0 {
            break;
        }
    }
    edges.sort_by(|left, right| {
        (
            &left.source,
            left.kind.as_str(),
            &left.target,
            left.discriminator.as_str(),
        )
            .cmp(&(
                &right.source,
                right.kind.as_str(),
                &right.target,
                right.discriminator.as_str(),
            ))
    });
    Ok(())
}

impl DefinitionIndex {
    fn build(nodes: &[GraphNode]) -> Self {
        let mut file_modules = BTreeMap::<ProjectRelativePath, String>::new();
        for node in nodes {
            if node.label.as_str() != "Module" {
                continue;
            }
            let Some(file) = node.file_path.clone() else {
                continue;
            };
            let qn = node.qualified_name.as_str().to_owned();
            let replace = file_modules
                .get(&file)
                .is_none_or(|current| qn.matches('.').count() < current.matches('.').count());
            if replace {
                file_modules.insert(file, qn);
            }
        }

        let mut definitions = Vec::new();
        for node in nodes {
            if !is_definition_label(node.label.as_str()) {
                continue;
            }
            let Some(file) = node.file_path.clone() else {
                continue;
            };
            let Some(language) = node
                .properties
                .get("language")
                .and_then(Value::as_str)
                .and_then(|value| LanguageId::new(value).ok())
            else {
                continue;
            };
            let qualified_name = node.qualified_name.as_str().to_owned();
            let owner_type =
                (node.label.as_str() == "Method").then(|| owner_type(&qualified_name, &node.name));
            let module_qn = file_modules
                .get(&file)
                .cloned()
                .unwrap_or_else(|| parent_qn(&qualified_name));
            definitions.push(DefinitionRef {
                id: node.id.clone(),
                name: node.name.clone(),
                qualified_name,
                label: node.label.as_str().to_owned(),
                language,
                module_qn,
                owner_type,
            });
        }
        definitions.sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
        let mut by_name = BTreeMap::<String, Vec<usize>>::new();
        let mut by_qn = BTreeMap::new();
        for (index, definition) in definitions.iter().enumerate() {
            by_name
                .entry(binding_key(&definition.name))
                .or_default()
                .push(index);
            by_qn.insert(normalize_name(&definition.qualified_name), index);
        }
        Self {
            definitions,
            by_name,
            by_qn,
            file_modules,
        }
    }

    fn resolve_call<'a>(
        &'a self,
        call: &ExtractedCall,
        imports: &[ExtractedImport],
    ) -> Option<(&'a DefinitionRef, ResolutionStrategy)> {
        if !is_lsp_wired(call.language.as_str()) {
            return None;
        }
        let short_name = binding_key(&call.short_name);
        let callee = normalize_name(&call.callee_name);
        let candidates = self
            .candidate_indices(&short_name, &callee, imports)
            .into_iter()
            .filter_map(|index| self.definitions.get(index))
            .filter(|definition| {
                is_callable_label(&definition.label)
                    && language_compatible(call.language.as_str(), definition.language.as_str())
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return None;
        }

        if let Some(receiver_type) = &call.receiver_type {
            let expanded = expand_alias(receiver_type, imports);
            if let Some(target) = unique(candidates.iter().copied().filter(|definition| {
                definition
                    .owner_type
                    .as_ref()
                    .is_some_and(|owner| tail_eq(owner, receiver_type) || tail_eq(owner, &expanded))
            })) {
                return Some((target, ResolutionStrategy::ReceiverType));
            }
        }

        if let Some(target) = Self::resolve_via_imports(&candidates, &callee, &short_name, imports)
        {
            return Some((target, ResolutionStrategy::ImportMap));
        }

        let fallback_module = parent_qn(&call.caller_qn);
        let caller_module = self
            .file_modules
            .get(&call.file)
            .map_or(fallback_module.as_str(), String::as_str);
        let caller_owner = owner_type(&call.caller_qn, "");
        if let Some(target) = unique(candidates.iter().copied().filter(|definition| {
            definition.module_qn == caller_module
                && (definition.label != "Method"
                    || definition
                        .owner_type
                        .as_ref()
                        .is_some_and(|owner| tail_eq(owner, &caller_owner)))
        })) {
            return Some((target, ResolutionStrategy::SameContainer));
        }

        if callee.contains('.')
            && let Some(target) = unique(candidates.iter().copied().filter(|definition| {
                normalized_suffix(&definition.qualified_name, &callee)
                    || definition.owner_type.as_ref().is_some_and(|owner| {
                        let tail = format!("{}.{}", normalize_name(owner), short_name);
                        callee.ends_with(&tail) || tail.ends_with(&callee)
                    })
            }))
        {
            return Some((target, ResolutionStrategy::QualifiedSuffix));
        }

        if matches!(call.language.as_str(), "java" | "kotlin")
            && let Some(call_tail) = class_method_tail(&callee)
            && let Some(target) = unique(candidates.iter().copied().filter(|definition| {
                class_method_tail(&definition.qualified_name)
                    .is_some_and(|target_tail| target_tail == call_tail)
            }))
        {
            return Some((target, ResolutionStrategy::JvmTail));
        }

        let is_member = call_receiver(&call.callee_name).is_some();
        if !(is_builtin(call.language.as_str(), &short_name)
            || is_member && matches!(call.language.as_str(), "javascript" | "typescript" | "tsx"))
            && let Some(target) = unique(candidates.iter().copied())
        {
            return Some((target, ResolutionStrategy::UniqueName));
        }
        None
    }

    fn resolve_relation<'a>(
        &'a self,
        relation: &ExtractedRelation,
        imports: &[ExtractedImport],
    ) -> Option<&'a DefinitionRef> {
        if !is_lsp_wired(relation.language.as_str()) && relation.language.as_str() != "graphql" {
            return None;
        }
        let target_name = normalize_name(&relation.target_name);
        if let Some(index) = self.by_qn.get(&target_name)
            && let Some(target) = self.definitions.get(*index)
            && is_type_label(&target.label)
            && language_compatible(relation.language.as_str(), target.language.as_str())
        {
            return Some(target);
        }
        let short_name = binding_key(&relation.target_name);
        let candidates = self
            .candidate_indices(&short_name, &target_name, imports)
            .into_iter()
            .filter_map(|index| self.definitions.get(index))
            .filter(|definition| {
                is_type_label(&definition.label)
                    && language_compatible(relation.language.as_str(), definition.language.as_str())
            })
            .collect::<Vec<_>>();
        if let Some(target) =
            Self::resolve_via_imports(&candidates, &target_name, &short_name, imports)
        {
            return Some(target);
        }
        self.file_modules
            .get(&relation.file)
            .and_then(|module| {
                unique(
                    candidates
                        .iter()
                        .copied()
                        .filter(|definition| definition.module_qn == *module),
                )
            })
            .or_else(|| unique(candidates.iter().copied()))
    }

    fn candidate_indices(
        &self,
        short_name: &str,
        reference: &str,
        imports: &[ExtractedImport],
    ) -> Vec<usize> {
        let mut names = BTreeSet::from([short_name.to_owned()]);
        let prefix = reference.split('.').next().unwrap_or(reference);
        if !reference.contains('.') {
            for import in imports {
                let alias = binding_key(&import.alias);
                if alias == prefix || alias == short_name {
                    names.insert(binding_key(&import.module_path));
                }
            }
        }
        names
            .into_iter()
            .filter_map(|name| self.by_name.get(&name))
            .flatten()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn resolve_via_imports<'a>(
        candidates: &[&'a DefinitionRef],
        callee: &str,
        short_name: &str,
        imports: &[ExtractedImport],
    ) -> Option<&'a DefinitionRef> {
        let prefix = callee.split('.').next().unwrap_or(callee);
        let mut matches = Vec::new();
        for import in imports {
            let alias = binding_key(&import.alias);
            let module = normalize_name(&import.module_path);
            if alias != prefix && alias != short_name {
                continue;
            }
            let suffix = callee.strip_prefix(prefix).unwrap_or_default();
            let expected = if suffix.is_empty() {
                module.clone()
            } else {
                format!("{module}{suffix}")
            };
            matches.extend(candidates.iter().copied().filter(|definition| {
                normalized_suffix(&definition.qualified_name, &expected)
                    || (binding_key(&definition.name) == short_name
                        && import_reaches(&definition.qualified_name, &module))
            }));
        }
        unique(matches)
    }
}

fn unique<'a>(values: impl IntoIterator<Item = &'a DefinitionRef>) -> Option<&'a DefinitionRef> {
    let mut values = values.into_iter();
    let first = values.next()?;
    values.all(|value| value.id == first.id).then_some(first)
}

fn graph_edge(
    project: &ProjectId,
    source: NodeId,
    target: NodeId,
    kind: &str,
    discriminator: Option<String>,
    properties: GraphProperties,
) -> Result<GraphEdge, IndexError> {
    let edge = GraphEdge::new(
        project.clone(),
        source,
        target,
        EdgeKind::new(kind)?,
        Generation::new(0),
    )
    .with_properties(properties);
    match discriminator {
        Some(value) => edge.with_discriminator(value).map_err(IndexError::from),
        None => Ok(edge),
    }
}

fn is_definition_label(label: &str) -> bool {
    is_callable_label(label) || is_type_label(label)
}

fn is_callable_label(label: &str) -> bool {
    matches!(label, "Function" | "Method")
}

fn is_type_label(label: &str) -> bool {
    matches!(
        label,
        "Class" | "Struct" | "Enum" | "Trait" | "Interface" | "Type" | "TypeAlias"
    )
}

fn is_lsp_wired(language: &str) -> bool {
    matches!(
        language,
        "go" | "c"
            | "cpp"
            | "cuda"
            | "python"
            | "javascript"
            | "typescript"
            | "tsx"
            | "php"
            | "csharp"
            | "java"
            | "kotlin"
            | "rust"
    )
}

fn language_compatible(caller: &str, target: &str) -> bool {
    caller == target
        || (matches!(caller, "c" | "cpp" | "cuda") && matches!(target, "c" | "cpp" | "cuda"))
        || (matches!(caller, "javascript" | "typescript" | "tsx")
            && matches!(target, "javascript" | "typescript" | "tsx"))
        || (matches!(caller, "java" | "kotlin") && matches!(target, "java" | "kotlin"))
}

fn normalize_name(value: &str) -> String {
    value
        .trim()
        .trim_matches(|character: char| {
            character.is_whitespace() || matches!(character, '"' | '\'' | '`' | '(' | ')' | ';')
        })
        .trim_start_matches("new ")
        .replace("->", ".")
        .replace("::", ".")
        .replace(['\\', '/'], ".")
        .replace('$', "")
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join(".")
}

fn binding_key(value: &str) -> String {
    normalize_name(value)
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn owner_type(qualified_name: &str, name: &str) -> String {
    let normalized = normalize_name(qualified_name);
    let binding = binding_key(name);
    let parent = if binding.is_empty() {
        normalized.rsplit_once('.').map_or("", |(parent, _)| parent)
    } else {
        let suffix = format!(".{binding}");
        normalized
            .strip_suffix(&suffix)
            .unwrap_or_else(|| normalized.rsplit_once('.').map_or("", |(parent, _)| parent))
    };
    parent.rsplit('.').next().unwrap_or_default().to_owned()
}

fn parent_qn(qualified_name: &str) -> String {
    qualified_name
        .rsplit_once('.')
        .map_or_else(String::new, |(parent, _)| parent.to_owned())
}

fn call_receiver(callee: &str) -> Option<String> {
    let normalized = normalize_name(callee);
    normalized
        .rsplit_once('.')
        .map(|(receiver, _)| receiver.to_owned())
        .filter(|receiver| !receiver.is_empty())
}

fn expand_alias(value: &str, imports: &[ExtractedImport]) -> String {
    let normalized = normalize_name(value);
    let prefix = normalized.split('.').next().unwrap_or_default();
    imports
        .iter()
        .find(|import| binding_key(&import.alias) == prefix)
        .map_or(normalized.clone(), |import| {
            let suffix = normalized.strip_prefix(prefix).unwrap_or_default();
            format!("{}{suffix}", normalize_name(&import.module_path))
        })
}

fn tail_eq(left: &str, right: &str) -> bool {
    let left = normalize_name(left);
    let right = normalize_name(right);
    left == right || left.ends_with(&format!(".{right}")) || right.ends_with(&format!(".{left}"))
}

fn normalized_suffix(qualified_name: &str, suffix: &str) -> bool {
    let qualified_name = normalize_name(qualified_name);
    let suffix = normalize_name(suffix);
    qualified_name == suffix || qualified_name.ends_with(&format!(".{suffix}"))
}

fn import_reaches(qualified_name: &str, module: &str) -> bool {
    let qualified_name = normalize_name(qualified_name);
    let module = normalize_name(module);
    qualified_name.starts_with(&format!("{module}."))
        || qualified_name.contains(&format!(".{module}."))
        || qualified_name.ends_with(&format!(".{module}"))
}

fn class_method_tail(value: &str) -> Option<String> {
    let normalized = normalize_name(value);
    let mut parts = normalized.rsplit('.');
    let method = parts.next()?;
    let class = parts.next()?;
    Some(format!("{class}.{method}"))
}

fn is_builtin(language: &str, name: &str) -> bool {
    match language {
        "python" => matches!(
            name,
            "abs"
                | "all"
                | "any"
                | "dict"
                | "enumerate"
                | "filter"
                | "len"
                | "list"
                | "map"
                | "max"
                | "min"
                | "open"
                | "print"
                | "range"
                | "set"
                | "sorted"
                | "str"
                | "sum"
                | "tuple"
                | "zip"
        ),
        "javascript" | "typescript" | "tsx" => matches!(
            name,
            "alert"
                | "clearInterval"
                | "clearTimeout"
                | "fetch"
                | "parseFloat"
                | "parseInt"
                | "setInterval"
                | "setTimeout"
        ),
        "go" => matches!(
            name,
            "append"
                | "cap"
                | "clear"
                | "close"
                | "complex"
                | "copy"
                | "delete"
                | "len"
                | "make"
                | "max"
                | "min"
                | "new"
                | "panic"
                | "print"
                | "println"
                | "recover"
        ),
        "c" | "cpp" | "cuda" => matches!(
            name,
            "calloc"
                | "free"
                | "malloc"
                | "memcpy"
                | "memset"
                | "printf"
                | "puts"
                | "realloc"
                | "sizeof"
                | "snprintf"
                | "strcmp"
                | "strlen"
        ),
        "rust" => matches!(
            name,
            "drop" | "panic" | "print" | "println" | "todo" | "unreachable"
        ),
        "php" => matches!(
            name,
            "array" | "count" | "echo" | "isset" | "print" | "strlen"
        ),
        _ => false,
    }
}
