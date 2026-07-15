use goldeneye_ports::IndexExtractedImport as ExtractedImport;

pub(super) fn is_definition_label(label: &str) -> bool {
    is_callable_label(label) || is_type_label(label)
}

pub(super) fn is_callable_label(label: &str) -> bool {
    matches!(label, "Function" | "Method")
}

pub(super) fn is_type_label(label: &str) -> bool {
    matches!(
        label,
        "Class" | "Struct" | "Enum" | "Trait" | "Interface" | "Type" | "TypeAlias"
    )
}

pub(super) fn is_lsp_wired(language: &str) -> bool {
    matches!(
        language,
        "go" | "c"
            | "cpp"
            | "cuda"
            | "python"
            | "javascript"
            | "typescript"
            | "tsx"
            | "php"
            | "csharp"
            | "java"
            | "kotlin"
            | "rust"
    )
}

pub(super) fn language_compatible(caller: &str, target: &str) -> bool {
    caller == target
        || (matches!(caller, "c" | "cpp" | "cuda") && matches!(target, "c" | "cpp" | "cuda"))
        || (matches!(caller, "javascript" | "typescript" | "tsx")
            && matches!(target, "javascript" | "typescript" | "tsx"))
        || (matches!(caller, "java" | "kotlin") && matches!(target, "java" | "kotlin"))
}

pub(super) fn normalize_name(value: &str) -> String {
    value
        .trim()
        .trim_matches(|character: char| {
            character.is_whitespace() || matches!(character, '"' | '\'' | '`' | '(' | ')' | ';')
        })
        .trim_start_matches("new ")
        .replace("->", ".")
        .replace("::", ".")
        .replace(['\\', '/'], ".")
        .replace('$', "")
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join(".")
}

pub(super) fn binding_key(value: &str) -> String {
    normalize_name(value)
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_owned()
}

pub(super) fn owner_type(qualified_name: &str, name: &str) -> String {
    let normalized = normalize_name(qualified_name);
    let binding = binding_key(name);
    let parent = if binding.is_empty() {
        normalized.rsplit_once('.').map_or("", |(parent, _)| parent)
    } else {
        let suffix = format!(".{binding}");
        normalized
            .strip_suffix(&suffix)
            .unwrap_or_else(|| normalized.rsplit_once('.').map_or("", |(parent, _)| parent))
    };
    parent.rsplit('.').next().unwrap_or_default().to_owned()
}

pub(super) fn parent_qn(qualified_name: &str) -> String {
    qualified_name
        .rsplit_once('.')
        .map_or_else(String::new, |(parent, _)| parent.to_owned())
}

pub(super) fn call_receiver(callee: &str) -> Option<String> {
    let normalized = normalize_name(callee);
    normalized
        .rsplit_once('.')
        .map(|(receiver, _)| receiver.to_owned())
        .filter(|receiver| !receiver.is_empty())
}

pub(super) fn expand_alias(value: &str, imports: &[ExtractedImport]) -> String {
    let normalized = normalize_name(value);
    let prefix = normalized.split('.').next().unwrap_or_default();
    imports
        .iter()
        .find(|import| binding_key(&import.alias) == prefix)
        .map_or(normalized.clone(), |import| {
            let suffix = normalized.strip_prefix(prefix).unwrap_or_default();
            format!("{}{suffix}", normalize_name(&import.module_path))
        })
}

pub(super) fn tail_eq(left: &str, right: &str) -> bool {
    let left = normalize_name(left);
    let right = normalize_name(right);
    left == right || left.ends_with(&format!(".{right}")) || right.ends_with(&format!(".{left}"))
}

pub(super) fn normalized_suffix(qualified_name: &str, suffix: &str) -> bool {
    let qualified_name = normalize_name(qualified_name);
    let suffix = normalize_name(suffix);
    qualified_name == suffix || qualified_name.ends_with(&format!(".{suffix}"))
}

pub(super) fn import_reaches(qualified_name: &str, module: &str) -> bool {
    let qualified_name = normalize_name(qualified_name);
    let module = normalize_name(module);
    qualified_name.starts_with(&format!("{module}."))
        || qualified_name.contains(&format!(".{module}."))
        || qualified_name.ends_with(&format!(".{module}"))
}

pub(super) fn class_method_tail(value: &str) -> Option<String> {
    let normalized = normalize_name(value);
    let mut parts = normalized.rsplit('.');
    let method = parts.next()?;
    let class = parts.next()?;
    Some(format!("{class}.{method}"))
}

pub(super) fn is_builtin(language: &str, name: &str) -> bool {
    match language {
        "python" => matches!(
            name,
            "abs"
                | "all"
                | "any"
                | "dict"
                | "enumerate"
                | "filter"
                | "len"
                | "list"
                | "map"
                | "max"
                | "min"
                | "open"
                | "print"
                | "range"
                | "set"
                | "sorted"
                | "str"
                | "sum"
                | "tuple"
                | "zip"
        ),
        "javascript" | "typescript" | "tsx" => matches!(
            name,
            "alert"
                | "clearInterval"
                | "clearTimeout"
                | "fetch"
                | "parseFloat"
                | "parseInt"
                | "setInterval"
                | "setTimeout"
        ),
        "go" => matches!(
            name,
            "append"
                | "cap"
                | "clear"
                | "close"
                | "complex"
                | "copy"
                | "delete"
                | "len"
                | "make"
                | "max"
                | "min"
                | "new"
                | "panic"
                | "print"
                | "println"
                | "recover"
        ),
        "c" | "cpp" | "cuda" => matches!(
            name,
            "calloc"
                | "free"
                | "malloc"
                | "memcpy"
                | "memset"
                | "printf"
                | "puts"
                | "realloc"
                | "sizeof"
                | "snprintf"
                | "strcmp"
                | "strlen"
        ),
        "rust" => matches!(
            name,
            "drop" | "panic" | "print" | "println" | "todo" | "unreachable"
        ),
        "php" => matches!(
            name,
            "array" | "count" | "echo" | "isset" | "print" | "strlen"
        ),
        _ => false,
    }
}
