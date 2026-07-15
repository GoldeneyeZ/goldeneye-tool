mod calls;
mod classify;
mod graph;
mod imports;
mod relations;

use calls::{
    audited_call_target, call_receiver, call_short_name, generic_call_target, is_call,
    last_identifier, receiver_looks_like_type, receiver_type,
};
use classify::{Definition, Scope, ScopeKind, classify, gomod_requirement_name};
use graph::{
    graph_edge, graph_node, module_name, path_stem, project_node_id, qualified_segment,
    source_span, stable_node_id,
};

use imports::{
    binding_key, embedded_es_imports, import_alias, import_bindings, infer_declared_type,
    normalize_import_path,
};
use relations::audited_relations;

use std::collections::BTreeMap;
use std::sync::Arc;

use goldeneye_domain::{
    Generation, GraphEdge, GraphNode, GraphProperties, LanguageId, NodeId, ProjectId,
    ProjectRelativePath,
};
use goldeneye_ports::{
    IndexDiagnosticKind, IndexExtractedCall as ExtractedCall, IndexExtractedFile as ExtractedFile,
    IndexExtractedImport as ExtractedImport, IndexExtractedRelation as ExtractedRelation,
    IndexExtractionRequest as Candidate, IndexFileSyntaxDiagnostics as FileSyntaxDiagnostics,
    IndexMode, IndexSyntaxDiagnostic,
};
use goldeneye_syntax::{DiagnosticKind, GrammarProvider, SyntaxEngine, SyntaxSnapshot};
use serde_json::{Value, json};
use tree_sitter::Node;

use crate::error::ExtractionError as IndexError;
use crate::language_specs::language_spec;

const MAX_PENDING_CALLS_PER_FILE: usize = 4_096;
const MAX_PENDING_RELATIONS_PER_FILE: usize = 1_024;
const MAX_PENDING_IMPORTS_PER_FILE: usize = 1_024;
const MAX_TYPE_BINDINGS_PER_SCOPE: usize = 2_048;

pub(crate) fn extract<P>(
    provider: P,
    candidate: Candidate,
    mode: IndexMode,
) -> Result<ExtractedFile, IndexError>
where
    P: GrammarProvider,
{
    let snapshot = SyntaxEngine::new(provider)
        .parse(
            candidate.language.clone(),
            Arc::clone(&candidate.source),
            Generation::new(0),
        )
        .map_err(|source| IndexError::Syntax {
            path: candidate.record.id.path.clone(),
            source,
        })?;
    let diagnostics = snapshot.has_errors().then(|| FileSyntaxDiagnostics {
        path: candidate.record.id.path.clone(),
        total: snapshot.diagnostic_total(),
        truncated: snapshot.diagnostics_truncated(),
        details: snapshot
            .diagnostics()
            .iter()
            .map(|diagnostic| IndexSyntaxDiagnostic {
                kind: match diagnostic.kind {
                    DiagnosticKind::Error => IndexDiagnosticKind::Error,
                    DiagnosticKind::Missing => IndexDiagnosticKind::Missing,
                },
                node_kind: diagnostic.node_kind.clone(),
                span: diagnostic.span,
            })
            .collect(),
    });
    let mut extractor = Extractor::new(
        &candidate.record.id.project,
        &candidate.record.id.path,
        &candidate.language,
        &snapshot,
        mode,
    )?;
    extractor.run()?;
    let nodes = extractor.nodes;
    let edges = extractor.edges;
    let calls = extractor.pending_calls;
    let relations = extractor.pending_relations;
    let imports = extractor.pending_imports;
    Ok(ExtractedFile {
        record: candidate.record,
        source: candidate.source,
        nodes,
        edges,
        calls,
        relations,
        imports,
        diagnostics,
    })
}

struct Extractor<'a> {
    project: &'a ProjectId,
    path: &'a ProjectRelativePath,
    language: &'a LanguageId,
    snapshot: &'a SyntaxSnapshot,
    mode: IndexMode,
    source: &'a [u8],
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    qualified_name_counts: BTreeMap<String, usize>,
    callable_definitions: BTreeMap<String, Vec<NodeId>>,
    type_scopes: BTreeMap<String, Vec<Scope>>,
    type_bindings: BTreeMap<NodeId, BTreeMap<String, String>>,
    pending_calls: Vec<ExtractedCall>,
    pending_relations: Vec<ExtractedRelation>,
    pending_imports: Vec<ExtractedImport>,
    module_scope: Scope,
}

