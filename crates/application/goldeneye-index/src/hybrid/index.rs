use std::collections::{BTreeMap, BTreeSet};

use goldeneye_domain::{GraphNode, LanguageId, NodeId, ProjectRelativePath};
use goldeneye_ports::{
    IndexExtractedCall as ExtractedCall, IndexExtractedImport as ExtractedImport,
    IndexExtractedRelation as ExtractedRelation,
};
use serde_json::Value;

use super::names::{
    binding_key, call_receiver, class_method_tail, expand_alias, import_reaches, is_builtin,
    is_callable_label, is_definition_label, is_lsp_wired, is_type_label, language_compatible,
    normalize_name, normalized_suffix, owner_type, parent_qn, tail_eq,
};

#[derive(Debug, Clone)]
pub(super) struct DefinitionRef {
    pub(super) id: NodeId,
    pub(super) name: String,
    pub(super) qualified_name: String,
    pub(super) label: String,
    pub(super) language: LanguageId,
    pub(super) module_qn: String,
    pub(super) owner_type: Option<String>,
}

#[derive(Debug)]
pub(super) struct DefinitionIndex {
    definitions: Vec<DefinitionRef>,
    by_name: BTreeMap<String, Vec<usize>>,
    by_qn: BTreeMap<String, usize>,
    file_modules: BTreeMap<ProjectRelativePath, String>,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ResolutionStrategy {
    ReceiverType,
    ImportMap,
    SameContainer,
    QualifiedSuffix,
    UniqueName,
    JvmTail,
}

impl ResolutionStrategy {
    pub(super) const fn as_str(self) -> &'static str {
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

impl DefinitionIndex {
    pub(super) fn build(nodes: &[GraphNode]) -> Self {
        let file_modules = Self::collect_file_modules(nodes);
        let mut definitions = Self::collect_definitions(nodes, &file_modules);
        definitions.sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
        let (by_name, by_qn) = Self::definition_lookups(&definitions);
        Self {
            definitions,
            by_name,
            by_qn,
            file_modules,
        }
    }

    fn collect_file_modules(nodes: &[GraphNode]) -> BTreeMap<ProjectRelativePath, String> {
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
        file_modules
    }

    fn collect_definitions(
        nodes: &[GraphNode],
        file_modules: &BTreeMap<ProjectRelativePath, String>,
    ) -> Vec<DefinitionRef> {
        let mut definitions = Vec::new();
        for node in nodes {
            let Some(definition) = Self::definition(node, file_modules) else {
                continue;
            };
            definitions.push(definition);
        }
        definitions
    }

    fn definition(
        node: &GraphNode,
        file_modules: &BTreeMap<ProjectRelativePath, String>,
    ) -> Option<DefinitionRef> {
        if !is_definition_label(node.label.as_str()) {
            return None;
        }
        let file = node.file_path.clone()?;
        let language = node
            .properties
            .get("language")
            .and_then(Value::as_str)
            .and_then(|value| LanguageId::new(value).ok())?;
        let qualified_name = node.qualified_name.as_str().to_owned();
        let owner_type =
            (node.label.as_str() == "Method").then(|| owner_type(&qualified_name, &node.name));
        let module_qn = file_modules
            .get(&file)
            .cloned()
            .unwrap_or_else(|| parent_qn(&qualified_name));
        Some(DefinitionRef {
            id: node.id.clone(),
            name: node.name.clone(),
            qualified_name,
            label: node.label.as_str().to_owned(),
            language,
            module_qn,
            owner_type,
        })
    }

    fn definition_lookups(
        definitions: &[DefinitionRef],
    ) -> (BTreeMap<String, Vec<usize>>, BTreeMap<String, usize>) {
        let mut by_name = BTreeMap::<String, Vec<usize>>::new();
        let mut by_qn = BTreeMap::new();
        for (index, definition) in definitions.iter().enumerate() {
            by_name
                .entry(binding_key(&definition.name))
                .or_default()
                .push(index);
            by_qn.insert(normalize_name(&definition.qualified_name), index);
        }
        (by_name, by_qn)
    }

