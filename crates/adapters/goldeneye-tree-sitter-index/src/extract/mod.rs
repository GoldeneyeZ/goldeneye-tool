mod calls;
mod classify;
mod definitions;
mod engine;
mod graph;
mod imports;
mod relations;
mod resolution;

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
    ProjectRelativePath, SourceSpan,
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
    let snapshot = parse_snapshot(provider, &candidate)?;
    let diagnostics = syntax_diagnostics(&candidate.record.id.path, &snapshot);
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

fn parse_snapshot<P>(provider: P, candidate: &Candidate) -> Result<SyntaxSnapshot, IndexError>
where
    P: GrammarProvider,
{
    SyntaxEngine::new(provider)
        .parse(
            candidate.language.clone(),
            Arc::clone(&candidate.source),
            Generation::new(0),
        )
        .map_err(|source| IndexError::Syntax {
            path: candidate.record.id.path.clone(),
            source,
        })
}

fn syntax_diagnostics(
    path: &ProjectRelativePath,
    snapshot: &SyntaxSnapshot,
) -> Option<FileSyntaxDiagnostics> {
    snapshot.has_errors().then(|| FileSyntaxDiagnostics {
        path: path.clone(),
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
