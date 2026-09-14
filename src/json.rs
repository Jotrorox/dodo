//! JSON values and UTF-8 encoding for the editor protocol, using only `std`.
//!
//! The grammar follows RFC 8259: <https://www.rfc-editor.org/rfc/rfc8259>.
//! Parsing requires a complete document, rejects invalid Unicode and non-finite
//! numbers, and allows at most 128 nested containers. Duplicate object keys use
//! the last value. Objects serialize in sorted key order for stable diagnostics.
//! Integers retain all 64 bits; fractions, exponents, negative zero, and integers
//! outside the 64-bit range use finite `f64` values. Float syntax stays distinct
//! from integer syntax because LSP requires integer request IDs and positions.
//!
//! This is a value-tree codec, not a general Rust serialization framework.
use std::collections::BTreeMap;
use std::fmt::{self, Write};
use std::ops::{Index, IndexMut};

const MAX_DEPTH: usize = 128;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

/// The private representation ensures that numbers are always finite.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Number(NumberRepr);

#[derive(Clone, Copy, Debug, PartialEq)]
enum NumberRepr {
    Negative(i64),
    Unsigned(u64),
    Float(f64),
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }

    pub fn is_object(&self) -> bool {
        matches!(self, Self::Object(_))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Number(Number(NumberRepr::Negative(value))) => Some(*value),
            Self::Number(Number(NumberRepr::Unsigned(value))) => i64::try_from(*value).ok(),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Number(Number(NumberRepr::Unsigned(value))) => Some(*value),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(Number(NumberRepr::Negative(value))) => Some(*value as f64),
            Self::Number(Number(NumberRepr::Unsigned(value))) => Some(*value as f64),
            Self::Number(Number(NumberRepr::Float(value))) => Some(*value),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Self::Array(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Self::Object(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut BTreeMap<String, Value>> {
        match self {
            Self::Object(value) => Some(value),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_object()?.get(key)
    }
}

// Missing fields and out-of-bounds reads return null, allowing protocol parsers
// to validate nested fields through their typed accessors without panicking.
impl Index<&str> for Value {
    type Output = Value;

    fn index(&self, key: &str) -> &Value {
        self.get(key).unwrap_or(&Value::Null)
    }
}

impl IndexMut<&str> for Value {
    fn index_mut(&mut self, key: &str) -> &mut Value {
        if self.is_null() {
            *self = Self::Object(BTreeMap::new());
        }
        self.as_object_mut()
            .expect("JSON field assignment requires an object")
            .entry(key.to_owned())
            .or_default()
    }
}

impl Index<&String> for Value {
    type Output = Value;

    fn index(&self, key: &String) -> &Value {
        &self[key.as_str()]
    }
}

impl IndexMut<&String> for Value {
    fn index_mut(&mut self, key: &String) -> &mut Value {
        &mut self[key.as_str()]
    }
}

impl Index<usize> for Value {
    type Output = Value;

    fn index(&self, index: usize) -> &Value {
        self.as_array()
            .and_then(|array| array.get(index))
            .unwrap_or(&Value::Null)
    }
}

impl IndexMut<usize> for Value {
    fn index_mut(&mut self, index: usize) -> &mut Value {
        match self {
            Self::Array(array) => &mut array[index],
            _ => panic!("JSON element assignment requires an array"),
        }
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl<T: Clone> From<&T> for Value
where
    Value: From<T>,
{
    fn from(value: &T) -> Self {
        Self::from(value.clone())
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(value: Vec<T>) -> Self {
        Self::Array(value.into_iter().map(Into::into).collect())
    }
}

impl<T: Into<Value>> From<BTreeMap<String, T>> for Value {
    fn from(value: BTreeMap<String, T>) -> Self {
        Self::Object(value.into_iter().map(|(k, v)| (k, v.into())).collect())
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(value: Option<T>) -> Self {
        value.map(Into::into).unwrap_or(Self::Null)
    }
}

macro_rules! integer_values {
    (signed: $($signed:ty),*; unsigned: $($unsigned:ty),*) => {
        $(impl From<$signed> for Value {
            fn from(value: $signed) -> Self {
                if value < 0 {
                    Self::Number(Number(NumberRepr::Negative(value as i64)))
                } else {
                    Self::Number(Number(NumberRepr::Unsigned(value as u64)))
                }
            }
        }
        impl PartialEq<$signed> for Value {
            fn eq(&self, value: &$signed) -> bool {
                self.as_i64() == Some(*value as i64)
            }
        })*
        $(impl From<$unsigned> for Value {
            fn from(value: $unsigned) -> Self {
                Self::Number(Number(NumberRepr::Unsigned(value as u64)))
            }
        }
        impl PartialEq<$unsigned> for Value {
            fn eq(&self, value: &$unsigned) -> bool {
                self.as_u64() == Some(*value as u64)
            }
        })*
    };
}

integer_values!(signed: i8, i16, i32, i64, isize; unsigned: u8, u16, u32, u64, usize);

/// Non-finite Rust floats become null; JSON has no NaN or infinity literals.
impl From<f64> for Value {
    fn from(value: f64) -> Self {
        if value.is_finite() {
            Self::Number(Number(NumberRepr::Float(value)))
        } else {
            Self::Null
        }
    }
}

impl PartialEq<&str> for Value {
    fn eq(&self, value: &&str) -> bool {
        self.as_str() == Some(*value)
    }
}

impl PartialEq<String> for Value {
    fn eq(&self, value: &String) -> bool {
        self.as_str() == Some(value.as_str())
    }
}

impl PartialEq<bool> for Value {
    fn eq(&self, value: &bool) -> bool {
        self.as_bool() == Some(*value)
    }
}

/// Construct a value from nested JSON syntax and borrowed Rust expressions.
/// Object keys are string literals or parenthesized string expressions.
/// Arrays and objects accept a trailing comma. Expressions are evaluated once.
#[macro_export]
macro_rules! json {
    (@array [$($values:expr,)*];) => { ::std::vec![$($values),*] };
    (@array [$($values:expr,)*]; null $(, $($rest:tt)*)?) => {
        $crate::json!(@array [$($values,)* $crate::json::Value::Null,]; $($($rest)*)?)
    };
    (@array [$($values:expr,)*]; [$($items:tt)*] $(, $($rest:tt)*)?) => {
        $crate::json!(@array [$($values,)* $crate::json!([$($items)*]),]; $($($rest)*)?)
    };
    (@array [$($values:expr,)*]; {$($fields:tt)*} $(, $($rest:tt)*)?) => {
        $crate::json!(@array [$($values,)* $crate::json!({$($fields)*}),]; $($($rest)*)?)
    };
    (@array [$($values:expr,)*]; $value:expr $(, $($rest:tt)*)?) => {
        $crate::json!(@array [$($values,)* $crate::json!($value),]; $($($rest)*)?)
    };
    (@object $object:ident;) => {};
    (@object $object:ident; $key:tt : null $(, $($rest:tt)*)?) => {
        $object.insert(($key).to_string(), $crate::json::Value::Null);
        $crate::json!(@object $object; $($($rest)*)?);
    };
    (@object $object:ident; $key:tt : [$($items:tt)*] $(, $($rest:tt)*)?) => {
        $object.insert(($key).to_string(), $crate::json!([$($items)*]));
        $crate::json!(@object $object; $($($rest)*)?);
    };
    (@object $object:ident; $key:tt : {$($fields:tt)*} $(, $($rest:tt)*)?) => {
        $object.insert(($key).to_string(), $crate::json!({$($fields)*}));
        $crate::json!(@object $object; $($($rest)*)?);
    };
    (@object $object:ident; $key:tt : $value:expr $(, $($rest:tt)*)?) => {
        $object.insert(($key).to_string(), $crate::json!($value));
        $crate::json!(@object $object; $($($rest)*)?);
    };
    (null) => { $crate::json::Value::Null };
    ([]) => { $crate::json::Value::Array(::std::vec::Vec::new()) };
    ([$($items:tt)+]) => {
        $crate::json::Value::Array($crate::json!(@array []; $($items)+))
    };
    ({}) => { $crate::json::Value::Object(::std::collections::BTreeMap::new()) };
    ({$($fields:tt)+}) => {{
        let mut object = ::std::collections::BTreeMap::new();
        $crate::json!(@object object; $($fields)+);
        $crate::json::Value::Object(object)
    }};
    ($value:expr) => { $crate::json::Value::from(&$value) };
}

pub use crate::json;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// Zero-based byte offset in the input, including any leading whitespace.
    pub offset: usize,
    pub message: &'static str,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for Error {}

/// Parse exactly one JSON value, with optional surrounding JSON whitespace.
pub fn from_slice(input: &[u8]) -> Result<Value, Error> {
    let text = std::str::from_utf8(input).map_err(|error| Error {
        offset: error.valid_up_to(),
        message: "invalid UTF-8",
    })?;
    from_str(text)
}

pub fn from_str(input: &str) -> Result<Value, Error> {
    let mut parser = Parser { input, offset: 0 };
    let value = parser.value(0)?;
    parser.whitespace();
    if parser.offset != input.len() {
        return Err(parser.error("trailing characters"));
    }
    Ok(value)
}

struct Parser<'a> {
    input: &'a str,
    offset: usize,
}

impl Parser<'_> {
    fn error(&self, message: &'static str) -> Error {
        Error {
            offset: self.offset,
            message,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.offset).copied()
    }

    fn consume(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.offset += 1;
            true
        } else {
            false
        }
    }

    fn whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.offset += 1;
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, Error> {
        self.whitespace();
        match self.peek() {
            Some(b'n') => self.literal("null", Value::Null),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'"') => self.string().map(Value::String),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(b'[' | b'{') if depth >= MAX_DEPTH => {
                Err(self.error("JSON nesting limit exceeded"))
            }
            Some(b'[') => self.array(depth + 1),
            Some(b'{') => self.object(depth + 1),
            _ => Err(self.error("expected a JSON value")),
        }
    }

    fn literal(&mut self, text: &str, value: Value) -> Result<Value, Error> {
        if !self.input[self.offset..].starts_with(text) {
            return Err(self.error("invalid literal"));
        }
        self.offset += text.len();
        Ok(value)
    }

    fn array(&mut self, depth: usize) -> Result<Value, Error> {
        self.offset += 1;
        self.whitespace();
        let mut values = Vec::new();
        if self.consume(b']') {
            return Ok(Value::Array(values));
        }
        loop {
            values.push(self.value(depth)?);
            self.whitespace();
            if self.consume(b']') {
                return Ok(Value::Array(values));
            }
            if !self.consume(b',') {
                return Err(self.error("expected ',' or ']'"));
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, Error> {
        self.offset += 1;
        self.whitespace();
        let mut values = BTreeMap::new();
        if self.consume(b'}') {
            return Ok(Value::Object(values));
        }
        loop {
            self.whitespace();
            let key = self.string()?;
            self.whitespace();
            if !self.consume(b':') {
                return Err(self.error("expected ':'"));
            }
            values.insert(key, self.value(depth)?);
            self.whitespace();
            if self.consume(b'}') {
                return Ok(Value::Object(values));
            }
            if !self.consume(b',') {
                return Err(self.error("expected ',' or '}'"));
            }
        }
    }

    fn string(&mut self) -> Result<String, Error> {
        if !self.consume(b'"') {
            return Err(self.error("expected a string"));
        }
        let mut result = String::new();
        let mut start = self.offset;
        loop {
            match self.peek() {
                Some(b'"') => {
                    result.push_str(&self.input[start..self.offset]);
                    self.offset += 1;
                    return Ok(result);
                }
                Some(b'\\') => {
                    result.push_str(&self.input[start..self.offset]);
                    self.offset += 1;
                    let escaped = self.peek().ok_or_else(|| self.error("incomplete escape"))?;
                    self.offset += 1;
                    result.push(match escaped {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{08}',
                        b'f' => '\u{0c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => self.unicode_escape()?,
                        _ => return Err(self.error("invalid escape")),
                    });
                    start = self.offset;
                }
                Some(0..=0x1f) => return Err(self.error("unescaped control character")),
                // UTF-8 was validated before parsing; ASCII delimiters always
                // occur at character boundaries, so copying these spans is safe.
                Some(_) => self.offset += 1,
                None => return Err(self.error("unterminated string")),
            }
        }
    }

    fn hex_quad(&mut self) -> Result<u32, Error> {
        let mut value = 0;
        for _ in 0..4 {
            let digit = match self.peek() {
                Some(b'0'..=b'9') => self.peek().unwrap() - b'0',
                Some(b'a'..=b'f') => self.peek().unwrap() - b'a' + 10,
                Some(b'A'..=b'F') => self.peek().unwrap() - b'A' + 10,
                _ => return Err(self.error("expected four hexadecimal digits")),
            };
            self.offset += 1;
            value = value * 16 + u32::from(digit);
        }
        Ok(value)
    }

    fn unicode_escape(&mut self) -> Result<char, Error> {
        let mut value = self.hex_quad()?;
        if (0xd800..=0xdbff).contains(&value) {
            if !self.consume(b'\\') || !self.consume(b'u') {
                return Err(self.error("expected a low surrogate"));
            }
            let low = self.hex_quad()?;
            if !(0xdc00..=0xdfff).contains(&low) {
                return Err(self.error("expected a low surrogate"));
            }
            value = 0x10000 + ((value - 0xd800) << 10) + (low - 0xdc00);
        }
        char::from_u32(value).ok_or_else(|| self.error("unpaired low surrogate"))
    }

    fn digits(&mut self) -> Result<(), Error> {
        let start = self.offset;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.offset += 1;
        }
        if self.offset == start {
            return Err(self.error("expected a digit"));
        }
        Ok(())
    }

    fn number(&mut self) -> Result<Value, Error> {
        let start = self.offset;
        let negative = self.consume(b'-');
        if !self.consume(b'0') {
            self.digits()?;
        }
        let mut float = false;
        if self.consume(b'.') {
            float = true;
            self.digits()?;
        }
        if self.consume(b'e') || self.consume(b'E') {
            float = true;
            if !self.consume(b'+') {
                self.consume(b'-');
            }
            self.digits()?;
        }
        let text = &self.input[start..self.offset];
        if !float && text != "-0" {
            if negative {
                if let Ok(value) = text.parse::<i64>() {
                    return Ok(Value::from(value));
                }
            } else if let Ok(value) = text.parse::<u64>() {
                return Ok(Value::from(value));
            }
        }
        let value: f64 = text.parse().map_err(|_| self.error("invalid number"))?;
        if !value.is_finite() {
            return Err(self.error("number out of range"));
        }
        Ok(Value::from(value))
    }
}

/// Encode compact JSON. Every constructible value has a valid JSON encoding.
pub fn to_vec(value: &Value) -> Vec<u8> {
    value.to_string().into_bytes()
}

fn write_string(f: &mut fmt::Formatter<'_>, text: &str) -> fmt::Result {
    f.write_char('"')?;
    let mut start = 0;
    for (offset, byte) in text.bytes().enumerate() {
        let escape = match byte {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            8 => "\\b",
            12 => "\\f",
            0..=0x1f => "",
            _ => continue,
        };
        f.write_str(&text[start..offset])?;
        if escape.is_empty() {
            write!(f, "\\u{byte:04x}")?;
        } else {
            f.write_str(escape)?;
        }
        start = offset + 1;
    }
    f.write_str(&text[start..])?;
    f.write_char('"')
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => f.write_str("null"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::Number(Number(NumberRepr::Negative(value))) => write!(f, "{value}"),
            Self::Number(Number(NumberRepr::Unsigned(value))) => write!(f, "{value}"),
            // Debug's shortest round-tripping float format retains `.0` or an
            // exponent, so a float request ID cannot turn into a valid integer.
            Self::Number(Number(NumberRepr::Float(value))) => write!(f, "{value:?}"),
            Self::String(value) => write_string(f, value),
            Self::Array(values) => {
                f.write_char('[')?;
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        f.write_char(',')?;
                    }
                    write!(f, "{value}")?;
                }
                f.write_char(']')
            }
            Self::Object(values) => {
                f.write_char('{')?;
                for (index, (key, value)) in values.iter().enumerate() {
                    if index != 0 {
                        f.write_char(',')?;
                    }
                    write_string(f, key)?;
                    write!(f, ":{value}")?;
                }
                f.write_char('}')
            }
        }
    }
}
