use std::collections::BTreeMap;
use std::sync::Arc;

use goldeneye_discovery::IndexMode;
use goldeneye_domain::{
    ByteSpan, EdgeKind, FileRecord, Generation, GraphEdge, GraphNode, GraphProperties, LanguageId,
    NodeId, NodeLabel, ProjectId, ProjectRelativePath, QualifiedName, SourcePoint, SourceSpan,
};
use goldeneye_syntax::{GrammarProvider, SyntaxEngine, SyntaxSnapshot};
use serde_json::{Value, json};
use tree_sitter::Node;

use crate::language_specs::{LanguageSpec, language_spec};
use crate::{FileSyntaxDiagnostics, IndexError};

#[derive(Clone)]
pub(crate) struct Candidate {
    pub record: FileRecord,
    pub language: LanguageId,
    pub source: Arc<[u8]>,
}

pub(crate) struct ExtractedFile {
    pub record: FileRecord,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub calls: Vec<ExtractedCall>,
    pub relations: Vec<ExtractedRelation>,
    pub diagnostics: Option<FileSyntaxDiagnostics>,
}

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
        details: snapshot.diagnostics().to_vec(),
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
    Ok(ExtractedFile {
        record: candidate.record,
        nodes,
        edges,
        calls,
        relations,
        diagnostics,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScopeKind {
    Module,
    Type,
    Callable,
}

#[derive(Debug, Clone)]
struct Scope {
    parent: NodeId,
    qualified_name: String,
    kind: ScopeKind,
    callable: Option<NodeId>,
}

struct Definition {
    label: &'static str,
    name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ExtractedCall {
    source: NodeId,
    short_name: String,
    start_byte: u64,
    line: u64,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ExtractedRelation {
    source: NodeId,
    kind: &'static str,
    target_name: String,
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
    pending_calls: Vec<ExtractedCall>,
    pending_relations: Vec<ExtractedRelation>,
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
            pending_calls: Vec::new(),
            pending_relations: Vec::new(),
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
        self.resolve_relations()
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
        let segment = qualified_segment(&definition.name);
        let base = format!("{}.{}", scope.qualified_name, segment);
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
                self.pending_relations.push(ExtractedRelation {
                    source: id.clone(),
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
        self.pending_calls.push(ExtractedCall {
            source,
            short_name,
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

fn classify(
    mode: IndexMode,
    language: &str,
    node: Node<'_>,
    scope: &Scope,
    source: &[u8],
) -> Option<Definition> {
    if let Some(definition) = classify_known(language, node, scope, source) {
        return Some(definition);
    }
    if mode == IndexMode::Fast {
        return None;
    }
    language_spec(language).map_or_else(
        || classify_generic(node, scope, source),
        |spec| classify_audited(spec, language, node, scope, source),
    )
}

fn classify_audited(
    spec: &LanguageSpec,
    language: &str,
    node: Node<'_>,
    scope: &Scope,
    source: &[u8],
) -> Option<Definition> {
    let kind = node.kind();

    if let Some(name) = audited_special_import_name(language, node, source) {
        return Some(Definition {
            label: "Import",
            name,
        });
    }

    // In Elixir, definitions, imports, branches, assignments, and ordinary calls
    // all share the grammar's `call`/`binary_operator` nodes. The upstream
    // extractor distinguishes the defining macro before applying its tables.
    if language == "elixir" && kind == "call" {
        return classify_elixir_call(node, source);
    }
    if matches!(language, "k8s" | "kustomize") && kind == "block_mapping_pair" {
        return classify_kubernetes_resource(node, source);
    }
    if language == "meson" && kind == "operatorunit" {
        let text = node_text(node, source);
        if text.contains("= func") {
            let name_node = node.named_child(0).and_then(first_name_like)?;
            let name = node_text(name_node, source);
            return (!name.is_empty()).then_some(Definition {
                label: "Function",
                name,
            });
        }
    }

    let label = if spec.function_kinds.contains(&kind) {
        if matches!(language, "capnp" | "protobuf" | "smali") {
            "Function"
        } else if scope.kind == ScopeKind::Type {
            "Method"
        } else {
            "Function"
        }
    } else if spec.class_kinds.contains(&kind) {
        audited_class_label(language, node, source)
    } else if spec.field_kinds.contains(&kind) {
        "Field"
    } else if spec.module_kinds.contains(&kind) && node.parent().is_some() {
        "Module"
    } else if spec.import_from_kinds.contains(&kind)
        || (spec.import_kinds.contains(&kind)
            && !spec.call_kinds.contains(&kind)
            && !spec.branch_kinds.contains(&kind)
            && !spec.throw_kinds.contains(&kind)
            && !spec.decorator_kinds.contains(&kind))
    {
        "Import"
    } else if spec.variable_kinds.contains(&kind) || spec.assignment_kinds.contains(&kind) {
        if scope.kind == ScopeKind::Type {
            "Field"
        } else {
            "Variable"
        }
    } else {
        return None;
    };

    audited_definition_name(language, node, label, source).map(|name| Definition { label, name })
}

fn audited_special_import_name(language: &str, node: Node<'_>, source: &[u8]) -> Option<String> {
    let kind = node.kind();
    let text = node_text(node, source);
    let keyword = match language {
        "crystal" if kind == "require" => "require",
        "dart" if matches!(kind, "import_or_export" | "import") => "import",
        "kotlin" if matches!(kind, "import" | "import_header") => "import",
        "puppet" if matches!(kind, "include_statement" | "include") => "include",
        "puppet" if matches!(kind, "require_statement" | "require") => "require",
        "r" if kind == "call" => [
            "library",
            "require",
            "requireNamespace",
            "loadNamespace",
            "source",
        ]
        .into_iter()
        .find(|keyword| starts_with_call(&text, keyword))?,
        "ruby" if matches!(kind, "call" | "command_call") => ["require_relative", "require"]
            .into_iter()
            .find(|keyword| starts_with_call(&text, keyword))?,
        _ => return None,
    };
    import_name_after_keyword(&text, keyword)
}

fn starts_with_call(text: &str, keyword: &str) -> bool {
    let Some(rest) = text.trim_start().strip_prefix(keyword) else {
        return false;
    };
    rest.starts_with(|character: char| character.is_whitespace() || character == '(')
}

fn import_name_after_keyword(text: &str, keyword: &str) -> Option<String> {
    let rest = text.trim_start().strip_prefix(keyword)?.trim_start();
    if let Some(value) = first_quoted_value(rest) {
        return Some(value);
    }
    let name = rest
        .trim_start_matches('(')
        .trim_start()
        .split(|character: char| {
            character.is_whitespace()
                || matches!(character, ')' | ';' | ',' | '{' | '[' | '\n' | '\r')
        })
        .next()
        .unwrap_or_default()
        .trim_matches(|character: char| matches!(character, '"' | '\'' | '(' | ')'));
    (!name.is_empty()).then(|| name.to_owned())
}

fn first_quoted_value(text: &str) -> Option<String> {
    let (start, quote) = text
        .char_indices()
        .find(|(_, character)| matches!(character, '"' | '\''))?;
    let value = &text[start + quote.len_utf8()..];
    let end = value.find(quote)?;
    Some(value[..end].to_owned())
}

fn classify_kubernetes_resource(node: Node<'_>, source: &[u8]) -> Option<Definition> {
    let key = node
        .child_by_field_name("key")
        .or_else(|| node.named_child(0))?;
    if node_text(key, source).trim() != "kind" {
        return None;
    }
    let value = node
        .child_by_field_name("value")
        .or_else(|| node.named_child(1))?;
    let name = node_text(value, source)
        .trim_matches(|character: char| {
            character.is_whitespace() || matches!(character, '"' | '\'')
        })
        .to_owned();
    (!name.is_empty()).then_some(Definition {
        label: "Resource",
        name,
    })
}

fn audited_class_label(language: &str, node: Node<'_>, source: &[u8]) -> &'static str {
    let kind = node.kind();
    if language == "markdown" && matches!(kind, "atx_heading" | "setext_heading") {
        return "Section";
    }
    if matches!(language, "sway" | "wgsl") && matches!(kind, "struct_item" | "struct_declaration") {
        return "Struct";
    }
    if language == "sway" && kind == "abi_item" {
        return "Interface";
    }
    if matches!(language, "swift" | "dlang") && matches!(kind, "struct_item" | "struct_declaration")
    {
        return "Struct";
    }
    if language == "swift" && kind == "class_declaration" {
        let declaration_kind = node
            .child_by_field_name("declaration_kind")
            .map(|value| node_text(value, source));
        if declaration_kind.as_deref() == Some("struct") {
            return "Struct";
        }
    }
    if matches!(
        kind,
        "interface_declaration"
            | "interface_type"
            | "trait_item"
            | "trait_definition"
            | "protocol_declaration"
    ) {
        "Interface"
    } else if matches!(kind, "enum_specifier" | "enum_declaration" | "enum_item") {
        "Enum"
    } else if matches!(
        kind,
        "type_alias_declaration" | "type_item" | "type_alias" | "type_definition"
    ) {
        "Type"
    } else {
        "Class"
    }
}

fn classify_elixir_call(node: Node<'_>, source: &[u8]) -> Option<Definition> {
    let callee = node.child(0)?;
    let macro_name = node_text(callee, source);
    let label = match macro_name.as_str() {
        "def" | "defp" | "defmacro" | "defmacrop" => "Function",
        "defmodule" | "defprotocol" | "defimpl" => "Class",
        _ => return None,
    };
    let argument = node
        .child_by_field_name("arguments")
        .or_else(|| node.named_child(1))?;
    let name_node = if label == "Function" {
        find_descendant_kind(argument, &["identifier"])
    } else {
        find_descendant_kind(argument, &["alias", "identifier"])
    }?;
    let name = node_text(name_node, source).trim().to_owned();
    (!name.is_empty()).then_some(Definition { label, name })
}

fn audited_definition_name(
    language: &str,
    node: Node<'_>,
    label: &str,
    source: &[u8],
) -> Option<String> {
    let name_node = audited_name_node(language, node, label)?;
    let raw_name = if matches!(label, "Variable" | "Field") {
        first_name_like(name_node).map_or_else(
            || node_text(name_node, source),
            |value| node_text(value, source),
        )
    } else {
        node_text(name_node, source)
    };
    let name = raw_name
        .trim_matches(|character: char| {
            matches!(character, '"' | '\'' | '`' | '<' | '>' | ':' | ';')
        })
        .trim();
    (!name.is_empty()).then(|| name.to_owned())
}

fn audited_name_node<'tree>(language: &str, node: Node<'tree>, label: &str) -> Option<Node<'tree>> {
    let kind = node.kind();
    if language == "zig" && kind == "test_declaration" {
        return find_descendant_kind(node, &["string_content"]);
    }
    if language == "zig"
        && matches!(
            kind,
            "struct_declaration" | "enum_declaration" | "union_declaration"
        )
        && let Some(parent) = node.parent()
        && parent.kind() == "variable_declaration"
    {
        return find_descendant_kind(parent, &["identifier"]);
    }
    if language == "lean" {
        if let Some(decl_id) = node.child_by_field_name("declId") {
            return first_name_like(decl_id).or(Some(decl_id));
        }
    }
    if language == "haskell" && matches!(label, "Function" | "Method") {
        return find_descendant_kind(node, &["variable", "name", "identifier"]);
    }
    if language == "commonlisp" && matches!(label, "Function" | "Method") {
        return find_descendant_kind(node, &["function_name", "sym_lit", "symbol"]);
    }
    if language == "makefile" && kind == "rule" {
        return find_descendant_kind(node, &["word"]);
    }
    if language == "meson" && kind == "function_expression" {
        return ancestor_kind(node, "assignment_statement", 3)
            .or_else(|| ancestor_kind(node, "assignment", 3))
            .and_then(|assignment| {
                assignment
                    .child_by_field_name("left")
                    .or_else(|| assignment.named_child(0))
            })
            .and_then(|left| first_name_like(left).or(Some(left)));
    }
    if language == "elm" && kind == "value_declaration" {
        return node
            .child_by_field_name("functionDeclarationLeft")
            .or_else(|| find_descendant_kind(node, &["function_declaration_left"]))
            .and_then(|left| find_descendant_kind(left, &["lower_case_identifier"]));
    }
    if language == "ocaml" && kind == "value_definition" {
        return find_descendant_kind(node, &["let_binding"])
            .and_then(|binding| binding.child_by_field_name("pattern"));
    }
    if language == "rescript" && kind == "function" {
        return ancestor_kind(node, "let_binding", 3)
            .and_then(|binding| binding.child_by_field_name("pattern"));
    }
    if language == "nickel" && kind == "fun_expr" {
        return ancestor_kind(node, "let_binding", 8)
            .and_then(|binding| binding.child_by_field_name("pat"))
            .and_then(|pattern| pattern.child_by_field_name("pat").or(Some(pattern)));
    }
    if language == "nix" && kind == "function_expression" {
        return ancestor_kind(node, "binding", 2)
            .and_then(|binding| binding.child_by_field_name("attrpath"))
            .and_then(|path| path.child_by_field_name("attr").or(Some(path)));
    }
    if language == "r" && kind == "function_definition" {
        return node.parent().and_then(|parent| {
            (parent.kind() == "binary_operator")
                .then(|| {
                    parent
                        .child_by_field_name("lhs")
                        .or_else(|| parent.named_child(0))
                })
                .flatten()
        });
    }
    if language == "lua" && kind == "function_definition" {
        let parent = node.parent().and_then(|parent| {
            (parent.kind() == "expression_list")
                .then(|| parent.parent())
                .flatten()
                .or(Some(parent))
        });
        if let Some(parent) = parent.filter(|parent| parent.kind() == "assignment_statement") {
            return parent
                .child_by_field_name("variables")
                .or_else(|| find_descendant_kind(parent, &["variable_list"]))
                .and_then(|variables| variables.named_child(0).or_else(|| variables.child(0)));
        }
    }
    if language == "fortran" && matches!(kind, "subroutine" | "function") {
        return find_descendant_kind(node, &["subroutine_statement", "function_statement"])
            .and_then(|statement| statement.child_by_field_name("name"));
    }
    if language == "typst" && kind == "let" {
        return node.child_by_field_name("pattern").and_then(|pattern| {
            (pattern.kind() == "call")
                .then(|| pattern.child_by_field_name("item"))
                .flatten()
        });
    }
    if language == "wolfram"
        && matches!(kind, "set_delayed_top" | "set_top" | "set_delayed" | "set")
    {
        return node.named_child(0).and_then(|left| {
            if left.kind() == "apply" {
                find_descendant_kind(left, &["user_symbol", "builtin_symbol"])
            } else if matches!(left.kind(), "user_symbol" | "builtin_symbol") {
                Some(left)
            } else {
                None
            }
        });
    }
    if language == "markdown" && matches!(kind, "atx_heading" | "setext_heading") {
        return Some(node);
    }

    let fields: &[&str] = match label {
        "Import" => &[
            "source",
            "path",
            "module_name",
            "module",
            "name",
            "argument",
        ],
        "Variable" | "Field" => &[
            "left",
            "name",
            "pattern",
            "declarator",
            "target",
            "property",
        ],
        "Module" => &["name", "module", "module_name"],
        _ => &["name", "declarator", "identifier", "type", "target"],
    };
    fields
        .iter()
        .find_map(|field| node.child_by_field_name(field))
        .and_then(|candidate| first_name_like(candidate).or(Some(candidate)))
        .or_else(|| first_name_like(node))
}

fn classify_known(
    language: &str,
    node: Node<'_>,
    scope: &Scope,
    source: &[u8],
) -> Option<Definition> {
    let kind = node.kind();
    let (label, field) = match language {
        "rust" => match kind {
            "struct_item" => ("Struct", "name"),
            "enum_item" => ("Enum", "name"),
            "trait_item" => ("Trait", "name"),
            "type_item" => ("TypeAlias", "name"),
            "function_item" if scope.kind == ScopeKind::Type => ("Method", "name"),
            "function_item" => ("Function", "name"),
            "field_declaration" => ("Field", "name"),
            "let_declaration" => ("Variable", "pattern"),
            "use_declaration" => ("Import", "argument"),
            _ => return None,
        },
        "python" => match kind {
            "class_definition" => ("Class", "name"),
            "function_definition" if scope.kind == ScopeKind::Type => ("Method", "name"),
            "function_definition" => ("Function", "name"),
            "assignment" if scope.kind == ScopeKind::Type => ("Field", "left"),
            "assignment" => ("Variable", "left"),
            "import_statement" => ("Import", "name"),
            "import_from_statement" => ("Import", "module_name"),
            _ => return None,
        },
        "javascript" | "typescript" | "tsx" => match kind {
            "class_declaration" => ("Class", "name"),
            "interface_declaration" => ("Interface", "name"),
            "type_alias_declaration" => ("TypeAlias", "name"),
            "function_declaration" => ("Function", "name"),
            "method_definition" => ("Method", "name"),
            "field_definition" => ("Field", "property"),
            "public_field_definition" => ("Field", "name"),
            "variable_declarator" => ("Variable", "name"),
            "import_statement" => ("Import", "source"),
            _ => return None,
        },
        "go" => match kind {
            "type_spec" => {
                let label = match node.child_by_field_name("type").map(|child| child.kind()) {
                    Some("struct_type") => "Struct",
                    Some("interface_type") => "Interface",
                    _ => "Type",
                };
                (label, "name")
            }
            "function_declaration" => ("Function", "name"),
            "method_declaration" => ("Method", "name"),
            "field_declaration" => ("Field", "name"),
            "short_var_declaration" => ("Variable", "left"),
            "var_spec" => ("Variable", "name"),
            "import_spec" => ("Import", "path"),
            _ => return None,
        },
        _ => return None,
    };
    let name_node = node
        .child_by_field_name(field)
        .or_else(|| first_identifier(node));
    let raw_name = name_node.map_or_else(
        || node_text(node, source),
        |value| {
            if matches!(
                kind,
                "short_var_declaration" | "assignment" | "let_declaration"
            ) {
                first_identifier(value).map_or_else(
                    || node_text(value, source),
                    |identifier| node_text(identifier, source),
                )
            } else {
                node_text(value, source)
            }
        },
    );
    let name = raw_name
        .trim_matches(|character: char| character == '"' || character == '\'')
        .trim();
    if name.is_empty() {
        None
    } else {
        Some(Definition {
            label,
            name: name.to_owned(),
        })
    }
}

fn classify_generic(node: Node<'_>, scope: &Scope, source: &[u8]) -> Option<Definition> {
    let kind = node.kind();
    let label = generic_definition_label(kind, scope)?;
    let name_node = generic_name_node(node, label)?;
    let raw_name = if matches!(label, "Variable" | "Field") {
        first_identifier(name_node).map_or_else(
            || node_text(name_node, source),
            |value| node_text(value, source),
        )
    } else {
        node_text(name_node, source)
    };
    let name = raw_name
        .trim_matches(|character: char| {
            matches!(character, '"' | '\'' | '`' | '<' | '>' | ':' | ';')
        })
        .trim();
    (!name.is_empty()).then(|| Definition {
        label,
        name: name.to_owned(),
    })
}

fn generic_definition_label(kind: &str, scope: &Scope) -> Option<&'static str> {
    if is_generic_method(kind) {
        return Some("Method");
    }
    if is_generic_function(kind) {
        return Some(if scope.kind == ScopeKind::Type {
            "Method"
        } else {
            "Function"
        });
    }
    generic_type_label(kind)
        .or_else(|| is_generic_field(kind).then_some("Field"))
        .or_else(|| is_generic_variable(kind).then_some("Variable"))
        .or_else(|| is_generic_import(kind).then_some("Import"))
}

fn is_generic_method(kind: &str) -> bool {
    matches!(
        kind,
        "method"
            | "method_declaration"
            | "method_definition"
            | "method_signature"
            | "constructor"
            | "constructor_declaration"
            | "constructor_definition"
            | "destructor"
            | "destructor_declaration"
            | "secondary_constructor"
    )
}

fn is_generic_function(kind: &str) -> bool {
    matches!(
        kind,
        "function"
            | "function_declaration"
            | "function_definition"
            | "function_item"
            | "function_signature"
            | "function_signature_item"
            | "function_statement"
            | "function_clause"
            | "function_def"
            | "func_def"
            | "method_elem"
            | "subroutine"
            | "subroutine_declaration"
            | "subroutine_declaration_statement"
            | "procedure"
            | "procedure_declaration"
            | "procedure_definition"
            | "procedure_definition_item"
            | "macro_definition"
            | "macro_declaration"
            | "macro_def"
            | "rpc"
    )
}

fn generic_type_label(kind: &str) -> Option<&'static str> {
    match kind {
        "class"
        | "class_declaration"
        | "class_definition"
        | "class_statement"
        | "class_specifier"
        | "class_interface"
        | "class_implementation" => Some("Class"),
        "struct"
        | "struct_item"
        | "struct_declaration"
        | "struct_definition"
        | "struct_specifier"
        | "structure"
        | "structure_declaration" => Some("Struct"),
        "enum" | "enum_item" | "enum_declaration" | "enum_definition" | "enum_specifier"
        | "enum_statement" => Some("Enum"),
        "trait" | "trait_item" | "trait_declaration" => Some("Trait"),
        "interface"
        | "interface_declaration"
        | "interface_definition"
        | "protocol_declaration"
        | "protocol_definition" => Some("Interface"),
        "type_alias" | "type_alias_declaration" | "type_alias_definition" | "type_item" => {
            Some("TypeAlias")
        }
        "type"
        | "type_declaration"
        | "type_definition"
        | "type_spec"
        | "data_type"
        | "newtype"
        | "custom_type"
        | "contract_declaration"
        | "message"
        | "service" => Some("Type"),
        _ => None,
    }
}

fn is_generic_field(kind: &str) -> bool {
    matches!(
        kind,
        "field"
            | "field_declaration"
            | "field_definition"
            | "public_field_definition"
            | "property"
            | "property_declaration"
            | "property_definition"
            | "record_field"
            | "enum_member"
    )
}

fn is_generic_variable(kind: &str) -> bool {
    matches!(
        kind,
        "let_declaration"
            | "variable_declaration"
            | "variable_declarator"
            | "variable_assignment"
            | "short_var_declaration"
            | "var_declaration"
            | "var_spec"
            | "assignment"
            | "assignment_statement"
            | "const_declaration"
    )
}

fn is_generic_import(kind: &str) -> bool {
    matches!(
        kind,
        "import"
            | "import_declaration"
            | "import_directive"
            | "import_or_export"
            | "import_spec"
            | "import_statement"
            | "import_from_statement"
            | "include"
            | "include_directive"
            | "include_statement"
            | "preproc_include"
            | "preproc_import"
            | "require_statement"
            | "use_declaration"
            | "use_statement"
            | "using_directive"
            | "using_statement"
    )
}

fn generic_name_node<'tree>(node: Node<'tree>, label: &str) -> Option<Node<'tree>> {
    let fields: &[&str] = if label == "Import" {
        &["path", "module_name", "source", "argument", "name"]
    } else if matches!(label, "Variable" | "Field") {
        &[
            "name",
            "declarator",
            "pattern",
            "left",
            "property",
            "field",
            "key",
        ]
    } else {
        &["name", "declarator", "identifier", "type"]
    };
    fields
        .iter()
        .find_map(|field| node.child_by_field_name(field))
        .or_else(|| first_identifier(node))
}

fn is_call(mode: IndexMode, language: &str, kind: &str) -> bool {
    let known = match language {
        "python" => kind == "call",
        "rust" | "javascript" | "typescript" | "tsx" | "go" => kind == "call_expression",
        _ => false,
    };
    if known || mode == IndexMode::Fast {
        return known;
    }
    language_spec(language).map_or_else(
        || {
            matches!(
                kind,
                "call"
                    | "call_expression"
                    | "function_call"
                    | "function_call_expression"
                    | "method_invocation"
                    | "invocation_expression"
                    | "new_expression"
                    | "object_creation_expression"
                    | "application_expression"
                    | "apply_expression"
                    | "command_call"
                    | "subroutine_call"
                    | "system_tf_call"
            )
        },
        |spec| spec.call_kinds.contains(&kind),
    )
}

fn audited_call_target<'tree>(language: &str, node: Node<'tree>) -> Option<Node<'tree>> {
    if language == "elixir" && node.kind() == "call" {
        return node.named_child(0);
    }
    if language == "cobol" && node.kind() == "call_statement" {
        return node
            .child_by_field_name("x")
            .or_else(|| node.named_child(0));
    }
    if language == "erlang" && node.kind() == "call" {
        return node
            .child_by_field_name("expr")
            .or_else(|| node.named_child(0));
    }
    if language == "dart" && node.kind() == "selector" {
        return node.prev_named_sibling();
    }
    if language == "nasm" && node.kind() == "actual_instruction" {
        return node
            .child_by_field_name("operands")
            .or_else(|| find_descendant_kind(node, &["operands"]))
            .and_then(|operands| find_descendant_kind(operands, &["word"]));
    }
    if language == "vhdl" && node.kind() == "parenthesis_group" {
        return node.prev_named_sibling();
    }
    [
        "function",
        "callee",
        "name",
        "method",
        "command",
        "command_name",
        "target",
    ]
    .into_iter()
    .find_map(|field| node.child_by_field_name(field))
    .or_else(|| first_name_like(node))
}

fn generic_call_target(node: Node<'_>) -> Option<Node<'_>> {
    ["callee", "name", "method", "command", "target"]
        .into_iter()
        .find_map(|field| node.child_by_field_name(field))
        .or_else(|| first_identifier(node))
}

fn receiver_type(node: Node<'_>, source: &[u8]) -> Option<String> {
    let receiver = node.child_by_field_name("receiver")?;
    let type_node = find_descendant_kind(receiver, &["type_identifier", "identifier"])?;
    Some(node_text(type_node, source))
}

fn call_short_name(callee: Node<'_>, source: &[u8]) -> String {
    for field in ["field", "property", "attribute"] {
        if let Some(value) = callee.child_by_field_name(field) {
            return node_text(value, source);
        }
    }
    if matches!(
        callee.kind(),
        "identifier" | "field_identifier" | "property_identifier"
    ) {
        return node_text(callee, source);
    }
    last_identifier(&node_text(callee, source))
}

fn first_identifier(node: Node<'_>) -> Option<Node<'_>> {
    find_descendant_kind(
        node,
        &[
            "identifier",
            "field_identifier",
            "property_identifier",
            "type_identifier",
        ],
    )
}

fn first_name_like(node: Node<'_>) -> Option<Node<'_>> {
    find_descendant_kind(
        node,
        &[
            "identifier",
            "ident",
            "id",
            "field_identifier",
            "property_identifier",
            "type_identifier",
            "namespace_identifier",
            "simple_identifier",
            "simple_name",
            "constant",
            "constant_identifier",
            "variable",
            "variable_name",
            "name",
            "alias",
            "symbol",
            "sym_lit",
            "function_name",
            "rpc_name",
            "method_identifier",
            "enum_identifier",
            "program_name",
            "lower_case_identifier",
            "upper_case_identifier",
            "long_identifier",
            "operator_identifier",
            "user_symbol",
            "builtin_symbol",
            "word",
            "key",
            "bare_key",
            "dotted_key",
            "section_name",
            "tag_name",
            "Name",
            "module_path",
            "qid",
            "atom",
            "command_name",
            "service_name",
            "message_name",
            "enum_name",
            "data_name",
            "record_name",
            "class_identifier",
            "string_content",
        ],
    )
}

fn ancestor_kind<'tree>(node: Node<'tree>, kind: &str, max_depth: usize) -> Option<Node<'tree>> {
    let mut current = node.parent();
    for _ in 0..max_depth {
        let ancestor = current?;
        if ancestor.kind() == kind {
            return Some(ancestor);
        }
        current = ancestor.parent();
    }
    None
}

fn find_descendant_kind<'tree>(node: Node<'tree>, kinds: &[&str]) -> Option<Node<'tree>> {
    if kinds.contains(&node.kind()) {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find_map(|child| find_descendant_kind(child, kinds))
}

fn node_text(node: Node<'_>, source: &[u8]) -> String {
    source
        .get(node.byte_range())
        .map_or_else(String::new, |value| {
            String::from_utf8_lossy(value).into_owned()
        })
}

fn embedded_es_imports(language: &str, source: &[u8]) -> Vec<String> {
    if !matches!(language, "astro" | "svelte" | "vue") {
        return Vec::new();
    }
    let source = String::from_utf8_lossy(source);
    let mut imports = source
        .lines()
        .filter_map(|line| {
            let import = line
                .find("import ")
                .map(|start| &line[start..])
                .filter(|_| {
                    line.trim_start().starts_with("import ")
                        || line
                            .split_once("import ")
                            .is_some_and(|(prefix, _)| prefix.trim_end().ends_with('>'))
                })?;
            first_quoted_value(import)
        })
        .collect::<Vec<_>>();
    imports.sort();
    imports.dedup();
    imports
}

fn gomod_requirement_name(text: &str) -> Option<String> {
    import_name_after_keyword(text, "require")
}

fn audited_relations(language: &str, node: Node<'_>, source: &[u8]) -> Vec<(&'static str, String)> {
    if language == "graphql" && node.kind() == "type_definition" {
        return Vec::new();
    }
    let text = node_text(node, source);
    if language == "smali" {
        return text
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if let Some(target) = line.strip_prefix(".super ") {
                    return relation_target(target).map(|target| ("INHERITS", target));
                }
                line.strip_prefix(".implements ")
                    .and_then(relation_target)
                    .map(|target| ("IMPLEMENTS", target))
            })
            .collect();
    }
    if language == "objc"
        && let Some(header) = text.lines().next()
        && let Some((_, base)) = header.split_once(':')
        && let Some(target) = relation_target(base)
    {
        return vec![("INHERITS", target)];
    }

    let header = text.split('{').next().unwrap_or(&text);
    let mut relations = Vec::new();
    relations.extend(
        relation_names_after_keyword(header, "extends")
            .into_iter()
            .map(|target| ("INHERITS", target)),
    );
    relations.extend(
        relation_names_after_keyword(header, "implements")
            .into_iter()
            .map(|target| ("IMPLEMENTS", target)),
    );
    relations
}

