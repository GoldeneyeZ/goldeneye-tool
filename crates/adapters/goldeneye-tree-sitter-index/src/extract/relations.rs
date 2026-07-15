use tree_sitter::Node;

use super::node_text;

pub(super) fn audited_relations(
    language: &str,
    node: Node<'_>,
    source: &[u8],
) -> Vec<(&'static str, String)> {
    let text = node_text(node, source);
    if language == "python" {
        return python_base_relations(&text);
    }
    if language == "smali" {
        return text
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if let Some(target) = line.strip_prefix(".super ") {
                    return relation_target(target).map(|target| ("INHERITS", target));
                }
                line.strip_prefix(".implements ")
                    .and_then(relation_target)
                    .map(|target| ("IMPLEMENTS", target))
            })
            .collect();
    }
    if language == "objc"
        && let Some(header) = text.lines().next()
        && let Some((_, base)) = header.split_once(':')
        && let Some(target) = relation_target(base)
    {
        return vec![("INHERITS", target)];
    }

    let header = text.split('{').next().unwrap_or(&text);
    let mut relations = Vec::new();
    relations.extend(
        relation_names_after_keyword(header, "extends")
            .into_iter()
            .map(|target| ("INHERITS", target)),
    );
    relations.extend(
        relation_names_after_keyword(header, "implements")
            .into_iter()
            .map(|target| ("IMPLEMENTS", target)),
    );
    if relations.is_empty() && matches!(language, "cpp" | "cuda" | "csharp" | "kotlin" | "rust") {
        relations.extend(colon_base_relations(language, header));
    }
    relations
}

fn python_base_relations(text: &str) -> Vec<(&'static str, String)> {
    let Some((_, bases)) = text.split_once('(') else {
        return Vec::new();
    };
    let Some((bases, _)) = bases.split_once(')') else {
        return Vec::new();
    };
    bases
        .split(',')
        .filter(|base| !base.contains('='))
        .filter_map(|base| {
            let base = base.split('[').next().unwrap_or(base);
            relation_target(base).map(|target| ("INHERITS", target))
        })
        .collect()
}

fn colon_base_relations(language: &str, header: &str) -> Vec<(&'static str, String)> {
    let Some((_, bases)) = header.split_once(':') else {
        return Vec::new();
    };
    bases
        .split(',')
        .enumerate()
        .filter_map(|(index, base)| {
            let base = base
                .trim()
                .strip_prefix("public ")
                .or_else(|| base.trim().strip_prefix("protected "))
                .or_else(|| base.trim().strip_prefix("private "))
                .unwrap_or(base.trim());
            let kind = if language == "csharp" && index > 0 {
                "IMPLEMENTS"
            } else {
                "INHERITS"
            };
            relation_target(base).map(|target| (kind, target))
        })
        .collect()
}

fn relation_names_after_keyword(text: &str, keyword: &str) -> Vec<String> {
    let Some(start) = find_word(text, keyword) else {
        return Vec::new();
    };
    let mut rest = text[start + keyword.len()..].trim_start();
    for terminator in [" extends ", " implements ", " where ", "{"] {
        if let Some(end) = rest.find(terminator) {
            rest = &rest[..end];
        }
    }
    rest.split([',', '&']).filter_map(relation_target).collect()
}

fn find_word(text: &str, word: &str) -> Option<usize> {
    text.match_indices(word).find_map(|(index, _)| {
        let before = text[..index].chars().next_back();
        let after = text[index + word.len()..].chars().next();
        let boundary = |character: Option<char>| {
            character.is_none_or(|character| !character.is_alphanumeric() && character != '_')
        };
        (boundary(before) && boundary(after)).then_some(index)
    })
}

fn relation_target(text: &str) -> Option<String> {
    let target = text
        .trim()
        .trim_end_matches(';')
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches(|character: char| {
            matches!(character, ':' | ',' | '(' | ')' | '<' | '>' | '"' | '\'')
        });
    (!target.is_empty()).then(|| target.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_and_colon_bases_preserve_relation_kinds() {
        assert_eq!(
            python_base_relations("class Child(Base[T], Mix, metaclass=Meta):"),
            vec![
                ("INHERITS", "Base".to_owned()),
                ("INHERITS", "Mix".to_owned())
            ]
        );
        assert!(python_base_relations("class Child(Base").is_empty());

        let cases = [
            (
                "cpp",
                "class Child : public Base, protected ns::Other",
                vec![("INHERITS", "Base"), ("INHERITS", "ns::Other")],
            ),
            (
                "csharp",
                "class Child : Base, IFoo, IBar",
                vec![
                    ("INHERITS", "Base"),
                    ("IMPLEMENTS", "IFoo"),
                    ("IMPLEMENTS", "IBar"),
                ],
            ),
        ];
        for (language, header, expected) in cases {
            let expected = expected
                .into_iter()
                .map(|(kind, target)| (kind, target.to_owned()))
                .collect::<Vec<_>>();
            assert_eq!(
                colon_base_relations(language, header),
                expected,
                "{language}"
            );
        }
    }

    #[test]
    fn keyword_relations_require_boundaries_and_stop_at_next_clause() {
        assert_eq!(
            relation_names_after_keyword(
                "class Child extends Base & Mix implements IFoo where T: Bound",
                "extends",
            ),
            vec!["Base", "Mix"]
        );
        assert_eq!(
            relation_names_after_keyword(
                "class Child extends Base implements IFoo, IBar where T: Bound",
                "implements",
            ),
            vec!["IFoo", "IBar"]
        );
        assert!(relation_names_after_keyword("class extendsThing", "extends").is_empty());
    }

    #[test]
    fn relation_targets_preserve_current_punctuation_and_generic_rules() {
        let cases = [
            (" (Base), ", Some("Base")),
            ("ns::Type; trailing", Some("ns::Type;")),
            ("Base<T>", Some("Base<T")),
            (" ,() ", None),
        ];
        for (input, expected) in cases {
            assert_eq!(relation_target(input).as_deref(), expected, "{input}");
        }
    }
}
