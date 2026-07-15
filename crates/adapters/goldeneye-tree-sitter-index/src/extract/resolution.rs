use super::{
    ExtractedCall, Extractor, GraphProperties, IndexError, IndexMode, MAX_PENDING_CALLS_PER_FILE,
    Node, NodeId, Scope, Value, audited_call_target, binding_key, call_receiver, call_short_name,
    generic_call_target, graph_edge, json, language_spec, last_identifier,
    receiver_looks_like_type,
};

impl Extractor<'_> {
    pub(super) fn record_call(&mut self, node: Node<'_>, scope: &Scope) -> Result<(), IndexError> {
        if self.is_ignored_nasm_instruction(node) {
            return Ok(());
        }
        let Some(source) = self.call_source(scope) else {
            return Ok(());
        };
        let Some(callee) = self.call_target(node) else {
            return Ok(());
        };
        let (text, short_name) = self.call_identity(node, callee);
        if short_name.is_empty() {
            return Ok(());
        }
        if self.pending_calls.len() >= MAX_PENDING_CALLS_PER_FILE {
            return Ok(());
        }
        let receiver_type = self.call_receiver_type(&source, &text);
        self.push_pending_call(node, scope, source, text, short_name, receiver_type)
    }

    fn is_ignored_nasm_instruction(&self, node: Node<'_>) -> bool {
        self.language.as_str() == "nasm"
            && node.kind() == "actual_instruction"
            && node
                .child_by_field_name("instruction")
                .map(|instruction| self.node_text(instruction))
                .as_deref()
                != Some("call")
    }

    fn call_source(&self, scope: &Scope) -> Option<NodeId> {
        if let Some(callable) = scope.callable.clone() {
            Some(callable)
        } else if self.mode != IndexMode::Fast {
            Some(scope.parent.clone())
        } else {
            None
        }
    }

    fn call_target<'tree>(&self, node: Node<'tree>) -> Option<Node<'tree>> {
        node.child_by_field_name("function").or_else(|| {
            (self.mode != IndexMode::Fast)
                .then(|| {
                    language_spec(self.language.as_str()).map_or_else(
                        || generic_call_target(node),
                        |_| audited_call_target(self.language.as_str(), node),
                    )
                })
                .flatten()
        })
    }

    fn call_identity(&self, node: Node<'_>, callee: Node<'_>) -> (String, String) {
        if self.language.as_str() == "puppet" && node.kind() == "include_statement" {
            (self.node_text(node), "include".to_owned())
        } else {
            (self.node_text(callee), call_short_name(callee, self.source))
        }
    }

    fn call_receiver_type(&self, source: &NodeId, text: &str) -> Option<String> {
        call_receiver(text).and_then(|receiver| {
            self.type_bindings
                .get(source)
                .and_then(|bindings| bindings.get(&binding_key(receiver)))
                .cloned()
                .or_else(|| receiver_looks_like_type(receiver).then(|| receiver.to_owned()))
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn push_pending_call(
        &mut self,
        node: Node<'_>,
        scope: &Scope,
        source: NodeId,
        text: String,
        short_name: String,
        receiver_type: Option<String>,
    ) -> Result<(), IndexError> {
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

    pub(super) fn resolve_relations(&mut self) -> Result<(), IndexError> {
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

    pub(super) fn resolve_calls(&mut self) -> Result<(), IndexError> {
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
}
