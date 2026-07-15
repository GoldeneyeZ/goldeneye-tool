use super::first_quoted_value;

pub(super) fn embedded_es_imports(language: &str, source: &[u8]) -> Vec<String> {
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

pub(super) fn import_bindings(language: &str, text: &str) -> Vec<(String, String)> {
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

fn split_alias(value: &str) -> (String, Option<String>) {
    for separator in [" as ", " AS "] {
        if let Some((target, alias)) = value.split_once(separator) {
            return (target.trim().to_owned(), Some(binding_key(alias.trim())));
        }
    }
    (value.trim().to_owned(), None)
}

fn quoted_values(text: &str) -> Vec<String> {
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

pub(super) fn normalize_import_path(value: &str) -> String {
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

pub(super) fn import_alias(module_path: &str) -> String {
    module_path
        .rsplit('.')
        .find(|segment| !segment.is_empty() && *segment != "*")
        .map(binding_key)
        .unwrap_or_default()
}

pub(super) fn binding_key(value: &str) -> String {
    value
        .trim()
        .trim_start_matches(['$', '&', '*'])
        .trim_matches(|character: char| {
            !character.is_alphanumeric() && character != '_' && character != '.'
        })
        .to_owned()
}

pub(super) fn infer_declared_type(text: &str, name: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_binding_dispatch_preserves_supported_syntaxes() {
        let cases = [
            (
                "python",
                "from pkg.mod import Thing as Alias, Other, *\nimport top.level, local.mod as lm",
                vec![
                    ("Alias", "pkg.mod.Thing"),
                    ("Other", "pkg.mod.Other"),
                    ("top", "top.level"),
                    ("lm", "local.mod"),
                ],
            ),
            (
                "typescript",
                "import { Foo as Bar, Baz } from '../pkg/mod.ts';\nimport * as ns from '@scope/pkg';\nimport Widget from './widget.js';\nimport './setup.js';",
                vec![
                    ("Bar", "pkg.mod.Foo"),
                    ("Baz", "pkg.mod.Baz"),
                    ("ns", "@scope.pkg"),
                    ("Widget", "widget.Widget"),
                    ("setup", "setup"),
                ],
            ),
            (
                "kotlin",
                "import java.util.Collections.emptyList\nimport foo.bar.Baz as Qux",
                vec![
                    ("emptyList", "java.util.Collections.emptyList"),
                    ("Qux", "foo.bar.Baz"),
                ],
            ),
            (
                "csharp",
                "using Alias = Company.Product.Type;\nusing System.Text;",
                vec![("Alias", "Company.Product.Type"), ("Text", "System.Text")],
            ),
            (
                "php",
                "use function Vendor\\Pkg\\helper as h;\nuse Vendor\\Pkg\\Thing;",
                vec![("h", "Vendor.Pkg.helper"), ("Thing", "Vendor.Pkg.Thing")],
            ),
            (
                "rust",
                "use crate::foo::{Bar as Baz, Qux};\nuse other::Item;",
                vec![
                    ("Baz", "crate.foo.Bar"),
                    ("Qux", "crate.foo.Qux"),
                    ("Item", "other.Item"),
                ],
            ),
            (
                "go",
                "import (\n alias \"example.com/pkg\"\n _ \"example.com/blank\"\n . \"example.com/dot\"\n)",
                vec![
                    ("alias", "example.com.pkg"),
                    ("blank", "example.com.blank"),
                    ("dot", "example.com.dot"),
                ],
            ),
            (
                "cpp",
                "#include \"local/foo.h\"\n#include <sys/types.h>",
                vec![("foo", "local.foo"), ("types", "sys.types")],
            ),
            (
                "unknown",
                "load \"pkg/file.ts\"",
                vec![("file", "pkg.file")],
            ),
        ];

        for (language, source, expected) in cases {
            let expected = expected
                .into_iter()
                .map(|(alias, target)| (alias.to_owned(), target.to_owned()))
                .collect::<Vec<_>>();
            assert_eq!(import_bindings(language, source), expected, "{language}");
        }
    }

    #[test]
    fn embedded_imports_are_scoped_sorted_and_deduplicated() {
        let source = br#"
            <script> import Zed from "z/pkg";
            import Alpha from './a.ts';
            <script> import ZedAgain from "z/pkg";
            const ignored = "import fake from 'nope'";
        "#;
        assert_eq!(
            embedded_es_imports("vue", source),
            vec!["./a.ts".to_owned(), "z/pkg".to_owned()]
        );
        assert!(embedded_es_imports("typescript", source).is_empty());
    }

    #[test]
    fn lexical_helpers_preserve_current_edge_cases() {
        let normalization = [
            ("../pkg/mod.ts", "pkg.mod"),
            ("crate::nested::Type", "crate.nested.Type"),
            ("Vendor\\Pkg\\Thing.php", "Vendor.Pkg.Thing"),
            ("./archive.test.js", "archive.test"),
            ("pkg::*", "pkg.*"),
        ];
        for (input, expected) in normalization {
            assert_eq!(normalize_import_path(input), expected, "{input}");
        }

        assert_eq!(
            split_alias("Thing as Alias"),
            ("Thing".to_owned(), Some("Alias".to_owned()))
        );
        assert_eq!(
            split_alias("Thing AS Alias"),
            ("Thing".to_owned(), Some("Alias".to_owned()))
        );
        assert_eq!(
            split_alias("Thing As Alias"),
            ("Thing As Alias".to_owned(), None)
        );
        assert_eq!(
            quoted_values("'one' \"two\" 'unterminated"),
            vec!["one", "two"]
        );
        assert_eq!(import_alias("pkg.*"), "pkg");
    }

    #[test]
    fn declared_type_inference_preserves_priority_and_boundaries() {
        let cases = [
            ("let value: &ns::Type = make()", "value", Some("ns.Type")),
            ("value as Vendor\\Type", "value", Some("Vendor.Type")),
            ("Widget value", "value", Some("Widget")),
            ("var value = new Widget()", "value", Some("Widget")),
            ("value := Widget{}", "value", Some("Widget")),
            ("let value: Vec<Item> = make()", "value", Some("Vec")),
            ("value2: Wrong", "value", None),
            ("const value = 42", "value", None),
        ];

        for (source, name, expected) in cases {
            assert_eq!(
                infer_declared_type(source, name).as_deref(),
                expected,
                "{source}"
            );
        }
    }
}