fn relation_names_after_keyword(text: &str, keyword: &str) -> Vec<String> {
    let Some(start) = find_word(text, keyword) else {
        return Vec::new();
    };
    let mut rest = text[start + keyword.len()..].trim_start();
    for terminator in [" extends ", " implements ", " where ", "{"] {
        if let Some(end) = rest.find(terminator) {
            rest = &rest[..end];
        }
    }
    rest.split([',', '&']).filter_map(relation_target).collect()
}

fn find_word(text: &str, word: &str) -> Option<usize> {
    text.match_indices(word).find_map(|(index, _)| {
        let before = text[..index].chars().next_back();
        let after = text[index + word.len()..].chars().next();
        let boundary = |character: Option<char>| {
            character.is_none_or(|character| !character.is_alphanumeric() && character != '_')
        };
        (boundary(before) && boundary(after)).then_some(index)
    })
}

fn relation_target(text: &str) -> Option<String> {
    let target = text
        .trim()
        .trim_end_matches(';')
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches(|character: char| {
            matches!(character, ':' | ',' | '(' | ')' | '<' | '>' | '"' | '\'')
        });
    (!target.is_empty()).then(|| target.to_owned())
}

fn last_identifier(value: &str) -> String {
    value
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .rfind(|segment| !segment.is_empty())
        .unwrap_or_default()
        .to_owned()
}

