use crate::types::QueryError;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum TokenKind {
    Identifier(String),
    String(String),
    Number(String),
    Symbol(Symbol),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Symbol {
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,
    Colon,
    Comma,
    Dot,
    Dash,
    ArrowRight,
    ArrowLeft,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Regex,
    Star,
    Pipe,
}

#[derive(Debug, Clone)]
pub(super) struct Token {
    pub(super) kind: TokenKind,
    pub(super) position: usize,
}

pub(super) fn lex(input: &str) -> Result<Vec<Token>, QueryError> {
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < input.len() {
        let character = input[index..]
            .chars()
            .next()
            .expect("index remains on a character boundary");
        if character.is_whitespace() {
            index += character.len_utf8();
            continue;
        }
        if character == '\'' || character == '"' {
            let (value, next) = lex_string(input, index, character)?;
            tokens.push(Token {
                kind: TokenKind::String(value),
                position: index,
            });
            index = next;
            continue;
        }
        if character == '`' {
            let (value, next) = lex_backtick_identifier(input, index)?;
            tokens.push(Token {
                kind: TokenKind::Identifier(value),
                position: index,
            });
            index = next;
            continue;
        }
        if character.is_ascii_digit() {
            let next = lex_number_end(input, index);
            tokens.push(Token {
                kind: TokenKind::Number(input[index..next].to_owned()),
                position: index,
            });
            index = next;
            continue;
        }
        if character == '_' || character.is_alphabetic() {
            let next = lex_identifier_end(input, index);
            tokens.push(Token {
                kind: TokenKind::Identifier(input[index..next].to_owned()),
                position: index,
            });
            index = next;
            continue;
        }
        let (symbol, consumed) = lex_symbol(input, index)?;
        tokens.push(Token {
            kind: TokenKind::Symbol(symbol),
            position: index,
        });
        index += consumed;
    }
    Ok(tokens)
}

fn lex_string(input: &str, start: usize, quote: char) -> Result<(String, usize), QueryError> {
    let mut value = String::new();
    let mut index = start + quote.len_utf8();
    while index < input.len() {
        let character = input[index..]
            .chars()
            .next()
            .expect("index remains on a character boundary");
        if character == quote {
            return Ok((value, index + character.len_utf8()));
        }
        if character == '\\' {
            index += character.len_utf8();
            let escaped = input[index..]
                .chars()
                .next()
                .ok_or_else(|| super::syntax(start, "unterminated string escape"))?;
            value.push(match escaped {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '\\' => '\\',
                '\'' => '\'',
                '"' => '"',
                other => other,
            });
            index += escaped.len_utf8();
            continue;
        }
        value.push(character);
        index += character.len_utf8();
    }
    Err(super::syntax(start, "unterminated string literal"))
}

fn lex_backtick_identifier(input: &str, start: usize) -> Result<(String, usize), QueryError> {
    let mut value = String::new();
    let mut index = start + 1;
    while index < input.len() {
        let character = input[index..]
            .chars()
            .next()
            .expect("index remains on a character boundary");
        if character == '`' {
            return Ok((value, index + 1));
        }
        value.push(character);
        index += character.len_utf8();
    }
    Err(super::syntax(start, "unterminated backtick identifier"))
}

fn lex_number_end(input: &str, start: usize) -> usize {
    let bytes = input.as_bytes();
    let mut index = start;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index + 1 < bytes.len() && bytes[index] == b'.' && bytes[index + 1].is_ascii_digit() {
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
    }
    index
}

fn lex_identifier_end(input: &str, start: usize) -> usize {
    let mut end = start;
    for (offset, character) in input[start..].char_indices() {
        if offset == 0 || character == '_' || character.is_alphanumeric() {
            end = start + offset + character.len_utf8();
        } else {
            break;
        }
    }
    end
}

fn lex_symbol(input: &str, index: usize) -> Result<(Symbol, usize), QueryError> {
    let rest = &input[index..];
    let pair = rest.get(..2);
    if let Some(symbol) = pair.and_then(|pair| match pair {
        "->" => Some(Symbol::ArrowRight),
        "<-" => Some(Symbol::ArrowLeft),
        "<>" | "!=" => Some(Symbol::NotEqual),
        "<=" => Some(Symbol::LessEqual),
        ">=" => Some(Symbol::GreaterEqual),
        "=~" => Some(Symbol::Regex),
        _ => None,
    }) {
        return Ok((symbol, 2));
    }
    let symbol = match rest.as_bytes()[0] {
        b'(' => Symbol::LeftParen,
        b')' => Symbol::RightParen,
        b'[' => Symbol::LeftBracket,
        b']' => Symbol::RightBracket,
        b'{' => Symbol::LeftBrace,
        b'}' => Symbol::RightBrace,
        b':' => Symbol::Colon,
        b',' => Symbol::Comma,
        b'.' => Symbol::Dot,
        b'-' => Symbol::Dash,
        b'=' => Symbol::Equal,
        b'<' => Symbol::Less,
        b'>' => Symbol::Greater,
        b'*' => Symbol::Star,
        b'|' => Symbol::Pipe,
        _ => return Err(super::syntax(index, "unsupported character")),
    };
    Ok((symbol, 1))
}

pub(super) fn reject_mutations(tokens: &[Token]) -> Result<(), QueryError> {
    const MUTATING: &[&str] = &[
        "ALTER", "CALL", "CREATE", "DELETE", "DETACH", "DROP", "FOREACH", "INSERT", "LOAD",
        "MERGE", "REMOVE", "SET", "UPDATE",
    ];
    for token in tokens {
        let TokenKind::Identifier(identifier) = &token.kind else {
            continue;
        };
        if let Some(keyword) = MUTATING
            .iter()
            .find(|keyword| identifier.eq_ignore_ascii_case(keyword))
        {
            return Err(QueryError::MutatingQuery {
                keyword: (*keyword).to_owned(),
            });
        }
    }
    Ok(())
}

pub(super) fn split_union_tokens(
    tokens: &[Token],
) -> Result<(Vec<Vec<Token>>, Vec<bool>), QueryError> {
    let mut branches = Vec::new();
    let mut modes = Vec::new();
    let mut current = Vec::new();
    let mut parentheses = 0usize;
    let mut brackets = 0usize;
    let mut braces = 0usize;
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        let at_top_level = parentheses == 0 && brackets == 0 && braces == 0;
        if at_top_level
            && matches!(
                &token.kind,
                TokenKind::Identifier(identifier) if identifier.eq_ignore_ascii_case("UNION")
            )
        {
            if current.is_empty() {
                return Err(super::syntax(
                    token.position,
                    "UNION is missing its left query",
                ));
            }
            branches.push(std::mem::take(&mut current));
            index += 1;
            let all = tokens.get(index).is_some_and(|token| {
                matches!(
                    &token.kind,
                    TokenKind::Identifier(identifier) if identifier.eq_ignore_ascii_case("ALL")
                )
            });
            if all {
                index += 1;
            }
            modes.push(all);
            continue;
        }
        if let TokenKind::Symbol(symbol) = token.kind {
            match symbol {
                Symbol::LeftParen => parentheses = parentheses.saturating_add(1),
                Symbol::RightParen => parentheses = parentheses.saturating_sub(1),
                Symbol::LeftBracket => brackets = brackets.saturating_add(1),
                Symbol::RightBracket => brackets = brackets.saturating_sub(1),
                Symbol::LeftBrace => braces = braces.saturating_add(1),
                Symbol::RightBrace => braces = braces.saturating_sub(1),
                _ => {}
            }
        }
        current.push(token.clone());
        index += 1;
    }
    if current.is_empty() {
        let position = tokens.last().map_or(0, |token| token.position);
        return Err(super::syntax(position, "UNION is missing its right query"));
    }
    branches.push(current);
    Ok((branches, modes))
}
