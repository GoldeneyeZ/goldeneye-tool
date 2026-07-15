use super::first_quoted_value;

mod bindings;
mod declared_types;
mod normalization;

pub(super) use bindings::{embedded_es_imports, import_bindings};
pub(super) use declared_types::infer_declared_type;
pub(super) use normalization::{binding_key, import_alias, normalize_import_path};

#[cfg(test)]
use bindings::{quoted_values, split_alias};

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
