//! UTF-8 source lexer. Identifiers intentionally use an explicit ASCII grammar.
use crate::ast::{Span, Type};
use crate::diagnostic::Diagnostic;

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Int(u64, Option<Type>),
    Float(f64, Option<Type>),
    String(Vec<u8>, bool),
    Symbol(&'static str),
    Newline,
    Eof,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

pub fn lex(source: &str) -> Result<Vec<Token>, Diagnostic> {
    Lexer { source, cursor: 0 }
        .lex(false)
        .map(|(tokens, _)| tokens)
}

/// Recover at the next token after a lexical error, preserving byte offsets.
pub fn lex_recovering(source: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    Lexer { source, cursor: 0 }
        .lex(true)
        .expect("recovering lexer cannot fail")
}

struct Lexer<'a> {
    source: &'a str,
    cursor: usize,
}

impl Lexer<'_> {
    fn bytes(&self) -> &[u8] {
        self.source.as_bytes()
    }
    fn current(&self) -> Option<u8> {
        self.bytes().get(self.cursor).copied()
    }
    fn error(&self, start: usize, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(
            Span {
                start,
                end: self.cursor.max(start + 1).min(self.source.len()),
            },
            message,
        )
    }
    fn lex(mut self, recover: bool) -> Result<(Vec<Token>, Vec<Diagnostic>), Diagnostic> {
        let mut tokens = Vec::new();
        let mut diagnostics = Vec::new();
        while let Some(ch) = self.current() {
            let start = self.cursor;
            let kind = match ch {
                b' ' | b'\t' | b'\r' => {
                    self.cursor += 1;
                    continue;
                }
                b'\n' => {
                    self.cursor += 1;
                    TokenKind::Newline
                }
                b'/' if self.bytes().get(self.cursor + 1) == Some(&b'/') => {
                    while self.current().is_some_and(|c| c != b'\n') {
                        self.cursor += 1;
                    }
                    continue;
                }
                _ => {
                    let result = (|| {
                        Ok(match ch {
                            b'"' => TokenKind::String(self.string(false, b'"')?, false),
                            b'b' if matches!(
                                self.bytes().get(self.cursor + 1),
                                Some(b'"' | b'\'')
                            ) =>
                            {
                                self.cursor += 1;
                                let quote = self.current().unwrap_or(b'"');
                                let value = self.string(true, quote)?;
                                if quote == b'\'' {
                                    if value.len() != 1 {
                                        return Err(self.error(
                                            start,
                                            "a byte literal must contain exactly one byte",
                                        ));
                                    }
                                    TokenKind::Int(u64::from(value[0]), Some(Type::u8()))
                                } else {
                                    TokenKind::String(value, true)
                                }
                            }
                            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                                self.cursor += 1;
                                while self
                                    .current()
                                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
                                {
                                    self.cursor += 1;
                                }
                                TokenKind::Ident(self.source[start..self.cursor].to_owned())
                            }
                            b'0'..=b'9' => self.number()?,
                            _ => {
                                let rest = &self.source[self.cursor..];
                                let symbol = [
                                    "<<=", ">>=", "..=", ":=", "->", "=>", "==", "!=", "<=", ">=",
                                    "&&", "||", "<<", ">>", "+=", "-=", "*=", "/=", "%=", "&=",
                                    "|=", "^=", "::", "..", "{", "}", "(", ")", "[", "]", ",", ";",
                                    ":", ".", "+", "-", "*", "/", "%", "=", "<", ">", "!", "?",
                                    "&", "|", "^", "~", "@",
                                ]
                                .into_iter()
                                .find(|s| rest.starts_with(s));
                                if let Some(symbol) = symbol {
                                    self.cursor += symbol.len();
                                    TokenKind::Symbol(symbol)
                                } else {
                                    let invalid = rest.chars().next().unwrap_or('\0');
                                    self.cursor += invalid.len_utf8();
                                    return Err(self.error(start, format!("unexpected character {invalid:?}; identifiers use ASCII letters, digits, and underscores")));
                                }
                            }
                        })
                    })();
                    match result {
                        Ok(kind) => kind,
                        Err(error) if recover => {
                            diagnostics.push(error);
                            continue;
                        }
                        Err(error) => return Err(error),
                    }
                }
            };
            tokens.push(Token {
                kind,
                span: Span {
                    start,
                    end: self.cursor,
                },
            });
        }
        tokens.push(Token {
            kind: TokenKind::Eof,
            span: Span {
                start: self.cursor,
                end: self.cursor,
            },
        });
        Ok((tokens, diagnostics))
    }
    fn string(&mut self, byte: bool, quote: u8) -> Result<Vec<u8>, Diagnostic> {
        let start = self.cursor;
        self.cursor += 1;
        let mut result = Vec::new();
        loop {
            let Some(ch) = self.current() else {
                return Err(self.error(start, "unterminated string or byte literal"));
            };
            self.cursor += 1;
            match ch {
                c if c == quote => break,
                b'\n' | b'\r' => {
                    return Err(self.error(start, "literal cannot cross a newline; use \\n"));
                }
                b'\\' => {
                    let Some(escape) = self.current() else {
                        return Err(self.error(start, "unterminated escape sequence"));
                    };
                    self.cursor += 1;
                    match escape {
                        b'n' => result.push(b'\n'),
                        b'r' => result.push(b'\r'),
                        b't' => result.push(b'\t'),
                        b'0' => result.push(0),
                        b'\\' | b'"' | b'\'' => result.push(escape),
                        b'x' => {
                            let mut value = 0u8;
                            for _ in 0..2 {
                                let digit = self
                                    .current()
                                    .and_then(|b| (b as char).to_digit(16))
                                    .ok_or_else(|| {
                                    self.error(start, "\\x requires exactly two hexadecimal digits")
                                })?;
                                self.cursor += 1;
                                value = value * 16 + digit as u8;
                            }
                            result.push(value);
                        }
                        b'u' if !byte => {
                            if self.current() != Some(b'{') {
                                return Err(
                                    self.error(start, "Unicode escapes use \\u{hex_digits}")
                                );
                            }
                            self.cursor += 1;
                            let digits = self.cursor;
                            while self.current().is_some_and(|b| b.is_ascii_hexdigit()) {
                                self.cursor += 1;
                            }
                            let text = &self.source[digits..self.cursor];
                            let value = u32::from_str_radix(text, 16)
                                .ok()
                                .and_then(char::from_u32)
                                .filter(|_| !text.is_empty() && text.len() <= 6)
                                .ok_or_else(|| {
                                    self.error(start, "invalid Unicode scalar escape")
                                })?;
                            if self.current() != Some(b'}') {
                                return Err(self.error(start, "expected `}` after Unicode escape"));
                            }
                            self.cursor += 1;
                            let mut buffer = [0; 4];
                            result.extend_from_slice(value.encode_utf8(&mut buffer).as_bytes());
                        }
                        _ => {
                            return Err(self.error(
                                start,
                                format!("unknown escape sequence \\{}", escape as char),
                            ));
                        }
                    }
                }
                c if byte && !c.is_ascii() => {
                    return Err(self.error(
                        start,
                        "byte literals require ASCII source characters; use \\xHH for other bytes",
                    ));
                }
                c => result.push(c),
            }
        }
        if !byte && std::str::from_utf8(&result).is_err() {
            return Err(self.error(start, "string literal must contain valid UTF-8"));
        }
        Ok(result)
    }
    fn number(&mut self) -> Result<TokenKind, Diagnostic> {
        let start = self.cursor;
        let mut radix = 10;
        if self.current() == Some(b'0') {
            match self.bytes().get(self.cursor + 1) {
                Some(b'x') => radix = 16,
                Some(b'o') => radix = 8,
                Some(b'b') => radix = 2,
                _ => {}
            }
        }
        if radix != 10 {
            self.cursor += 2;
        }
        let digits_start = self.cursor;
        while self
            .current()
            .is_some_and(|c| c == b'_' || (c as char).is_digit(radix))
        {
            self.cursor += 1;
        }
        if self.cursor == digits_start
            || !self.bytes()[digits_start..self.cursor]
                .iter()
                .any(|b| *b != b'_')
        {
            return Err(self.error(start, "expected digits after numeric base prefix"));
        }
        let mut floating = false;
        if radix == 10
            && self.current() == Some(b'.')
            && self
                .bytes()
                .get(self.cursor + 1)
                .is_some_and(u8::is_ascii_digit)
        {
            floating = true;
            self.cursor += 1;
            while self
                .current()
                .is_some_and(|c| c.is_ascii_digit() || c == b'_')
            {
                self.cursor += 1;
            }
        }
        if radix == 10 && matches!(self.current(), Some(b'e' | b'E')) {
            floating = true;
            self.cursor += 1;
            if matches!(self.current(), Some(b'+' | b'-')) {
                self.cursor += 1;
            }
            let exponent_start = self.cursor;
            while self
                .current()
                .is_some_and(|c| c.is_ascii_digit() || c == b'_')
            {
                self.cursor += 1;
            }
            if !self.bytes()[exponent_start..self.cursor]
                .iter()
                .any(u8::is_ascii_digit)
            {
                return Err(self.error(start, "expected exponent digits"));
            }
        }
        let end = self.cursor;
        while self
            .current()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
            self.cursor += 1;
        }
        let suffix = &self.source[end..self.cursor];
        let ty = if suffix.is_empty() {
            None
        } else {
            Some(match suffix {
                "i8" => Type::Int {
                    signed: true,
                    bits: 8,
                },
                "i16" => Type::Int {
                    signed: true,
                    bits: 16,
                },
                "i32" => Type::Int {
                    signed: true,
                    bits: 32,
                },
                "i64" => Type::Int {
                    signed: true,
                    bits: 64,
                },
                "u8" => Type::u8(),
                "u16" => Type::Int {
                    signed: false,
                    bits: 16,
                },
                "u32" => Type::Int {
                    signed: false,
                    bits: 32,
                },
                "u64" => Type::Int {
                    signed: false,
                    bits: 64,
                },
                "isize" => Type::isize(),
                "usize" => Type::usize(),
                "f32" if radix == 10 => Type::Float(32),
                "f64" if radix == 10 => Type::Float(64),
                _ => {
                    return Err(
                        self.error(start, format!("invalid numeric suffix or digit `{suffix}`"))
                    );
                }
            })
        };
        let digits = self.source[digits_start..end].replace('_', "");
        if floating || matches!(ty, Some(Type::Float(_))) {
            if ty.as_ref().is_some_and(Type::is_integer) {
                return Err(self.error(
                    start,
                    "floating-point literals cannot have an integer suffix",
                ));
            }
            let value = digits
                .parse::<f64>()
                .map_err(|_| self.error(start, "invalid floating-point literal"))?;
            if !value.is_finite() {
                return Err(self.error(
                    start,
                    "floating-point literal is outside the finite f64 range",
                ));
            }
            Ok(TokenKind::Float(value, ty))
        } else {
            let value = u64::from_str_radix(&digits, radix)
                .map_err(|_| self.error(start, "integer literal is outside the u64 range"))?;
            Ok(TokenKind::Int(value, ty))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_literals_and_longest_operators() {
        let tokens = lex("0xffu8 0b10 0o77 1_000 1.25f32 2e3 <<= >> >=").unwrap();
        assert!(matches!(
            tokens[0].kind,
            TokenKind::Int(255, Some(Type::Int { bits: 8, .. }))
        ));
        assert!(matches!(tokens[1].kind, TokenKind::Int(2, None)));
        assert!(matches!(tokens[2].kind, TokenKind::Int(63, None)));
        assert!(matches!(tokens[3].kind, TokenKind::Int(1000, None)));
        assert!(matches!(
            tokens[4].kind,
            TokenKind::Float(1.25, Some(Type::Float(32)))
        ));
        assert!(matches!(tokens[5].kind, TokenKind::Float(2000.0, None)));
        assert_eq!(tokens[6].kind, TokenKind::Symbol("<<="));
    }
    #[test]
    fn utf8_and_byte_escapes() {
        let tokens = lex(r#""hé\u{1f426}" b"hi\xff" b'\n'"#).unwrap();
        assert_eq!(
            tokens[0].kind,
            TokenKind::String("hé🐦".as_bytes().to_vec(), false)
        );
        assert_eq!(
            tokens[1].kind,
            TokenKind::String(vec![b'h', b'i', 255], true)
        );
        assert_eq!(tokens[2].kind, TokenKind::Int(10, Some(Type::u8())));
    }
    #[test]
    fn inclusive_pattern_ranges_use_the_longest_operator() {
        let tokens = lex("b'0'..=b'9' 1..10 | 20..=30").unwrap();
        assert_eq!(tokens[1].kind, TokenKind::Symbol("..="));
        assert_eq!(tokens[4].kind, TokenKind::Symbol(".."));
        assert_eq!(tokens[6].kind, TokenKind::Symbol("|"));
        assert_eq!(tokens[8].kind, TokenKind::Symbol("..="));
    }
    #[test]
    fn comments_preserve_newlines_and_byte_spans() {
        let tokens = lex("x // hello\n\"é\"\n").unwrap();
        assert_eq!(tokens[1].kind, TokenKind::Newline);
        assert_eq!(tokens[2].span, Span { start: 11, end: 15 });
    }
    #[test]
    fn malformed_source_returns_diagnostics() {
        for source in [
            "18446744073709551616",
            "0x",
            "0b2",
            "12u7",
            "1e+",
            "\"x",
            "\"\\q\"",
            "b'ab'",
            "b'é'",
            "\"\\u{d800}\"",
            "λ",
        ] {
            assert!(lex(source).is_err(), "accepted {source:?}");
        }
    }
}