fn path_stem(path: &ProjectRelativePath) -> String {
    let mut segments = path.as_str().split('/').collect::<Vec<_>>();
    if let Some(last) = segments.last_mut()
        && let Some((stem, _)) = last.rsplit_once('.')
    {
        *last = stem;
    }
    segments
        .into_iter()
        .map(qualified_segment)
        .collect::<Vec<_>>()
        .join(".")
}

fn module_name(path: &ProjectRelativePath, language: &LanguageId) -> String {
    if language.as_str() != "go" {
        return path_stem(path);
    }
    path.as_str()
        .rsplit_once('/')
        .map_or_else(String::new, |(directory, _)| {
            directory
                .split('/')
                .map(qualified_segment)
                .collect::<Vec<_>>()
                .join(".")
        })
}

fn qualified_segment(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut separator = false;
    for character in value.chars() {
        if character.is_alphanumeric() || character == '_' {
            if separator && !result.is_empty() {
                result.push('_');
            }
            separator = false;
            result.push(character);
        } else {
            separator = true;
        }
    }
    if result.is_empty() {
        "anonymous".to_owned()
    } else {
        result
    }
}

fn stable_node_id(label: &str, qualified_name: &str) -> Result<NodeId, IndexError> {
    let hash = blake3::hash(format!("goldeneye-node-v1\0{label}\0{qualified_name}").as_bytes());
    Ok(NodeId::new(format!(
        "{}:{}",
        label.to_ascii_lowercase(),
        &hash.to_hex()[..32]
    ))?)
}

