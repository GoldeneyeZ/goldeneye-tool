use super::{
    Definition, ExtractedImport, ExtractedRelation, Extractor, GraphProperties, IndexError,
    MAX_PENDING_IMPORTS_PER_FILE, MAX_PENDING_RELATIONS_PER_FILE, MAX_TYPE_BINDINGS_PER_SCOPE,
    Node, NodeId, Scope, ScopeKind, audited_relations, binding_key, graph_edge, graph_node,
    import_alias, import_bindings, infer_declared_type, normalize_import_path, qualified_segment,
    source_span, stable_node_id,
};

impl Extractor<'_> {
    pub(super) fn add_definition(
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
        let qualified_name = self.next_definition_qualified_name(node, &definition, scope);
        let id = self.emit_definition_node(node, &definition, scope, &qualified_name)?;
        if matches!(definition.label, "Function" | "Method") {
            return Ok(Some(self.callable_scope(definition, id, qualified_name)));
        }
        if is_type_definition(definition.label) {
            return Ok(Some(self.record_type_scope(
                node,
                definition,
                scope,
                id,
                qualified_name,
            )));
        }
        Ok(None)
    }

    fn next_definition_qualified_name(
        &mut self,
        node: Node<'_>,
        definition: &Definition,
        scope: &Scope,
    ) -> String {
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
        if *count == 1 {
            base
        } else {
            format!("{base}#{count}")
        }
    }

    fn emit_definition_node(
        &mut self,
        node: Node<'_>,
        definition: &Definition,
        scope: &Scope,
        qualified_name: &str,
    ) -> Result<NodeId, IndexError> {
        let id = stable_node_id(definition.label, qualified_name)?;
        let span = source_span(node)?;
        let graph_node = graph_node(
            self.project,
            self.path,
            self.language,
            definition.label,
            &definition.name,
            qualified_name,
            node.kind(),
            span,
        )?;
        let relation = definition_relation(definition.label, scope.kind);
        self.edges.push(graph_edge(
            self.project,
            scope.parent.clone(),
            id.clone(),
            relation,
            None,
            GraphProperties::new(),
        )?);
        self.nodes.push(graph_node);
        Ok(id)
    }

    fn callable_scope(
        &mut self,
        definition: Definition,
        id: NodeId,
        qualified_name: String,
    ) -> Scope {
        self.callable_definitions
            .entry(definition.name)
            .or_default()
            .push(id.clone());
        Scope {
            parent: id.clone(),
            qualified_name,
            kind: ScopeKind::Callable,
            callable: Some(id),
        }
    }

    fn record_type_scope(
        &mut self,
        node: Node<'_>,
        definition: Definition,
        scope: &Scope,
        id: NodeId,
        qualified_name: String,
    ) -> Scope {
        for (kind, target_name) in audited_relations(self.language.as_str(), node, self.source) {
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
        type_scope
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
}

fn definition_relation(label: &str, scope_kind: ScopeKind) -> &'static str {
    if matches!(label, "Field" | "Variable") && scope_kind != ScopeKind::Module {
        "CONTAINS"
    } else if label == "Import" {
        "IMPORTS"
    } else {
        "DEFINES"
    }
}

fn is_type_definition(label: &str) -> bool {
    matches!(
        label,
        "Class" | "Struct" | "Enum" | "Trait" | "Interface" | "Type" | "TypeAlias"
    )
}
