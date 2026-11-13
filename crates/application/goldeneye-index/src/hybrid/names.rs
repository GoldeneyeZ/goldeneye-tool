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

/// Whether `value` ends with `.{tail}`.
pub(super) fn ends_with_segment(value: &str, tail: &str) -> bool {
    value
        .strip_suffix(tail)
        .is_some_and(|rest| rest.ends_with('.'))
}

/// Whether two normalized names are equal or one is the last segments of the other.
pub(super) fn tail_eq(left: &str, right: &str) -> bool {
    left == right || ends_with_segment(left, right) || ends_with_segment(right, left)
}

/// Whether `value` ends with `{owner}.{name}`.
pub(super) fn ends_with_joined(value: &str, owner: &str, name: &str) -> bool {
    value
        .strip_suffix(name)
        .and_then(|rest| rest.strip_suffix('.'))
        .is_some_and(|rest| rest.ends_with(owner))
}

/// Whether `{owner}.{name}` ends with `value`.
pub(super) fn joined_ends_with(owner: &str, name: &str, value: &str) -> bool {
    if value.len() <= name.len() {
        return name.ends_with(value);
    }
    value
        .strip_suffix(name)
        .and_then(|rest| rest.strip_suffix('.'))
        .is_some_and(|rest| owner.ends_with(rest))
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

#[cfg(test)]
mod tests {
    use super::{ends_with_joined, ends_with_segment, joined_ends_with, tail_eq};

    const NAMES: [&str; 11] = [
        "", "a", "b", "a.b", "x.a.b", "ab", "a.b.c", "b.c", "a.", ".b", "é.b",
    ];

    #[test]
    fn segment_checks_agree_with_the_strings_they_stand_for() {
        for value in NAMES {
            for owner in NAMES {
                assert_eq!(
                    ends_with_segment(value, owner),
                    value.ends_with(&format!(".{owner}")),
                    "{value:?} ends with .{owner:?}"
                );
                for name in NAMES {
                    let joined = format!("{owner}.{name}");
                    assert_eq!(
                        ends_with_joined(value, owner, name),
                        value.ends_with(&joined),
                        "{value:?} ends with {joined:?}"
                    );
                    assert_eq!(
                        joined_ends_with(owner, name, value),
                        joined.ends_with(value),
                        "{joined:?} ends with {value:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_name_matches_another_when_one_is_the_last_segments_of_the_other() {
        assert!(tail_eq("Engine", "Engine"));
        assert!(tail_eq("Engine", "sync.Engine"));
        assert!(tail_eq("sync.Engine", "Engine"));
        assert!(!tail_eq("Engine", "SyncEngine"));
        assert!(!tail_eq("sync.Engine", "db.Engine"));
    }
}