#[allow(clippy::too_many_arguments)]
fn graph_node(
    project: &ProjectId,
    path: &ProjectRelativePath,
    language: &LanguageId,
    label: &str,
    name: &str,
    qualified_name: &str,
    syntax_kind: &str,
    span: SourceSpan,
) -> Result<GraphNode, IndexError> {
    let mut properties = GraphProperties::new();
    properties.insert("language".into(), json!(language.as_str()));
    properties.insert("syntax_kind".into(), json!(syntax_kind));
    properties.insert("file_path".into(), json!(path.as_str()));
    Ok(GraphNode::new(
        project.clone(),
        stable_node_id(label, qualified_name)?,
        NodeLabel::new(label)?,
        name,
        QualifiedName::new(qualified_name)?,
        Some(path.clone()),
        Some(span),
        Generation::new(0),
    )?
    .with_properties(properties))
}

pub(crate) fn project_node(
    project: &goldeneye_domain::ProjectRecord,
) -> Result<GraphNode, IndexError> {
    let qualified_name = project.id.as_str();
    let mut properties = GraphProperties::new();
    properties.insert("root_path".into(), json!(project.root_path));
    Ok(GraphNode::new(
        project.id.clone(),
        stable_node_id("Project", qualified_name)?,
        NodeLabel::new("Project")?,
        qualified_name,
        QualifiedName::new(qualified_name)?,
        None,
        None,
        Generation::new(0),
    )?
    .with_properties(properties))
}

