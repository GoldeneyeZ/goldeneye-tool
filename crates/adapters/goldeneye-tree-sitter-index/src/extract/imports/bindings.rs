use super::{binding_key, first_quoted_value, import_alias, normalize_import_path};

pub(in super::super) fn embedded_es_imports(language: &str, source: &[u8]) -> Vec<String> {
    if !matches!(language, "astro" | "svelte" | "vue") {
        return Vec::new();
    }
    let source = String::from_utf8_lossy(source);
    let mut imports = source
        .lines()
        .filter_map(|line| {
            let import = line
                .find("import ")
                .map(|start| &line[start..])
                .filter(|_| {
                    line.trim_start().starts_with("import ")
                        || line
                            .split_once("import ")
                            .is_some_and(|(prefix, _)| prefix.trim_end().ends_with('>'))
                })?;
            first_quoted_value(import)
        })
        .collect::<Vec<_>>();
    imports.sort();
    imports.dedup();
    imports
}

pub(in super::super) fn import_bindings(language: &str, text: &str) -> Vec<(String, String)> {
    match language {
        "python" => python_import_bindings(text),
        "javascript" | "typescript" | "tsx" | "astro" | "svelte" | "vue" => {
            es_import_bindings(text)
        }
        "java" | "kotlin" => jvm_import_bindings(text),
        "csharp" => csharp_import_bindings(text),
        "php" => php_import_bindings(text),
        "rust" => rust_import_bindings(text),
        "go" => go_import_bindings(text),
        "c" | "cpp" | "cuda" | "objc" => c_import_bindings(text),
        _ => generic_import_bindings(text),
    }
}

fn python_import_bindings(text: &str) -> Vec<(String, String)> {
    let mut imports = Vec::new();
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("from ")
            && let Some((module, names)) = rest.split_once(" import ")
        {
            let module = normalize_import_path(module);
            for item in names
                .trim_matches(|character| matches!(character, '(' | ')'))
                .split(',')
            {
                let (name, alias) = split_alias(item.trim());
                if name.is_empty() || name == "*" {
                    continue;
                }
                let alias = alias.unwrap_or_else(|| import_alias(&name));
                let target = format!("{module}.{}", normalize_import_path(&name));
                imports.push((alias, target.trim_matches('.').to_owned()));
            }
        } else if let Some(rest) = line.strip_prefix("import ") {
            for item in rest.split(',') {
                let (module, alias) = split_alias(item.trim());
                let module = normalize_import_path(&module);
                let alias = alias
                    .unwrap_or_else(|| module.split('.').next().unwrap_or_default().to_owned());
                if !alias.is_empty() && !module.is_empty() {
                    imports.push((alias, module));
                }
            }
        }
    }
    imports
}

fn es_import_bindings(text: &str) -> Vec<(String, String)> {
    let mut imports = Vec::new();
    for line in text.lines().map(str::trim) {
        let Some(import_at) = line.find("import ") else {
            continue;
        };
        let import = &line[import_at + "import ".len()..];
        let Some(module_raw) = first_quoted_value(import) else {
            continue;
        };
        let module = normalize_import_path(&module_raw);
        let specifier = import
            .split_once(" from ")
            .map(|(value, _)| value.trim())
            .unwrap_or_default();
        if specifier.starts_with('{') {
            for item in specifier
                .trim_matches(|c| matches!(c, '{' | '}'))
                .split(',')
            {
                let (name, alias) = split_alias(item.trim());
                if name.is_empty() {
                    continue;
                }
                let alias = alias.unwrap_or_else(|| name.clone());
                imports.push((alias, format!("{module}.{}", normalize_import_path(&name))));
            }
        } else if let Some(alias) = specifier.strip_prefix("* as ") {
            imports.push((binding_key(alias), module));
        } else if !specifier.is_empty() {
            let alias = specifier
                .split(',')
                .next()
                .map(binding_key)
                .unwrap_or_default();
            if !alias.is_empty() {
                imports.push((alias.clone(), format!("{module}.{alias}")));
            }
        } else {
            imports.push((import_alias(&module), module));
        }
    }
    imports
}

fn jvm_import_bindings(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("import ")?.trim();
            let rest = rest.strip_prefix("static ").unwrap_or(rest);
            let (target, alias) = split_alias(rest.trim_end_matches(';'));
            let target = normalize_import_path(&target);
            let alias = alias.unwrap_or_else(|| import_alias(&target));
            (!alias.is_empty() && !target.is_empty()).then_some((alias, target))
        })
        .collect()
}

