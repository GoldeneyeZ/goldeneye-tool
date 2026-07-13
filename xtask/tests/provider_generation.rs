use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use goldeneye_grammar_pack::{GrammarPackLock, LanguageBindingStatus, lock_file_hash};
use tempfile::tempdir;
use xtask::{
    GenerationOutcome, generate_notices, generate_provider, render_notices, render_provider,
};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask is inside the workspace")
        .to_path_buf()
}

fn lock_path() -> PathBuf {
    workspace_root().join("grammars/full-pack.toml")
}

fn language_line<'a>(source: &'a str, language_id: &str) -> &'a str {
    let needle = format!("id: {language_id:?},");
    source
        .lines()
        .find(|line| line.contains(&needle))
        .unwrap_or_else(|| panic!("missing generated language row for {language_id}"))
}

fn grammar_index(line: &str) -> usize {
    line.split_once("grammar_index: ")
        .expect("available row has grammar index")
        .1
        .split(|character: char| !character.is_ascii_digit())
        .next()
        .expect("grammar index has digits")
        .parse()
        .expect("grammar index is numeric")
}

fn without_embedded_lock_hash(source: &str) -> String {
    source
        .lines()
        .filter(|line| {
            !line.starts_with("// goldeneye-full-pack-lock-sha256:")
                && !line.starts_with("<!-- goldeneye-full-pack-lock-sha256:")
                && !line.contains("FULL_PACK_LOCK_SHA256")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn reverse_lock_tables(source: &str) -> String {
    fn reverse(section: &str, marker: &str) -> String {
        let mut blocks = section
            .split(marker)
            .skip(1)
            .map(|tail| format!("{marker}{tail}"))
            .collect::<Vec<_>>();
        blocks.reverse();
        blocks.concat()
    }

    let grammar_start = source.find("[[grammars]]").expect("grammar table");
    let mapping_start = source
        .find("[[language_mappings]]")
        .expect("language mapping table");
    format!(
        "{}{}{}",
        &source[..grammar_start],
        reverse(&source[grammar_start..mapping_start], "[[grammars]]"),
        reverse(&source[mapping_start..], "[[language_mappings]]")
    )
}

fn html_cell(value: &str) -> String {
    let escaped = value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('|', "&#124;")
        .replace('\r', "&#13;")
        .replace('\n', "&#10;");
    format!("<code>{escaped}</code>")
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one generated-registry proof keeps all cross-table ordinal assertions adjacent"
)]
fn provider_registry_is_exact_deterministic_and_order_independent() {
    let path = lock_path();
    let lock = GrammarPackLock::load(&path).unwrap();
    let expected_hash = lock_file_hash(&path).unwrap();

    let first = render_provider(&path).unwrap();
    let second = render_provider(&path).unwrap();

    assert_eq!(first, second);
    syn::parse_file(&first).expect("generated provider must be valid Rust syntax");
    assert_eq!(
        first.lines().next().unwrap(),
        format!("// goldeneye-full-pack-lock-sha256: {expected_hash}")
    );
    assert!(first.contains(&format!(
        "pub(crate) const FULL_PACK_UPSTREAM_COMMIT: &str = {:?};",
        lock.upstream_commit()
    )));
    assert!(!first.contains(workspace_root().to_string_lossy().as_ref()));
    assert!(!first.to_ascii_lowercase().contains("timestamp"));

    assert_eq!(
        first
            .lines()
            .filter(|line| line.trim_start().starts_with("GeneratedLanguage {"))
            .count(),
        160
    );
    assert_eq!(
        first
            .lines()
            .filter(|line| line.trim_start().starts_with("GeneratedGrammar {"))
            .count(),
        157
    );
    assert_eq!(
        first.matches("GeneratedAvailability::Available").count(),
        159
    );
    assert_eq!(
        first.matches("GeneratedAvailability::Unavailable").count(),
        1
    );
    assert_eq!(
        first
            .lines()
            .filter(|line| line.trim_start().starts_with("fn grammar_"))
            .count(),
        157
    );
    let mut expected_mappings = lock.language_mappings.iter().collect::<Vec<_>>();
    expected_mappings.sort_unstable_by(|left, right| {
        left.language_id
            .as_bytes()
            .cmp(right.language_id.as_bytes())
    });
    let language_rows = first
        .lines()
        .filter(|line| line.trim_start().starts_with("GeneratedLanguage {"))
        .collect::<Vec<_>>();
    let bound = lock
        .language_mappings
        .iter()
        .filter(|mapping| mapping.status == LanguageBindingStatus::Available)
        .map(|mapping| mapping.grammar.as_deref().unwrap())
        .collect::<BTreeSet<_>>();
    let grammar_indices = bound
        .iter()
        .enumerate()
        .map(|(index, grammar)| (*grammar, index))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(language_rows.len(), expected_mappings.len());
    for (row, mapping) in language_rows.iter().zip(&expected_mappings) {
        assert!(
            row.contains(&format!("id: {:?},", mapping.language_id)),
            "language rows are not in exact lexical lock order: {row}"
        );
        match mapping.status {
            LanguageBindingStatus::Available => {
                let expected_index = grammar_indices[mapping.grammar.as_deref().unwrap()];
                assert_eq!(
                    grammar_index(row),
                    expected_index,
                    "wrong grammar index for {}",
                    mapping.language_id
                );
            }
            LanguageBindingStatus::Unavailable => {
                assert!(row.contains("GeneratedAvailability::Unavailable"));
            }
        }
    }
    for ordinal in 0..157 {
        assert!(
            first.contains(&format!("fn grammar_{ordinal:03}() -> *const ();")),
            "missing ordinal extern {ordinal}"
        );
    }
    assert!(!first.contains("fn grammar_157()"));
    let factories = first
        .split_once("const FACTORIES: [unsafe extern \"C\" fn() -> *const (); 157] = [\n")
        .unwrap()
        .1
        .split_once("];")
        .unwrap()
        .0
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let expected_factories = (0..157)
        .map(|ordinal| format!("grammar_{ordinal:03},"))
        .collect::<Vec<_>>();
    assert_eq!(factories, expected_factories);

    let nim = language_line(&first, "nim");
    let nim_reason = lock
        .language_mappings
        .iter()
        .find(|mapping| mapping.language_id == "nim")
        .and_then(|mapping| mapping.reason.as_deref())
        .unwrap();
    assert_eq!(
        nim.trim(),
        format!(
            "GeneratedLanguage {{ id: \"nim\", availability: GeneratedAvailability::Unavailable {{ reason: {nim_reason:?} }} }},"
        )
    );
    let yaml_index = grammar_index(language_line(&first, "yaml"));
    assert_eq!(grammar_index(language_line(&first, "k8s")), yaml_index);
    assert_eq!(
        grammar_index(language_line(&first, "kustomize")),
        yaml_index
    );
    assert!(!first.contains("objectscript_routine"));
    assert!(!first.contains("objectscript_udl"));

    assert_eq!(bound.len(), 157);
    let mut callable_abi_histogram = BTreeMap::new();
    for name in &bound {
        let grammar = lock
            .grammars
            .iter()
            .find(|grammar| grammar.name == *name)
            .unwrap();
        *callable_abi_histogram.entry(grammar.abi).or_insert(0) += 1;
    }
    assert_eq!(
        callable_abi_histogram,
        BTreeMap::from([(13, 9), (14, 78), (15, 70)])
    );
    let grammar_rows = first
        .lines()
        .filter(|line| line.trim_start().starts_with("GeneratedGrammar {"))
        .collect::<Vec<_>>();
    for (ordinal, name) in bound.iter().enumerate() {
        let grammar = lock
            .grammars
            .iter()
            .find(|grammar| grammar.name == *name)
            .unwrap();
        let link_name = format!("goldeneye_full_{}", grammar.exported_symbol);
        assert!(first.contains(&format!(
            "#[link_name = {link_name:?}]\n    fn grammar_{ordinal:03}() -> *const ();"
        )));
        assert_eq!(
            grammar_rows[ordinal].trim(),
            format!(
                "GeneratedGrammar {{ name: {:?}, exported_symbol: {:?}, abi: {}, scanner_language: {:?}, source_hash: {:?} }},",
                grammar.name,
                grammar.exported_symbol,
                grammar.abi,
                grammar.scanner_language,
                grammar.source_hash
            )
        );
    }

    let temporary = tempdir().unwrap();
    let reordered_path = temporary.path().join("reordered.toml");
    let original = fs::read_to_string(&path).unwrap();
    fs::write(&reordered_path, reverse_lock_tables(&original)).unwrap();
    let reordered = render_provider(&reordered_path).unwrap();
    assert_eq!(
        without_embedded_lock_hash(&first),
        without_embedded_lock_hash(&reordered)
    );
}

#[test]
fn provider_renderer_escapes_lock_controlled_rust_strings() {
    let source = fs::read_to_string(lock_path()).unwrap();
    let mut escaped = source.replacen(
        "upstream_commit = \"2469ecc3a7a2f80debe296e1f17a1efcfdb9450c\"",
        r#"upstream_commit = "commit \" slash \\ newline \n tab \t""#,
        1,
    );
    let marker = "language_id = \"nim\"";
    let mapping_start = escaped.find(marker).unwrap();
    let reason_start = mapping_start + escaped[mapping_start..].find("reason = ").unwrap();
    let reason_end = reason_start + escaped[reason_start..].find('\n').unwrap();
    escaped.replace_range(
        reason_start..reason_end,
        r#"reason = "quote \" slash \\ newline \n tab \t""#,
    );
    let temporary = tempdir().unwrap();
    let path = temporary.path().join("escaped.toml");
    fs::write(&path, escaped).unwrap();

    let generated = render_provider(&path).unwrap();

    syn::parse_file(&generated).expect("escaped provider must remain valid Rust syntax");
    assert!(
        generated.contains(
            r#"FULL_PACK_UPSTREAM_COMMIT: &str = "commit \" slash \\ newline \n tab \t";"#
        )
    );
    assert!(generated.contains(r#"reason: "quote \" slash \\ newline \n tab \t""#));
}

#[test]
fn generated_outputs_support_non_mutating_check_mode() {
    let temporary = tempdir().unwrap();
    let provider = temporary.path().join("generated.rs");
    let notices = temporary.path().join("notices.md");
    let missing_provider = temporary.path().join("missing-generated.rs");
    let missing_notices = temporary.path().join("missing-notices.md");

    assert!(
        generate_provider(lock_path(), &missing_provider, true)
            .unwrap_err()
            .to_string()
            .contains("drift")
    );
    assert!(
        generate_notices(lock_path(), &missing_notices, true)
            .unwrap_err()
            .to_string()
            .contains("drift")
    );
    assert!(!missing_provider.exists());
    assert!(!missing_notices.exists());

    assert_eq!(
        generate_provider(lock_path(), &provider, false).unwrap(),
        GenerationOutcome::Written
    );
    assert_eq!(
        generate_provider(lock_path(), &provider, false).unwrap(),
        GenerationOutcome::Unchanged
    );
    let provider_content = fs::read(&provider).unwrap();
    let provider_modified = fs::metadata(&provider).unwrap().modified().unwrap();
    let provider_permissions = fs::metadata(&provider).unwrap().permissions();
    let parent_permissions = fs::metadata(temporary.path()).unwrap().permissions();
    let mut provider_read_only = provider_permissions.clone();
    provider_read_only.set_readonly(true);
    let mut parent_read_only = parent_permissions.clone();
    parent_read_only.set_readonly(true);
    fs::set_permissions(&provider, provider_read_only).unwrap();
    fs::set_permissions(temporary.path(), parent_read_only).unwrap();
    let provider_check = generate_provider(lock_path(), &provider, true);
    fs::set_permissions(temporary.path(), parent_permissions.clone()).unwrap();
    fs::set_permissions(&provider, provider_permissions).unwrap();
    assert_eq!(provider_check.unwrap(), GenerationOutcome::Unchanged);
    assert_eq!(fs::read(&provider).unwrap(), provider_content);
    assert_eq!(
        fs::metadata(&provider).unwrap().modified().unwrap(),
        provider_modified
    );
    assert_eq!(
        generate_notices(lock_path(), &notices, false).unwrap(),
        GenerationOutcome::Written
    );
    let notices_content = fs::read(&notices).unwrap();
    let notices_modified = fs::metadata(&notices).unwrap().modified().unwrap();
    let notices_permissions = fs::metadata(&notices).unwrap().permissions();
    let mut notices_read_only = notices_permissions.clone();
    notices_read_only.set_readonly(true);
    let mut parent_read_only = parent_permissions.clone();
    parent_read_only.set_readonly(true);
    fs::set_permissions(&notices, notices_read_only).unwrap();
    fs::set_permissions(temporary.path(), parent_read_only).unwrap();
    let notices_check = generate_notices(lock_path(), &notices, true);
    fs::set_permissions(temporary.path(), parent_permissions).unwrap();
    fs::set_permissions(&notices, notices_permissions).unwrap();
    assert_eq!(notices_check.unwrap(), GenerationOutcome::Unchanged);
    assert_eq!(fs::read(&notices).unwrap(), notices_content);
    assert_eq!(
        fs::metadata(&notices).unwrap().modified().unwrap(),
        notices_modified
    );

    fs::write(&provider, "stale provider\n").unwrap();
    fs::write(&notices, "stale notices\n").unwrap();
    let provider_error = generate_provider(lock_path(), &provider, true)
        .unwrap_err()
        .to_string();
    let notices_error = generate_notices(lock_path(), &notices, true)
        .unwrap_err()
        .to_string();

    assert!(provider_error.contains("drift"), "{provider_error}");
    assert!(notices_error.contains("drift"), "{notices_error}");
    assert_eq!(fs::read_to_string(&provider).unwrap(), "stale provider\n");
    assert_eq!(fs::read_to_string(&notices).unwrap(), "stale notices\n");
}

#[test]
fn license_ledger_accounts_for_native_support_licenses() {
    let notices = render_notices(lock_path()).unwrap();

    assert!(notices.contains("## Native Support Assets"));
    assert!(notices.contains("<code>common/LICENSE</code>"));
    assert!(notices.contains("<code>common/tree_sitter/LICENSE</code>"));
}

#[test]
fn license_ledger_has_exact_sorted_provenance_rows() {
    let path = lock_path();
    let lock = GrammarPackLock::load(&path).unwrap();
    let expected_hash = lock_file_hash(&path).unwrap();

    let first = render_notices(&path).unwrap();
    let second = render_notices(&path).unwrap();

    assert_eq!(first, second);
    assert!(!first.contains(workspace_root().to_string_lossy().as_ref()));
    assert!(!first.to_ascii_lowercase().contains("timestamp"));
    let lines = first.lines().collect::<Vec<_>>();
    assert_eq!(
        &lines[..8],
        &[
            "# Full Grammar Pack License Ledger",
            "",
            "Generated by cargo xtask grammars generate-notices; do not edit.",
            "",
            &format!("<!-- goldeneye-full-pack-lock-sha256: {expected_hash} -->"),
            "",
            "| Grammar | Repository | Revision or missing-revision reason | Direct license path | Source SHA-256 |",
            "| --- | --- | --- | --- | --- |",
        ]
    );
    let grammar_rows = &lines[8..8 + 159];
    assert_eq!(grammar_rows.len(), 159);
    assert_eq!(
        lock.grammars
            .iter()
            .filter(|grammar| grammar.commit.is_some())
            .count(),
        148
    );
    assert_eq!(
        lock.grammars
            .iter()
            .filter(|grammar| grammar.missing_commit_reason.is_some())
            .count(),
        11
    );

    let mut expected_names = lock
        .grammars
        .iter()
        .map(|grammar| grammar.name.as_str())
        .collect::<Vec<_>>();
    expected_names.sort_unstable();
    let actual_names = grammar_rows
        .iter()
        .map(|row| {
            row.strip_prefix("| <code>")
                .unwrap()
                .split_once("</code>")
                .unwrap()
                .0
        })
        .collect::<Vec<_>>();
    assert_eq!(actual_names, expected_names);

    let records = lock
        .grammars
        .iter()
        .map(|grammar| (grammar.name.as_str(), grammar))
        .collect::<BTreeMap<_, _>>();
    let expected_rows = expected_names
        .iter()
        .map(|name| {
            let grammar = records[name];
            let revision = grammar
                .commit
                .as_deref()
                .or(grammar.missing_commit_reason.as_deref())
                .unwrap();
            format!(
                "| {} | {} | {} | {} | {} |",
                html_cell(&grammar.name),
                html_cell(&grammar.repository),
                html_cell(revision),
                html_cell(&format!("{}/LICENSE", grammar.name)),
                html_cell(&grammar.source_hash)
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        grammar_rows,
        expected_rows.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert_native_support_ledger(&lock, &lines[8 + 159..]);
    assert!(first.contains("<code>objectscript_routine</code>"));
    assert!(first.contains("<code>objectscript_udl</code>"));

    let temporary = tempdir().unwrap();
    let reordered_path = temporary.path().join("reordered.toml");
    let original = fs::read_to_string(&path).unwrap();
    fs::write(&reordered_path, reverse_lock_tables(&original)).unwrap();
    let reordered = render_notices(&reordered_path).unwrap();
    assert_eq!(
        without_embedded_lock_hash(&first),
        without_embedded_lock_hash(&reordered)
    );
}

fn assert_native_support_ledger(lock: &GrammarPackLock, support_lines: &[&str]) {
    assert_eq!(
        &support_lines[..5],
        &[
            "",
            "## Native Support Assets",
            "",
            "| Support group | Repository | Revision or missing-revision reason | License path | Source SHA-256 |",
            "| --- | --- | --- | --- | --- |",
        ]
    );
    let support_rows = &support_lines[5..];
    assert_eq!(support_rows.len(), 2);
    let support = &lock.native_support[0];
    let revision = support
        .commit
        .as_deref()
        .or(support.missing_commit_reason.as_deref())
        .unwrap();
    let expected_support_rows = support
        .license_files
        .iter()
        .map(|license| {
            format!(
                "| {} | {} | {} | {} | {} |",
                html_cell(&support.name),
                html_cell(&support.repository),
                html_cell(revision),
                html_cell(&format!("{}/{}", support.name, license)),
                html_cell(&support.source_hash)
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        support_rows,
        expected_support_rows
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    );
}

#[test]
fn notice_renderer_escapes_markdown_controlled_repository_and_revision() {
    fn replace_field(source: &mut String, record_start: usize, field: &str, value: &str) {
        let start = record_start + source[record_start..].find(field).unwrap();
        let end = start + source[start..].find('\n').unwrap();
        source.replace_range(start..end, value);
    }

    let mut source = fs::read_to_string(lock_path()).unwrap();
    let record_start = source.find("name = \"ada\"").unwrap();
    replace_field(
        &mut source,
        record_start,
        "repository = ",
        r#"repository = "https://example.invalid/a|b?<x>&y\\z\nline\rreturn""#,
    );
    let record_start = source.find("name = \"ada\"").unwrap();
    replace_field(
        &mut source,
        record_start,
        "commit = ",
        r#"commit = "revision | <tag> & \"quote\"\nline\rreturn""#,
    );
    let temporary = tempdir().unwrap();
    let path = temporary.path().join("escaped-notices.toml");
    fs::write(&path, source).unwrap();

    let notices = render_notices(&path).unwrap();
    let lock = GrammarPackLock::load(&path).unwrap();
    let grammar = lock
        .grammars
        .iter()
        .find(|grammar| grammar.name == "ada")
        .unwrap();
    let expected = format!(
        "| {} | {} | {} | {} | {} |",
        html_cell(&grammar.name),
        html_cell(&grammar.repository),
        html_cell(grammar.commit.as_deref().unwrap()),
        html_cell("ada/LICENSE"),
        html_cell(&grammar.source_hash)
    );

    assert_eq!(
        notices
            .lines()
            .find(|line| line.starts_with("| <code>ada</code>"))
            .unwrap(),
        expected
    );
    assert!(expected.contains("&#124;"));
    assert!(expected.contains("&lt;"));
    assert!(expected.contains("&amp;"));
    assert!(expected.contains("&#10;"));
    assert!(expected.contains("&#13;"));
}

#[test]
fn cli_generates_and_checks_provider_and_notices() {
    let temporary = tempdir().unwrap();
    let provider = temporary.path().join("generated.rs");
    let notices = temporary.path().join("notices.md");
    let binary = env!("CARGO_BIN_EXE_xtask");

    for (command, output) in [
        ("generate-provider", provider.as_path()),
        ("generate-notices", notices.as_path()),
    ] {
        let missing_check = Command::new(binary)
            .args(["grammars", command, "--lock"])
            .arg(lock_path())
            .arg("--output")
            .arg(output)
            .arg("--check")
            .output()
            .unwrap();
        assert!(!missing_check.status.success());
        assert!(!output.exists());

        let generated = Command::new(binary)
            .args(["grammars", command, "--lock"])
            .arg(lock_path())
            .arg("--output")
            .arg(output)
            .output()
            .unwrap();
        assert!(
            generated.status.success(),
            "{}",
            String::from_utf8_lossy(&generated.stderr)
        );
        let expected = if command == "generate-provider" {
            render_provider(lock_path()).unwrap()
        } else {
            render_notices(lock_path()).unwrap()
        };
        assert_eq!(fs::read_to_string(output).unwrap(), expected);
        let checked = Command::new(binary)
            .args(["grammars", command, "--lock"])
            .arg(lock_path())
            .arg("--output")
            .arg(output)
            .arg("--check")
            .output()
            .unwrap();
        assert!(
            checked.status.success(),
            "{}",
            String::from_utf8_lossy(&checked.stderr)
        );
        assert_eq!(fs::read_to_string(output).unwrap(), expected);
    }
}

#[test]
fn full_grammar_crate_is_default_empty_and_lint_isolated() {
    let crate_root = workspace_root().join("crates/goldeneye-full-grammars");
    let manifest = fs::read_to_string(crate_root.join("Cargo.toml")).unwrap();
    let library = fs::read_to_string(crate_root.join("src/lib.rs")).unwrap();

    assert!(manifest.contains("[features]\ndefault = []"));
    assert!(manifest.contains("[lints.rust]\nunsafe_code = \"deny\""));
    assert!(manifest.contains("[lints.clippy]\nall = \"deny\"\npedantic = \"deny\""));
    assert!(!manifest.contains("[lints]\nworkspace = true"));
    assert!(manifest.contains("tree-sitter-language = { workspace = true, optional = true }"));
    assert!(manifest.contains("compiled = ["));
    assert!(
        library.contains("#[cfg(feature = \"compiled\")]\n#[allow(unsafe_code)]\nmod generated")
    );
    assert!(library.contains("include!(\"generated.rs\")"));
    assert!(!library.contains("GOLDENEYE_GRAMMAR_PACK_DIR"));
    assert!(crate_root.join("src/generated.rs").is_file());
}