impl<'a> Extractor<'a> {
    fn new(
        project: &'a ProjectId,
        path: &'a ProjectRelativePath,
        language: &'a LanguageId,
        snapshot: &'a SyntaxSnapshot,
        mode: IndexMode,
    ) -> Result<Self, IndexError> {
        let source = snapshot.source();
        let root = snapshot.root();
        let path_stem = path_stem(path);
        let module_name = module_name(path, language);
        let module_qualified_name = if module_name.is_empty() {
            project.as_str().to_owned()
        } else {
            format!("{}.{}", project.as_str(), module_name)
        };
        let file_qualified_name = format!("{}.{}.__file__", project.as_str(), path_stem);
        let file_id = stable_node_id("File", &file_qualified_name)?;
        let root_span = source_span(root)?;

        let mut nodes = Vec::with_capacity(32);
        nodes.push(graph_node(
            project,
            path,
            language,
            "File",
            path.as_str().rsplit('/').next().unwrap_or(path.as_str()),
            &file_qualified_name,
            "file",
            root_span,
        )?);
        let (module_id, edges) = if module_name.is_empty() {
            let project_id = project_node_id(project)?;
            (
                project_id.clone(),
                vec![graph_edge(
                    project,
                    file_id,
                    project_id,
                    "DEFINES",
                    None,
                    GraphProperties::new(),
                )?],
            )
        } else {
            let module_id = stable_node_id("Module", &module_qualified_name)?;
            nodes.push(graph_node(
                project,
                path,
                language,
                "Module",
                module_name.rsplit('.').next().unwrap_or(&module_name),
                &module_qualified_name,
                root.kind(),
                root_span,
            )?);
            (
                module_id.clone(),
                vec![graph_edge(
                    project,
                    file_id,
                    module_id,
                    "DEFINES",
                    None,
                    GraphProperties::new(),
                )?],
            )
        };
        Ok(Self {
            project,
            path,
            language,
            snapshot,
            mode,
            source,
            nodes,
            edges,
            qualified_name_counts: BTreeMap::new(),
            callable_definitions: BTreeMap::new(),
            type_scopes: BTreeMap::new(),
            type_bindings: BTreeMap::new(),
            pending_calls: Vec::new(),
            pending_relations: Vec::new(),
            pending_imports: Vec::new(),
            module_scope: Scope {
                parent: module_id,
                qualified_name: module_qualified_name,
                kind: ScopeKind::Module,
                callable: None,
            },
        })
    }

    fn run(&mut self) -> Result<(), IndexError> {
        let root = self.snapshot.root();
        let scope = self.module_scope.clone();
        if self.mode != IndexMode::Fast {
            for name in embedded_es_imports(self.language.as_str(), self.source) {
                self.add_definition(
                    root,
                    Definition {
                        label: "Import",
                        name,
                    },
                    &scope,
                )?;
            }
        }
        let root_is_definition = self.mode != IndexMode::Fast
            && language_spec(self.language.as_str()).is_some_and(|spec| {
                let kind = root.kind();
                !spec.module_kinds.contains(&kind)
                    && (spec.function_kinds.contains(&kind)
                        || spec.class_kinds.contains(&kind)
                        || spec.field_kinds.contains(&kind)
                        || spec.variable_kinds.contains(&kind)
                        || spec.assignment_kinds.contains(&kind))
            });
        if root_is_definition {
            self.walk(root, &scope)?;
        } else {
            let mut cursor = root.walk();
            for child in root.named_children(&mut cursor) {
                self.walk(child, &scope)?;
            }
        }
        self.resolve_calls()?;
        self.resolve_relations()?;
        self.pending_imports.sort();
        self.pending_imports.dedup();
        Ok(())
    }

