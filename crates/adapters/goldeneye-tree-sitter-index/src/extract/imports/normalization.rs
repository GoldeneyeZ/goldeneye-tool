pub(in super::super) fn normalize_import_path(value: &str) -> String {
    let mut normalized = value
        .trim()
        .trim_matches(|character: char| {
            character.is_whitespace()
                || matches!(character, '"' | '\'' | '<' | '>' | ';' | '(' | ')')
        })
        .replace("::", ".")
        .replace(['/', '\\'], ".");
    while normalized.starts_with("./") || normalized.starts_with("../") {
        normalized = normalized
            .trim_start_matches("../")
            .trim_start_matches("./")
            .to_owned();
    }
    normalized = normalized.trim_start_matches('.').to_owned();
    for extension in [
        ".tsx", ".ts", ".jsx", ".js", ".mjs", ".cjs", ".py", ".rs", ".go", ".java", ".kt", ".cs",
        ".php", ".hpp", ".h", ".cpp", ".cc", ".c", ".cu",
    ] {
        if normalized.ends_with(extension) {
            normalized.truncate(normalized.len() - extension.len());
            break;
        }
    }
    normalized
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join(".")
}

pub(in super::super) fn import_alias(module_path: &str) -> String {
    module_path
        .rsplit('.')
        .find(|segment| !segment.is_empty() && *segment != "*")
        .map(binding_key)
        .unwrap_or_default()
}

pub(in super::super) fn binding_key(value: &str) -> String {
    value
        .trim()
        .trim_start_matches(['$', '&', '*'])
        .trim_matches(|character: char| {
            !character.is_alphanumeric() && character != '_' && character != '.'
        })
        .to_owned()
}