pub(crate) fn branch_node(
    project: &goldeneye_domain::ProjectRecord,
) -> Result<GraphNode, IndexError> {
    let qualified_name = format!("{}.__branch__.working-tree", project.id.as_str());
    let mut properties = GraphProperties::new();
    properties.insert("branch".into(), json!("working-tree"));
    Ok(GraphNode::new(
        project.id.clone(),
        stable_node_id("Branch", &qualified_name)?,
        NodeLabel::new("Branch")?,
        "working-tree",
        QualifiedName::new(qualified_name)?,
        None,
        None,
        Generation::new(0),
    )?
    .with_properties(properties))
}

pub(crate) fn project_has_branch(
    project: &ProjectId,
    branch: &GraphNode,
) -> Result<GraphEdge, IndexError> {
    graph_edge(
        project,
        project_node_id(project)?,
        branch.id.clone(),
        "HAS_BRANCH",
        None,
        GraphProperties::new(),
    )
}

pub(crate) fn project_contains_file(
    project: &ProjectId,
    file_node: &GraphNode,
) -> Result<GraphEdge, IndexError> {
    let branch_qualified_name = format!("{}.__branch__.working-tree", project.as_str());
    graph_edge(
        project,
        stable_node_id("Branch", &branch_qualified_name)?,
        file_node.id.clone(),
        "CONTAINS_FILE",
        None,
        GraphProperties::new(),
    )
}