    fn walk(&mut self, node: Node<'_>, scope: &Scope) -> Result<(), IndexError> {
        if self.language.as_str() == "rust" && node.kind() == "impl_item" {
            let impl_scope = node
                .child_by_field_name("type")
                .map(|type_node| self.node_text(type_node))
                .and_then(|name| self.unique_type_scope(&last_identifier(&name)))
                .unwrap_or_else(|| scope.clone());
            return self.walk_children(node, &impl_scope);
        }

        if self.mode != IndexMode::Fast
            && self.language.as_str() == "gomod"
            && node.kind() == "require_directive"
            && let Some(name) = gomod_requirement_name(&self.node_text(node))
        {
            self.add_definition(
                node,
                Definition {
                    label: "Import",
                    name,
                },
                scope,
            )?;
        }

        if is_call(self.mode, self.language.as_str(), node.kind()) {
            self.record_call(node, scope)?;
        }

        let effective_scope =
            if self.language.as_str() == "go" && node.kind() == "method_declaration" {
                receiver_type(node, self.source)
                    .and_then(|name| self.unique_type_scope(&name))
                    .unwrap_or_else(|| scope.clone())
            } else {
                scope.clone()
            };

        if let Some(definition) = classify(
            self.mode,
            self.language.as_str(),
            node,
            &effective_scope,
            self.source,
        ) {
            let next_scope = self.add_definition(node, definition, &effective_scope)?;
            return self.walk_children(node, next_scope.as_ref().unwrap_or(&effective_scope));
        }
        self.walk_children(node, &effective_scope)
    }

