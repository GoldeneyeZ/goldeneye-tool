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
    Ok(ExtractedFile {
        record: candidate.record,
        nodes,
        edges,
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

struct PendingCall {
    source: NodeId,
    short_name: String,
    start_byte: u64,
    line: u64,
    text: String,
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
    pending_calls: Vec<PendingCall>,
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
        let mut cursor = root.walk();
        for child in root.named_children(&mut cursor) {
            self.walk(child, &scope)?;
        }
        self.resolve_calls()
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
        let Some(source) = scope.callable.clone() else {
            return Ok(());
        };
        let callee = node.child_by_field_name("function").or_else(|| {
            (self.mode != IndexMode::Fast)
                .then(|| generic_call_target(node))
                .flatten()
        });
        let Some(callee) = callee else {
            return Ok(());
        };
        let text = self.node_text(callee);
        let short_name = call_short_name(callee, self.source);
        if short_name.is_empty() {
            return Ok(());
        }
        self.pending_calls.push(PendingCall {
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

    fn resolve_calls(&mut self) -> Result<(), IndexError> {
        self.pending_calls.sort_by(|left, right| {
            (&left.source, left.start_byte, &left.short_name).cmp(&(
                &right.source,
                right.start_byte,
                &right.short_name,
            ))
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
    classify_known(language, node, scope, source).or_else(|| {
        (mode != IndexMode::Fast)
            .then(|| classify_generic(node, scope, source))
            .flatten()
    })
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
    known
        || (mode != IndexMode::Fast
            && matches!(
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
            ))
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