pub(crate) fn project_node_id(project: &ProjectId) -> Result<NodeId, IndexError> {
    stable_node_id("Project", project.as_str())
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

fn source_span(node: Node<'_>) -> Result<SourceSpan, IndexError> {
    let range = node.range();
    let start_byte = u64::try_from(range.start_byte)
        .map_err(|_| IndexError::CoordinateOverflow("start byte"))?;
    let end_byte =
        u64::try_from(range.end_byte).map_err(|_| IndexError::CoordinateOverflow("end byte"))?;
    let start_row =
        u64::try_from(range.start_point.row).map_err(|_| IndexError::CoordinateOverflow("row"))?;
    let start_column = u64::try_from(range.start_point.column)
        .map_err(|_| IndexError::CoordinateOverflow("column"))?;
    let end_row =
        u64::try_from(range.end_point.row).map_err(|_| IndexError::CoordinateOverflow("row"))?;
    let end_column = u64::try_from(range.end_point.column)
        .map_err(|_| IndexError::CoordinateOverflow("column"))?;
    Ok(SourceSpan::new(
        ByteSpan::new(start_byte, end_byte)?,
        SourcePoint::new(start_row, start_column),
        SourcePoint::new(end_row, end_column),
    )?)
}

#[cfg(all(test, feature = "full-grammar-tests"))]
mod full_language_tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    use goldeneye_discovery::IndexMode;
    use goldeneye_domain::{
        ContentHash, FileId, FileRecord, Generation, LanguageId, ProjectId, ProjectRelativePath,
    };
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
