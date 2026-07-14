use std::fs;
use std::num::NonZeroUsize;
use std::path::Path;

use goldeneye_discovery::{DiscoveryOptions, IndexMode};
use goldeneye_domain::{FileId, LanguageId, ProjectRelativePath};
use goldeneye_index::{CancellationToken, IndexOptions, IndexService};
use goldeneye_store::Store;
use goldeneye_syntax::{CoreGrammarProvider, Grammar, GrammarProvider, SyntaxError};
use tempfile::TempDir;

#[derive(Debug, Clone, Copy)]
struct NonCoreFixtureProvider;

impl GrammarProvider for NonCoreFixtureProvider {
    fn grammar(&self, language_id: &LanguageId) -> Result<Grammar, SyntaxError> {
        if language_id.as_str() != "java" {
            return Err(SyntaxError::UnsupportedGrammar {
                language_id: language_id.clone(),
            });
        }
        let rust = LanguageId::new("rust").expect("fixture language ID");
        let mut grammar = CoreGrammarProvider.grammar(&rust)?;
        grammar.language_id = language_id.clone();
        Ok(grammar)
    }

    fn supported_ids(&self) -> Vec<LanguageId> {
        vec![LanguageId::new("java").expect("fixture language ID")]
    }
}

fn write_fixture(root: &Path) {
    fs::write(
        root.join("Fixture.java"),
        "struct Widget { value: i32 }\nfn helper() {}\nfn run() { helper(); }\n",
    )
    .expect("write non-core grammar fixture");
}

fn index_mode(root: &Path, mode: IndexMode) -> (Vec<String>, bool) {
    let options = IndexOptions {
        discovery: DiscoveryOptions {
            mode,
            ..DiscoveryOptions::default()
        },
        max_workers: NonZeroUsize::new(1).expect("one worker"),
        max_files: None,
        cancellation: CancellationToken::new(),
    };
    let mut service = IndexService::new(
        Store::open_in_memory().expect("memory store"),
        NonCoreFixtureProvider,
        options,
    );
    let result = service.index_repository(root).expect("index fixture");
    assert_eq!(result.parsed_files, 1);
    let file = FileId::new(
        result.project.id.clone(),
        ProjectRelativePath::new("Fixture.java").expect("fixture path"),
    );
    let nodes = service
        .store()
        .nodes_for_file(&file)
        .expect("fixture nodes");
    let has_calls = nodes.iter().any(|node| {
        service
            .store()
            .edges_from(&result.project.id, &node.id)
            .expect("fixture edges")
            .iter()
            .any(|edge| edge.kind.as_str() == "CALLS")
    });
    let mut labels = nodes
        .into_iter()
        .map(|node| node.label.as_str().to_owned())
        .collect::<Vec<_>>();
    labels.sort();
    (labels, has_calls)
}

#[test]
fn audited_languages_do_not_fall_back_to_unrelated_generic_node_kinds() {
    let temp = TempDir::new().expect("temp repository");
    write_fixture(temp.path());

    let (fast_labels, fast_calls) = index_mode(temp.path(), IndexMode::Fast);
    assert_eq!(fast_labels, ["File", "Module"]);
    assert!(!fast_calls);

    for mode in [IndexMode::Moderate, IndexMode::Full] {
        let (labels, has_calls) = index_mode(temp.path(), mode);
        assert_eq!(labels, ["Field", "File", "Module"]);
        assert!(!labels.iter().any(|label| label == "Function"));
        assert!(!labels.iter().any(|label| label == "Struct"));
        assert!(!has_calls);
    }
}