    fn walk_children(&mut self, node: Node<'_>, scope: &Scope) -> Result<(), IndexError> {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.walk(child, scope)?;
        }
        Ok(())
    }

    fn add_definition(
        &mut self,
        node: Node<'_>,
        definition: Definition,
        scope: &Scope,
    ) -> Result<Option<Scope>, IndexError> {
        if definition.label == "Import" {
            self.record_imports(node, &definition.name);
        }
        if matches!(definition.label, "Variable" | "Field") {
            self.record_type_binding(node, &definition.name, scope);
        }
        let segment = qualified_segment(&definition.name);
        let base = if definition.label == "Import" {
            format!(
                "{}.__imports__.{}#{}",
                scope.qualified_name,
                segment,
                node.start_byte()
            )
        } else {
            format!("{}.{}", scope.qualified_name, segment)
        };
        let count = self.qualified_name_counts.entry(base.clone()).or_default();
        *count += 1;
        let qualified_name = if *count == 1 {
            base
        } else {
            format!("{base}#{count}")
        };
        let id = stable_node_id(definition.label, &qualified_name)?;
        let span = source_span(node)?;
        let graph_node = graph_node(
            self.project,
            self.path,
            self.language,
            definition.label,
            &definition.name,
            &qualified_name,
            node.kind(),
            span,
        )?;
        let relation = if matches!(definition.label, "Field" | "Variable")
            && scope.kind != ScopeKind::Module
        {
            "CONTAINS"
        } else if definition.label == "Import" {
            "IMPORTS"
        } else {
            "DEFINES"
        };
        self.edges.push(graph_edge(
            self.project,
            scope.parent.clone(),
            id.clone(),
            relation,
            None,
            GraphProperties::new(),
        )?);
        self.nodes.push(graph_node);

        if matches!(definition.label, "Function" | "Method") {
            self.callable_definitions
                .entry(definition.name)
                .or_default()
                .push(id.clone());
            return Ok(Some(Scope {
                parent: id.clone(),
                qualified_name,
                kind: ScopeKind::Callable,
                callable: Some(id),
            }));
        }
        if matches!(
            definition.label,
            "Class" | "Struct" | "Enum" | "Trait" | "Interface" | "Type" | "TypeAlias"
        ) {
            for (kind, target_name) in audited_relations(self.language.as_str(), node, self.source)
            {
                if self.pending_relations.len() >= MAX_PENDING_RELATIONS_PER_FILE {
                    break;
                }
                self.pending_relations.push(ExtractedRelation {
                    source: id.clone(),
                    file: self.path.clone(),
                    language: self.language.clone(),
                    kind,
                    target_name,
                });
            }
            let type_scope = Scope {
                parent: id,
                qualified_name,
                kind: ScopeKind::Type,
                callable: scope.callable.clone(),
            };
            self.type_scopes
                .entry(definition.name)
                .or_default()
                .push(type_scope.clone());
            return Ok(Some(type_scope));
        }
        Ok(None)
    }

    fn record_imports(&mut self, node: Node<'_>, fallback_name: &str) {
        if self.pending_imports.len() >= MAX_PENDING_IMPORTS_PER_FILE {
            return;
        }
        let text = self.node_text(node);
        let mut imports = import_bindings(self.language.as_str(), &text);
        if imports.is_empty() {
            let module_path = normalize_import_path(fallback_name);
            if !module_path.is_empty() {
                imports.push((import_alias(&module_path), module_path));
            }
        }
        for (alias, module_path) in imports {
            if self.pending_imports.len() >= MAX_PENDING_IMPORTS_PER_FILE {
                break;
            }
            if alias.is_empty() || module_path.is_empty() {
                continue;
            }
            self.pending_imports.push(ExtractedImport {
                file: self.path.clone(),
                language: self.language.clone(),
                alias,
                module_path,
            });
        }
    }

    fn record_type_binding(&mut self, node: Node<'_>, name: &str, scope: &Scope) {
        let Some(type_name) = infer_declared_type(&self.node_text(node), name) else {
            return;
        };
        let bindings = self.type_bindings.entry(scope.parent.clone()).or_default();
        if bindings.len() >= MAX_TYPE_BINDINGS_PER_SCOPE {
            return;
        }
        bindings.insert(binding_key(name), type_name);
    }

    fn record_call(&mut self, node: Node<'_>, scope: &Scope) -> Result<(), IndexError> {
        if self.language.as_str() == "nasm"
            && node.kind() == "actual_instruction"
            && node
                .child_by_field_name("instruction")
                .map(|instruction| self.node_text(instruction))
                .as_deref()
                != Some("call")
        {
            return Ok(());
        }
        let source = if let Some(callable) = scope.callable.clone() {
            callable
        } else if self.mode != IndexMode::Fast {
            scope.parent.clone()
        } else {
            return Ok(());
        };
        let callee = node.child_by_field_name("function").or_else(|| {
            (self.mode != IndexMode::Fast)
                .then(|| {
                    language_spec(self.language.as_str()).map_or_else(
                        || generic_call_target(node),
                        |_| audited_call_target(self.language.as_str(), node),
                    )
                })
                .flatten()
        });
        let Some(callee) = callee else {
            return Ok(());
        };
        let (text, short_name) =
            if self.language.as_str() == "puppet" && node.kind() == "include_statement" {
                (self.node_text(node), "include".to_owned())
            } else {
                (self.node_text(callee), call_short_name(callee, self.source))
            };
        if short_name.is_empty() {
            return Ok(());
        }
        if self.pending_calls.len() >= MAX_PENDING_CALLS_PER_FILE {
            return Ok(());
        }
        let receiver_type = call_receiver(&text).and_then(|receiver| {
            self.type_bindings
                .get(&source)
                .and_then(|bindings| bindings.get(&binding_key(receiver)))
                .cloned()
                .or_else(|| receiver_looks_like_type(receiver).then(|| receiver.to_owned()))
        });
        self.pending_calls.push(ExtractedCall {
            source,
            file: self.path.clone(),
            language: self.language.clone(),
            caller_qn: scope.qualified_name.clone(),
            callee_name: text.clone(),
            short_name,
            receiver_type,
            start_byte: u64::try_from(node.start_byte())
                .map_err(|_| IndexError::CoordinateOverflow("call start byte"))?,
            line: u64::try_from(node.start_position().row)
                .map_err(|_| IndexError::CoordinateOverflow("call row"))?
                .checked_add(1)
                .ok_or(IndexError::CoordinateOverflow("call line"))?,
            text,
        });
        Ok(())
    }

    fn resolve_relations(&mut self) -> Result<(), IndexError> {
        self.pending_relations.sort();
        self.pending_relations.dedup();
        for relation in &self.pending_relations {
            let target = last_identifier(&relation.target_name);
            let Some(target_scope) = self
                .type_scopes
                .get(&target)
                .and_then(|scopes| scopes.last())
                .cloned()
            else {
                continue;
            };
            self.edges.push(graph_edge(
                self.project,
                relation.source.clone(),
                target_scope.parent,
                relation.kind,
                Some(relation.target_name.clone()),
                GraphProperties::new(),
            )?);
        }
        Ok(())
    }

    fn resolve_calls(&mut self) -> Result<(), IndexError> {
        self.pending_calls.sort_by(|left, right| {
            (&left.source, left.start_byte, &left.short_name).cmp(&(
                &right.source,
                right.start_byte,
                &right.short_name,
            ))
        });
        self.pending_calls.dedup_by(|left, right| {
            left.source == right.source
                && left.start_byte == right.start_byte
                && left.short_name == right.short_name
        });
        for call in &self.pending_calls {
            let Some(targets) = self.callable_definitions.get(&call.short_name) else {
                continue;
            };
            if targets.len() != 1 {
                continue;
            }
            let mut properties = GraphProperties::new();
            properties.insert("callee".into(), Value::String(call.text.clone()));
            properties.insert("line".into(), json!(call.line));
            self.edges.push(graph_edge(
                self.project,
                call.source.clone(),
                targets[0].clone(),
                "CALLS",
                Some(call.start_byte.to_string()),
                properties,
            )?);
        }
        Ok(())
    }

    fn unique_type_scope(&self, name: &str) -> Option<Scope> {
        self.type_scopes
            .get(name)
            .filter(|scopes| scopes.len() == 1)
            .and_then(|scopes| scopes.first())
            .cloned()
    }

    fn node_text(&self, node: Node<'_>) -> String {
        node_text(node, self.source)
    }
}

