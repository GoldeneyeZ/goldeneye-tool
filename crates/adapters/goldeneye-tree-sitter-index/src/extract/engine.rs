use super::{
    BTreeMap, Definition, Extractor, GraphEdge, GraphNode, GraphProperties, IndexError, IndexMode,
    LanguageId, Node, NodeId, ProjectId, ProjectRelativePath, Scope, ScopeKind, SourceSpan,
    SyntaxSnapshot, classify, embedded_es_imports, gomod_requirement_name, graph_edge, graph_node,
    is_call, language_spec, last_identifier, module_name, node_text, path_stem, project_node_id,
    receiver_type, source_span, stable_node_id,
};

impl<'a> Extractor<'a> {
    pub(super) fn new(
        project: &'a ProjectId,
        path: &'a ProjectRelativePath,
        language: &'a LanguageId,
        snapshot: &'a SyntaxSnapshot,
        mode: IndexMode,
    ) -> Result<Self, IndexError> {
        let source = snapshot.source();
        let initial = initial_graph(project, path, language, snapshot.root())?;
        Ok(Self {
            project,
            path,
            language,
            snapshot,
            mode,
            source,
            nodes: initial.nodes,
            edges: initial.edges,
            qualified_name_counts: BTreeMap::new(),
            callable_definitions: BTreeMap::new(),
            type_scopes: BTreeMap::new(),
            type_bindings: BTreeMap::new(),
            pending_calls: Vec::new(),
            pending_relations: Vec::new(),
            pending_imports: Vec::new(),
            module_scope: Scope {
                parent: initial.module_id,
                qualified_name: initial.module_qualified_name,
                kind: ScopeKind::Module,
                callable: None,
            },
        })
    }

    pub(super) fn run(&mut self) -> Result<(), IndexError> {
        let root = self.snapshot.root();
        let scope = self.module_scope.clone();
        self.seed_embedded_imports(root, &scope)?;
        self.walk_root(root, &scope)?;
        self.resolve_calls()?;
        self.resolve_relations()?;
        self.pending_imports.sort();
        self.pending_imports.dedup();
        Ok(())
    }

    fn seed_embedded_imports(&mut self, root: Node<'_>, scope: &Scope) -> Result<(), IndexError> {
        if self.mode != IndexMode::Fast {
            for name in embedded_es_imports(self.language.as_str(), self.source) {
                self.add_definition(
                    root,
                    Definition {
                        label: "Import",
                        name,
                    },
                    scope,
                )?;
            }
        }
        Ok(())
    }

    fn walk_root(&mut self, root: Node<'_>, scope: &Scope) -> Result<(), IndexError> {
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
            self.walk(root, scope)?;
        } else {
            let mut cursor = root.walk();
            for child in root.named_children(&mut cursor) {
                self.walk(child, scope)?;
            }
        }
        Ok(())
    }

    fn walk(&mut self, node: Node<'_>, scope: &Scope) -> Result<(), IndexError> {
        if let Some(impl_scope) = self.rust_impl_scope(node, scope) {
            return self.walk_children(node, &impl_scope);
        }
        self.record_gomod_requirement(node, scope)?;
        if is_call(self.mode, self.language.as_str(), node.kind()) {
            self.record_call(node, scope)?;
        }
        let effective_scope = self.effective_scope(node, scope);
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

    fn rust_impl_scope(&self, node: Node<'_>, scope: &Scope) -> Option<Scope> {
        (self.language.as_str() == "rust" && node.kind() == "impl_item").then(|| {
            node.child_by_field_name("type")
                .map(|type_node| self.node_text(type_node))
                .and_then(|name| self.unique_type_scope(&last_identifier(&name)))
                .unwrap_or_else(|| scope.clone())
        })
    }

    fn record_gomod_requirement(
        &mut self,
        node: Node<'_>,
        scope: &Scope,
    ) -> Result<(), IndexError> {
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
        Ok(())
    }

    fn effective_scope(&self, node: Node<'_>, scope: &Scope) -> Scope {
        if self.language.as_str() == "go" && node.kind() == "method_declaration" {
            receiver_type(node, self.source)
                .and_then(|name| self.unique_type_scope(&name))
                .unwrap_or_else(|| scope.clone())
        } else {
            scope.clone()
        }
    }

    fn walk_children(&mut self, node: Node<'_>, scope: &Scope) -> Result<(), IndexError> {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.walk(child, scope)?;
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

    pub(super) fn node_text(&self, node: Node<'_>) -> String {
        node_text(node, self.source)
    }
}

struct InitialGraph {
    module_id: NodeId,
    module_qualified_name: String,
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
}

fn initial_graph(
    project: &ProjectId,
    path: &ProjectRelativePath,
    language: &LanguageId,
    root: Node<'_>,
) -> Result<InitialGraph, IndexError> {
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
    nodes.push(initial_file_node(
        project,
        path,
        language,
        &file_qualified_name,
        root_span,
    )?);
    let (module_id, edges) = initial_module_graph(
        project,
        path,
        language,
        root,
        root_span,
        file_id,
        &module_name,
        &module_qualified_name,
        &mut nodes,
    )?;
    Ok(InitialGraph {
        module_id,
        module_qualified_name,
        nodes,
        edges,
    })
}

fn initial_file_node(
    project: &ProjectId,
    path: &ProjectRelativePath,
    language: &LanguageId,
    file_qualified_name: &str,
    root_span: SourceSpan,
) -> Result<GraphNode, IndexError> {
    graph_node(
        project,
        path,
        language,
        "File",
        path.as_str().rsplit('/').next().unwrap_or(path.as_str()),
        file_qualified_name,
        "file",
        root_span,
    )
}

#[allow(clippy::too_many_arguments)]
fn initial_module_graph(
    project: &ProjectId,
    path: &ProjectRelativePath,
    language: &LanguageId,
    root: Node<'_>,
    root_span: SourceSpan,
    file_id: NodeId,
    module_name: &str,
    module_qualified_name: &str,
    nodes: &mut Vec<GraphNode>,
) -> Result<(NodeId, Vec<GraphEdge>), IndexError> {
    if module_name.is_empty() {
        return project_module_graph(project, file_id);
    }
    named_module_graph(
        project,
        path,
        language,
        root,
        root_span,
        file_id,
        module_name,
        module_qualified_name,
        nodes,
    )
}

fn project_module_graph(
    project: &ProjectId,
    file_id: NodeId,
) -> Result<(NodeId, Vec<GraphEdge>), IndexError> {
    let project_id = project_node_id(project)?;
    Ok((
        project_id.clone(),
        vec![graph_edge(
            project,
            file_id,
            project_id,
            "DEFINES",
            None,
            GraphProperties::new(),
        )?],
    ))
}

#[allow(clippy::too_many_arguments)]
fn named_module_graph(
    project: &ProjectId,
    path: &ProjectRelativePath,
    language: &LanguageId,
    root: Node<'_>,
    root_span: SourceSpan,
    file_id: NodeId,
    module_name: &str,
    module_qualified_name: &str,
    nodes: &mut Vec<GraphNode>,
) -> Result<(NodeId, Vec<GraphEdge>), IndexError> {
    let module_id = stable_node_id("Module", module_qualified_name)?;
    nodes.push(graph_node(
        project,
        path,
        language,
        "Module",
        module_name.rsplit('.').next().unwrap_or(module_name),
        module_qualified_name,
        root.kind(),
        root_span,
    )?);
    Ok((
        module_id.clone(),
        vec![graph_edge(
            project,
            file_id,
            module_id,
            "DEFINES",
            None,
            GraphProperties::new(),
        )?],
    ))
}
