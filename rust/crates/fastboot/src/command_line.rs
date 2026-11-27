//! FastbootCommandLine.kt tokenization. Keep both exact UTF-16 literals and
//! unescaped values without converting Android paths or OEM arguments to ASCII.
#[derive(Debug, PartialEq, Eq)]
pub struct Token<'a> {
    pub literal: &'a [u16],
    pub value: Vec<u16>,
}

fn whitespace(ch: u16) -> bool {
    // Kotlin Char.isWhitespace = Character.isWhitespace || Character.isSpaceChar.
    matches!(ch,9..=13 | 0x1c..=0x20 | 0xa0 | 0x1680 | 0x2000..=0x200a | 0x2028..=0x2029 | 0x202f | 0x205f | 0x3000)
}

#[derive(Debug, PartialEq, Eq)]
pub struct UnclosedQuoteOrEscape;

pub fn tokenize(input: &[u16]) -> Result<Vec<Token<'_>>, UnclosedQuoteOrEscape> {
    let mut tokens = Vec::new();
    let mut value = Vec::new();
    let mut start = None;
    let mut quote = None;
    let mut escaped = false;
    for (i, &ch) in input.iter().enumerate() {
        if !whitespace(ch) {
            start.get_or_insert(i);
        }
        if escaped {
            value.push(ch);
            escaped = false;
        } else if ch == 92 {
            escaped = true;
        } else if let Some(q) = quote {
            if ch == q {
                quote = None;
            } else {
                value.push(ch);
            }
        } else if matches!(ch, 34 | 39) {
            quote = Some(ch);
        } else if whitespace(ch) {
            if let Some(n) = start.take() {
                tokens.push(Token {
                    literal: &input[n..i],
                    value: std::mem::take(&mut value),
                });
            }
        } else {
            value.push(ch);
        }
    }
    if quote.is_some() || escaped {
        return Err(UnclosedQuoteOrEscape);
    }
    if let Some(n) = start {
        tokens.push(Token {
            literal: &input[n..],
            value,
        });
    }
    Ok(tokens)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn android_literals_including_empty_quotes_escapes_and_surrogates() {
        let mut text: Vec<_> = "oem  '' \u{a0}\"две части\" a\\ b "
            .encode_utf16()
            .collect();
        text.push(0xd800);
        let t = tokenize(&text).unwrap();
        assert_eq!(t.len(), 5);
        assert!(t[1].value.is_empty());
        assert_eq!(t[1].literal, [39, 39]);
        assert_eq!(String::from_utf16(&t[2].value).unwrap(), "две части");
        assert_eq!(String::from_utf16(&t[3].value).unwrap(), "a b");
        assert_eq!(t[4].value, [0xd800]);
        for raw in ["oem 'oops", "oem a\\"] {
            assert!(tokenize(&raw.encode_utf16().collect::<Vec<_>>()).is_err());
        }
    }
}