fn first_quoted_value(text: &str) -> Option<String> {
    let (start, quote) = text
        .char_indices()
        .find(|(_, character)| matches!(character, '"' | '\''))?;
    let value = &text[start + quote.len_utf8()..];
    let end = value.find(quote)?;
    Some(value[..end].to_owned())
}

fn node_text(node: Node<'_>, source: &[u8]) -> String {
    source
        .get(node.byte_range())
        .map_or_else(String::new, |value| {
            String::from_utf8_lossy(value).into_owned()
        })
}

#[cfg(all(test, feature = "full-grammar-tests"))]
mod full_language_tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    use goldeneye_domain::{
        ContentHash, FileId, FileRecord, Generation, LanguageId, ProjectId, ProjectRelativePath,
    };
    use goldeneye_ports::IndexMode;
    use goldeneye_syntax::FullGrammarProvider;

    use super::{Candidate, extract};

    mod fixtures {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/full_language_fixtures.rs"
        ));
    }

    #[test]
    fn audited_corpus_matches_upstream_definition_and_raw_call_expectations() {
        let mut missing_labels = BTreeMap::<String, Vec<String>>::new();
        let mut missing_callees = BTreeMap::<String, Vec<String>>::new();
        let mut missing_imports = Vec::new();
        let mut missing_relations = BTreeMap::<String, Vec<String>>::new();

        for fixture in fixtures::LANGUAGE_FIXTURES {
            let source = Arc::<[u8]>::from(fixture.source.as_bytes());
            let project =
                ProjectId::new(format!("corpus-{}", fixture.language)).expect("fixture project ID");
            let path = ProjectRelativePath::new(fixture.path).expect("fixture path");
            let byte_len = u64::try_from(source.len()).expect("fixture byte length");
            let extracted = extract(
                FullGrammarProvider,
                Candidate {
                    record: FileRecord::new(
                        FileId::new(project, path),
                        ContentHash::of(source.as_ref()),
                        Generation::new(0),
                        0,
                        byte_len,
                    ),
                    language: LanguageId::new(fixture.language).expect("fixture language ID"),
                    source,
                },
                IndexMode::Full,
            )
            .unwrap_or_else(|error| panic!("{} extraction failed: {error}", fixture.language));

            let labels = extracted
                .nodes
                .iter()
                .map(|node| node.label.as_str())
                .collect::<BTreeSet<_>>();
            for expected in fixture.expected_labels {
                if !labels.contains(expected) {
                    missing_labels
                        .entry((*expected).to_owned())
                        .or_default()
                        .push(fixture.language.to_owned());
                }
            }
            if fixture.expects_import && !labels.contains("Import") {
                missing_imports.push(fixture.language.to_owned());
            }
            for (kind, targets) in [
                ("INHERITS", fixture.expected_inherits),
                ("IMPLEMENTS", fixture.expected_implements),
            ] {
                for target in targets {
                    if !extracted.relations.iter().any(|relation| {
                        relation.kind == kind && relation.target_name.contains(target)
                    }) {
                        missing_relations
                            .entry(kind.to_owned())
                            .or_default()
                            .push(format!("{} -> {target}", fixture.language));
                    }
                }
            }

            if let Some(callee) = fixture.callee
                && !extracted
                    .calls
                    .iter()
                    .any(|call| call.text.contains(callee))
            {
                missing_callees.insert(
                    fixture.language.to_owned(),
                    extracted
                        .calls
                        .iter()
                        .map(|call| format!("{} <- {}", call.short_name, call.text))
                        .collect(),
                );
            }
        }

        assert!(
            missing_labels.is_empty(),
            "missing expected labels: {missing_labels:#?}"
        );
        assert!(
            missing_callees.is_empty(),
            "missing expected raw callees: {missing_callees:#?}"
        );
        assert!(
            missing_imports.is_empty(),
            "missing expected imports: {missing_imports:#?}"
        );
        assert!(
            missing_relations.is_empty(),
            "missing expected relations: {missing_relations:#?}"
        );
    }
}
