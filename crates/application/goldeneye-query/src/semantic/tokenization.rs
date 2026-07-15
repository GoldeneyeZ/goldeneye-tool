use super::ABBREVIATIONS;

const TOKEN_BUFFER_LEN: usize = 128;

#[must_use]
pub fn tokenize_identifier(name: &str, max_tokens: usize) -> Vec<String> {
    if max_tokens == 0 {
        return Vec::new();
    }
    let mut tokens = scan_identifier(name, max_tokens);
    expand_abbreviations(&mut tokens, max_tokens);
    tokens
}

fn scan_identifier(name: &str, max_tokens: usize) -> Vec<String> {
    let bytes = name.as_bytes();
    let mut tokens = Vec::with_capacity(max_tokens.min(16));
    let mut buffer = Vec::with_capacity(TOKEN_BUFFER_LEN);

    for (index, &byte) in bytes.iter().enumerate() {
        if tokens.len() >= max_tokens {
            break;
        }
        let delimiter = matches!(
            byte,
            b'.' | b'/' | b'_' | b'-' | b' ' | b'(' | b')' | b',' | b':'
        );
        let camel_break =
            index > 0 && byte.is_ascii_uppercase() && bytes[index - 1].is_ascii_lowercase();
        if delimiter || camel_break {
            flush_token(&mut buffer, &mut tokens, max_tokens);
            if delimiter {
                continue;
            }
        }
        if buffer.len() < TOKEN_BUFFER_LEN - 1 && byte.is_ascii_alphanumeric() {
            buffer.push(byte.to_ascii_lowercase());
        }
    }
    flush_token(&mut buffer, &mut tokens, max_tokens);
    tokens
}

fn expand_abbreviations(tokens: &mut Vec<String>, max_tokens: usize) {
    let original_count = tokens.len();
    for index in 0..original_count {
        if tokens.len() >= max_tokens {
            break;
        }
        if let Some((_, expanded)) = ABBREVIATIONS
            .iter()
            .find(|(abbreviation, _)| *abbreviation == tokens[index])
        {
            tokens.push((*expanded).to_owned());
        }
    }
}

fn flush_token(buffer: &mut Vec<u8>, tokens: &mut Vec<String>, max_tokens: usize) {
    if !buffer.is_empty() && tokens.len() < max_tokens {
        // Only ASCII alphanumeric bytes enter the buffer.
        tokens.push(String::from_utf8(std::mem::take(buffer)).expect("ASCII token"));
    }
    buffer.clear();
}
