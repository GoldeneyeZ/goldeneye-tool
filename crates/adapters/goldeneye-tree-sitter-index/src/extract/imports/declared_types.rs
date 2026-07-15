use super::normalization::binding_key;

pub(in super::super) fn infer_declared_type(text: &str, name: &str) -> Option<String> {
    let name = binding_key(name);
    if name.is_empty() {
        return None;
    }
    let position = find_identifier_position(text, &name)?;
    let before = text[..position].trim_end();
    let after = text[position + name.len()..].trim_start();

    if let Some(type_text) = after.strip_prefix(':') {
        let candidate = take_type_name(type_text);
        if !candidate.is_empty() {
            return Some(candidate);
        }
    }
    if let Some(type_text) = after.strip_prefix("as ") {
        let candidate = take_type_name(type_text);
        if !candidate.is_empty() {
            return Some(candidate);
        }
    }
    let declared = before
        .split_whitespace()
        .next_back()
        .map(take_type_name)
        .unwrap_or_default();
    if !declared.is_empty()
        && !matches!(
            declared.as_str(),
            "let"
                | "const"
                | "var"
                | "auto"
                | "final"
                | "static"
                | "private"
                | "public"
                | "protected"
                | "internal"
                | "local"
        )
    {
        return Some(declared);
    }
    let rhs = after
        .split_once(":=")
        .map(|(_, rhs)| rhs)
        .or_else(|| after.split_once('=').map(|(_, rhs)| rhs))
        .unwrap_or(after)
        .trim_start();
    let rhs = rhs.strip_prefix("new ").unwrap_or(rhs).trim_start();
    let candidate = take_type_name(rhs);
    (!candidate.is_empty() && rhs[candidate.len()..].trim_start().starts_with(['(', '{']))
        .then_some(candidate)
}

fn find_identifier_position(text: &str, identifier: &str) -> Option<usize> {
    text.match_indices(identifier).find_map(|(index, _)| {
        let before = text[..index].chars().next_back();
        let after = text[index + identifier.len()..].chars().next();
        let boundary = |character: Option<char>| {
            character.is_none_or(|character| !character.is_alphanumeric() && character != '_')
        };
        (boundary(before) && boundary(after)).then_some(index)
    })
}

fn take_type_name(value: &str) -> String {
    value
        .trim_start_matches(|character: char| {
            character.is_whitespace() || matches!(character, '&' | '*' | '?' | '(')
        })
        .chars()
        .take_while(|character| {
            character.is_alphanumeric() || matches!(character, '_' | '.' | ':' | '\\' | '/' | '$')
        })
        .collect::<String>()
        .replace("::", ".")
        .replace(['\\', '/'], ".")
        .trim_end_matches('.')
        .to_owned()
}