fn csharp_import_bindings(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let rest = line
                .trim()
                .strip_prefix("using ")?
                .trim_end_matches(';')
                .trim();
            let (alias, target) = rest.split_once('=').map_or_else(
                || (None, rest),
                |(alias, target)| (Some(alias.trim()), target.trim()),
            );
            let target = normalize_import_path(target);
            let alias = alias.map_or_else(|| import_alias(&target), binding_key);
            (!alias.is_empty() && !target.is_empty()).then_some((alias, target))
        })
        .collect()
}

fn php_import_bindings(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let rest = line
                .trim()
                .strip_prefix("use ")?
                .trim_end_matches(';')
                .trim();
            let rest = rest
                .strip_prefix("function ")
                .or_else(|| rest.strip_prefix("const "))
                .unwrap_or(rest);
            let (target, alias) = split_alias(rest);
            let target = normalize_import_path(&target);
            let alias = alias.unwrap_or_else(|| import_alias(&target));
            (!alias.is_empty() && !target.is_empty()).then_some((alias, target))
        })
        .collect()
}

fn rust_import_bindings(text: &str) -> Vec<(String, String)> {
    let mut imports = Vec::new();
    for line in text.lines().map(str::trim) {
        let Some(rest) = line.strip_prefix("use ") else {
            continue;
        };
        let rest = rest.trim_end_matches(';').trim();
        if let Some((base, names)) = rest.split_once("::{") {
            let base = normalize_import_path(base);
            for item in names.trim_end_matches('}').split(',') {
                let (name, alias) = split_alias(item.trim());
                if name.is_empty() {
                    continue;
                }
                let alias = alias.unwrap_or_else(|| import_alias(&name));
                imports.push((alias, format!("{base}.{}", normalize_import_path(&name))));
            }
        } else {
            let (target, alias) = split_alias(rest);
            let target = normalize_import_path(&target);
            let alias = alias.unwrap_or_else(|| import_alias(&target));
            if !alias.is_empty() && !target.is_empty() {
                imports.push((alias, target));
            }
        }
    }
    imports
}

fn go_import_bindings(text: &str) -> Vec<(String, String)> {
    let mut imports = Vec::new();
    for line in text.lines().map(str::trim) {
        for module_raw in quoted_values(line) {
            let module = normalize_import_path(&module_raw);
            let quote_at = line.find(['"', '\'']).unwrap_or_default();
            let prefix = line[..quote_at].trim().trim_start_matches("import").trim();
            let alias = if prefix.is_empty() || matches!(prefix, "(" | "." | "_") {
                import_alias(&module)
            } else {
                binding_key(prefix.split_whitespace().last().unwrap_or_default())
            };
            if !alias.is_empty() && !module.is_empty() {
                imports.push((alias, module));
            }
        }
    }
    imports
}

fn c_import_bindings(text: &str) -> Vec<(String, String)> {
    quoted_values(text)
        .into_iter()
        .chain(
            text.split('<')
                .skip(1)
                .filter_map(|rest| rest.split_once('>').map(|(value, _)| value.to_owned())),
        )
        .filter_map(|path| {
            let module = normalize_import_path(&path);
            let alias = import_alias(&module);
            (!alias.is_empty() && !module.is_empty()).then_some((alias, module))
        })
        .collect()
}

fn generic_import_bindings(text: &str) -> Vec<(String, String)> {
    quoted_values(text)
        .into_iter()
        .filter_map(|path| {
            let module = normalize_import_path(&path);
            let alias = import_alias(&module);
            (!alias.is_empty() && !module.is_empty()).then_some((alias, module))
        })
        .collect()
}

pub(super) fn split_alias(value: &str) -> (String, Option<String>) {
    for separator in [" as ", " AS "] {
        if let Some((target, alias)) = value.split_once(separator) {
            return (target.trim().to_owned(), Some(binding_key(alias.trim())));
        }
    }
    (value.trim().to_owned(), None)
}

pub(super) fn quoted_values(text: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut start = None;
    let mut quote = '\0';
    for (index, character) in text.char_indices() {
        if let Some(value_start) = start {
            if character == quote {
                values.push(text[value_start..index].to_owned());
                start = None;
            }
        } else if matches!(character, '"' | '\'') {
            quote = character;
            start = Some(index + character.len_utf8());
        }
    }
    values
}