    pub(super) fn resolve_call<'a>(
        &'a self,
        call: &ExtractedCall,
        imports: &[ExtractedImport],
    ) -> Option<(&'a DefinitionRef, ResolutionStrategy)> {
        if !is_lsp_wired(call.language.as_str()) {
            return None;
        }
        let short_name = binding_key(&call.short_name);
        let callee = normalize_name(&call.callee_name);
        let candidates = self.call_candidates(call, &short_name, &callee, imports);
        if candidates.is_empty() {
            return None;
        }
        if let Some(target) = Self::resolve_receiver_type(&candidates, call, imports) {
            return Some((target, ResolutionStrategy::ReceiverType));
        }
        if let Some(target) = Self::resolve_via_imports(&candidates, &callee, &short_name, imports)
        {
            return Some((target, ResolutionStrategy::ImportMap));
        }
        if let Some(target) = self.resolve_same_container(&candidates, call) {
            return Some((target, ResolutionStrategy::SameContainer));
        }
        if let Some(target) = Self::resolve_qualified_suffix(&candidates, &callee, &short_name) {
            return Some((target, ResolutionStrategy::QualifiedSuffix));
        }
        if let Some(target) = Self::resolve_jvm_tail(&candidates, call, &callee) {
            return Some((target, ResolutionStrategy::JvmTail));
        }
        Self::resolve_unique_name(&candidates, call, &short_name)
            .map(|target| (target, ResolutionStrategy::UniqueName))
    }

    fn call_candidates<'a>(
        &'a self,
        call: &ExtractedCall,
        short_name: &str,
        callee: &str,
        imports: &[ExtractedImport],
    ) -> Vec<&'a DefinitionRef> {
        self.candidate_indices(short_name, callee, imports)
            .into_iter()
            .filter_map(|index| self.definitions.get(index))
            .filter(|definition| {
                is_callable_label(&definition.label)
                    && language_compatible(call.language.as_str(), definition.language.as_str())
            })
            .collect()
    }

    fn resolve_receiver_type<'a>(
        candidates: &[&'a DefinitionRef],
        call: &ExtractedCall,
        imports: &[ExtractedImport],
    ) -> Option<&'a DefinitionRef> {
        let receiver_type = call.receiver_type.as_ref()?;
        let expanded = expand_alias(receiver_type, imports);
        unique(candidates.iter().copied().filter(|definition| {
            definition
                .owner_type
                .as_ref()
                .is_some_and(|owner| tail_eq(owner, receiver_type) || tail_eq(owner, &expanded))
        }))
    }

    fn resolve_same_container<'a>(
        &'a self,
        candidates: &[&'a DefinitionRef],
        call: &ExtractedCall,
    ) -> Option<&'a DefinitionRef> {
        let fallback_module = parent_qn(&call.caller_qn);
        let caller_module = self
            .file_modules
            .get(&call.file)
            .map_or(fallback_module.as_str(), String::as_str);
        let caller_owner = owner_type(&call.caller_qn, "");
        unique(candidates.iter().copied().filter(|definition| {
            definition.module_qn == caller_module
                && (definition.label != "Method"
                    || definition
                        .owner_type
                        .as_ref()
                        .is_some_and(|owner| tail_eq(owner, &caller_owner)))
        }))
    }

    fn resolve_qualified_suffix<'a>(
        candidates: &[&'a DefinitionRef],
        callee: &str,
        short_name: &str,
    ) -> Option<&'a DefinitionRef> {
        if !callee.contains('.') {
            return None;
        }
        unique(candidates.iter().copied().filter(|definition| {
            normalized_suffix(&definition.qualified_name, callee)
                || definition.owner_type.as_ref().is_some_and(|owner| {
                    let tail = format!("{}.{}", normalize_name(owner), short_name);
                    callee.ends_with(&tail) || tail.ends_with(callee)
                })
        }))
    }

    fn resolve_jvm_tail<'a>(
        candidates: &[&'a DefinitionRef],
        call: &ExtractedCall,
        callee: &str,
    ) -> Option<&'a DefinitionRef> {
        if !matches!(call.language.as_str(), "java" | "kotlin") {
            return None;
        }
        let call_tail = class_method_tail(callee)?;
        unique(candidates.iter().copied().filter(|definition| {
            class_method_tail(&definition.qualified_name)
                .is_some_and(|target_tail| target_tail == call_tail)
        }))
    }

    fn resolve_unique_name<'a>(
        candidates: &[&'a DefinitionRef],
        call: &ExtractedCall,
        short_name: &str,
    ) -> Option<&'a DefinitionRef> {
        let is_member = call_receiver(&call.callee_name).is_some();
        if is_builtin(call.language.as_str(), short_name)
            || is_member && matches!(call.language.as_str(), "javascript" | "typescript" | "tsx")
        {
            return None;
        }
        unique(candidates.iter().copied())
    }

    pub(super) fn resolve_relation<'a>(
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
