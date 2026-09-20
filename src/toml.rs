//! Dependency-free, owned TOML 1.0 values. See <https://toml.io/en/v1.0.0>.
//! Locations are byte offsets; diagnostics render Unicode line/column positions.
//! Tables retain declaration state to reject redefinitions and inline extension.
use std::collections::BTreeMap;
use std::fmt;

const MAX_DEPTH: usize = 64;
#[derive(Clone, Debug, PartialEq)]
pub struct Value {
    pub kind: Kind,
    pub offset: usize,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    String(String),
    Integer(i64),
    Float(f64),
    Bool(bool),
    DateTime(String),
    Array(Vec<Value>),
    Table(Table),
}
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub entries: BTreeMap<String, Value>,
    declaration: Declaration,
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Declaration {
    Implicit,
    Explicit,
    Dotted,
    Inline,
    Array,
}
impl Table {
    fn new(declaration: Declaration) -> Self {
        Self {
            entries: BTreeMap::new(),
            declaration,
        }
    }
}
impl Value {
    fn table(offset: usize, declaration: Declaration) -> Self {
        Self {
            kind: Kind::Table(Table::new(declaration)),
            offset,
        }
    }
    pub fn as_table(&self) -> Option<&BTreeMap<String, Value>> {
        if let Kind::Table(table) = &self.kind {
            Some(&table.entries)
        } else {
            None
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub offset: usize,
    pub message: String,
}
impl Error {
    fn new(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
        }
    }
    pub fn render(&self, source: &str, path: &std::path::Path) -> String {
        let offset = self.offset.min(source.len());
        let before = &source[..offset];
        let line = before.bytes().filter(|b| *b == b'\n').count() + 1;
        let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
        format!("{}:{line}:{column}: {}", path.display(), self.message)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (byte {})", self.message, self.offset)
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;

pub fn parse(source: &str) -> Result<Value> {
    let mut parser = Parser { source, pos: 0 };
    let mut root = Value::table(0, Declaration::Explicit);
    let mut current = Vec::new();
    loop {
        parser.space_comments()?;
        if parser.peek().is_none() {
            return Ok(root);
        }
        if parser.eat('[') {
            let array = parser.eat('[');
            let offset = parser.pos - if array { 2 } else { 1 };
            let keys = parser.keys()?;
            parser.require(']')?;
            if array {
                parser.require(']')?;
            }
            declare(&mut root, &keys, array, offset)?;
            current = keys;
        } else {
            let keys = parser.keys()?;
            parser.require('=')?;
            parser.space();
            let value = parser.value(current.len() + keys.len())?;
            let table = context(&mut root, &current)?;
            insert(table, &keys, value)?;
        }
        parser.space();
        parser.comment()?;
        if parser.peek().is_some() && !parser.newline()? {
            return parser.fail("expected a newline after the statement");
        }
    }
}
fn table_mut(value: &mut Value) -> Result<&mut Table> {
    let offset = value.offset;
    match &mut value.kind {
        Kind::Table(t) if t.declaration != Declaration::Inline => Ok(t),
        Kind::Array(items)
            if items.last().is_some_and(
                |v| matches!(&v.kind, Kind::Table(t) if t.declaration == Declaration::Array),
            ) =>
        {
            table_mut(items.last_mut().unwrap())
        }
        _ => Err(Error::new(offset, "cannot extend a value or inline table")),
    }
}
fn context<'a>(value: &'a mut Value, keys: &[String]) -> Result<&'a mut Table> {
    let t = table_mut(value)?;
    if let Some((key, rest)) = keys.split_first() {
        context(t.entries.get_mut(key).expect("declared table"), rest)
    } else {
        Ok(t)
    }
}
fn declare(value: &mut Value, keys: &[String], array: bool, offset: usize) -> Result<()> {
    let (key, rest) = keys.split_first().unwrap();
    let t = table_mut(value).map_err(|e| Error::new(offset, e.message))?;
    if !rest.is_empty() {
        let next = t
            .entries
            .entry(key.clone())
            .or_insert_with(|| Value::table(offset, Declaration::Implicit));
        return declare(next, rest, array, offset);
    }
    if array {
        let next = t.entries.entry(key.clone()).or_insert_with(|| Value {
            kind: Kind::Array(vec![]),
            offset,
        });
        match &mut next.kind {
            Kind::Array(items) if items.is_empty() && next.offset == offset || items.last().is_some_and(|v| matches!(&v.kind, Kind::Table(t) if t.declaration == Declaration::Array)) => {
                items.push(Value::table(offset, Declaration::Array)); Ok(())
            }
            _ => Err(Error::new(offset, format!("cannot redefine '{key}' as an array of tables"))),
        }
    } else {
        let next = t
            .entries
            .entry(key.clone())
            .or_insert_with(|| Value::table(offset, Declaration::Implicit));
        match &mut next.kind {
            Kind::Table(t) if t.declaration == Declaration::Implicit => {
                t.declaration = Declaration::Explicit;
                Ok(())
            }
            _ => Err(Error::new(
                offset,
                format!("table '{key}' is already defined"),
            )),
        }
    }
}
fn insert(table: &mut Table, keys: &[String], value: Value) -> Result<()> {
    let (key, rest) = keys.split_first().unwrap();
    if rest.is_empty() {
        if table.entries.contains_key(key) {
            return Err(Error::new(value.offset, format!("duplicate key '{key}'")));
        }
        table.entries.insert(key.clone(), value);
        Ok(())
    } else {
        let next = table
            .entries
            .entry(key.clone())
            .or_insert_with(|| Value::table(value.offset, Declaration::Dotted));
        let t = match &mut next.kind {
            Kind::Table(t)
                if matches!(t.declaration, Declaration::Dotted | Declaration::Implicit) =>
            {
                t
            }
            _ => {
                return Err(Error::new(
                    value.offset,
                    format!("cannot redefine or extend '{key}' with a dotted key"),
                ));
            }
        };
        t.declaration = Declaration::Dotted;
        insert(t, rest, value)
    }
}
struct Parser<'a> {
    source: &'a str,
    pos: usize,
}
impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.source[self.pos..].chars().next()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }
    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn fail<T>(&self, text: &str) -> Result<T> {
        Err(Error::new(self.pos, text))
    }
    fn require(&mut self, c: char) -> Result<()> {
        if self.eat(c) {
            Ok(())
        } else {
            self.fail(&format!("expected '{c}'"))
        }
    }
    fn space(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.bump();
        }
    }
    fn newline(&mut self) -> Result<bool> {
        if self.eat('\r') {
            self.require('\n')?;
            Ok(true)
        } else {
            Ok(self.eat('\n'))
        }
    }
    fn comment(&mut self) -> Result<()> {
        if self.eat('#') {
            while let Some(c) = self.peek() {
                if matches!(c, '\n' | '\r') {
                    break;
                }
                if c.is_ascii_control() && c != '\t' {
                    return self.fail("control character in comment");
                }
                self.bump();
            }
        }
        Ok(())
    }
    fn space_comments(&mut self) -> Result<()> {
        loop {
            self.space();
            self.comment()?;
            if !self.newline()? {
                return Ok(());
            }
        }
    }
    fn keys(&mut self) -> Result<Vec<String>> {
        let mut keys = vec![];
        loop {
            self.space();
            let key = if matches!(self.peek(), Some('\'' | '"')) {
                self.string(false)?
            } else {
                let start = self.pos;
                while self
                    .peek()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
                {
                    self.bump();
                }
                if start == self.pos {
                    return self.fail("expected a bare or quoted key");
                }
                self.source[start..self.pos].to_owned()
            };
            keys.push(key);
            if keys.len() > MAX_DEPTH {
                return self.fail("TOML key nesting exceeds 64 levels");
            }
            self.space();
            if !self.eat('.') {
                return Ok(keys);
            }
        }
    }
    fn string(&mut self, allow_multi: bool) -> Result<String> {
        let quote = self.bump().unwrap();
        let multi =
            allow_multi && self.source[self.pos..].starts_with(&quote.to_string().repeat(2));
        if multi {
            self.pos += 2;
            self.newline()?;
        }
        let mut out = String::new();
        loop {
            let Some(c) = self.peek() else {
                return self.fail("unterminated string");
            };
            if c == quote {
                let count = self.source[self.pos..]
                    .chars()
                    .take_while(|c| *c == quote)
                    .count();
                if !multi {
                    self.bump();
                    return Ok(out);
                }
                if count >= 3 {
                    if count > 5 {
                        return self.fail("too many closing quotes");
                    }
                    self.pos += count;
                    out.extend(std::iter::repeat_n(quote, count - 3));
                    return Ok(out);
                }
                self.bump();
                out.push(c);
                continue;
            }
            if c == '\\' && quote == '"' {
                self.bump();
                if multi && matches!(self.peek(), Some(' ' | '\t' | '\r' | '\n')) {
                    self.space();
                    if !self.newline()? {
                        return self.fail("string continuation requires a newline");
                    }
                    loop {
                        self.space();
                        if !self.newline()? {
                            break;
                        }
                    }
                    continue;
                }
                match self.bump() {
                    Some('b') => out.push('\u{8}'),
                    Some('t') => out.push('\t'),
                    Some('n') => out.push('\n'),
                    Some('f') => out.push('\u{c}'),
                    Some('r') => out.push('\r'),
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some(escape @ ('u' | 'U')) => {
                        let n = if escape == 'u' { 4 } else { 8 };
                        let mut code = 0;
                        for _ in 0..n {
                            let Some(digit) = self
                                .peek()
                                .and_then(|c| c.to_digit(16).filter(|_| c.is_ascii()))
                            else {
                                return self.fail("expected Unicode escape hex digits");
                            };
                            self.bump();
                            code = code * 16 + digit;
                        }
                        let Some(c) = char::from_u32(code) else {
                            return self.fail("invalid Unicode scalar value");
                        };
                        out.push(c);
                    }
                    _ => return self.fail("invalid string escape"),
                }
            } else if matches!(c, '\r' | '\n') && multi {
                self.newline()?;
                out.push('\n');
            } else {
                if c.is_ascii_control() && c != '\t' {
                    return self.fail("unescaped control character in string");
                }
                self.bump();
                out.push(c);
            }
        }
    }
    fn value(&mut self, depth: usize) -> Result<Value> {
        if depth > MAX_DEPTH {
            return self.fail("TOML value nesting exceeds 64 levels");
        }
        let offset = self.pos;
        let kind = match self.peek() {
            Some('"' | '\'') => Kind::String(self.string(true)?),
            Some('[') => {
                self.bump();
                let mut values = vec![];
                self.space_comments()?;
                while !self.eat(']') {
                    values.push(self.value(depth + 1)?);
                    self.space_comments()?;
                    if self.eat(']') {
                        break;
                    }
                    self.require(',')?;
                    self.space_comments()?;
                }
                Kind::Array(values)
            }
            Some('{') => {
                self.bump();
                let mut table = Table::new(Declaration::Inline);
                self.space();
                if !self.eat('}') {
                    loop {
                        let keys = self.keys()?;
                        self.require('=')?;
                        self.space();
                        let value = self.value(depth + keys.len())?;
                        insert(&mut table, &keys, value)?;
                        self.space();
                        if self.eat('}') {
                            break;
                        }
                        self.require(',')?;
                        self.space();
                        if self.peek() == Some('}') {
                            return self.fail("trailing comma in inline table");
                        }
                    }
                }
                Kind::Table(table)
            }
            _ => {
                while let Some(c) = self.peek() {
                    if matches!(c, ' ' | '\t' | '\r' | '\n' | '#' | ',' | ']' | '}') {
                        break;
                    }
                    self.bump();
                }
                // A single space may separate the date and time.
                if self.pos - offset == 10
                    && self.peek() == Some(' ')
                    && self.source.as_bytes()[offset..self.pos].get(4) == Some(&b'-')
                    && self
                        .source
                        .as_bytes()
                        .get(self.pos + 1)
                        .is_some_and(u8::is_ascii_digit)
                {
                    self.bump();
                    while self.peek().is_some_and(|c| {
                        !matches!(c, ' ' | '\t' | '\r' | '\n' | '#' | ',' | ']' | '}')
                    }) {
                        self.bump();
                    }
                }
                let token = &self.source[offset..self.pos];
                match token {
                    "true" => Kind::Bool(true),
                    "false" => Kind::Bool(false),
                    _ if datetime(token) => Kind::DateTime(token.to_owned()),
                    _ => number(token).ok_or_else(|| {
                        Error::new(offset, format!("invalid TOML value '{token}'"))
                    })?,
                }
            }
        };
        Ok(Value { kind, offset })
    }
}
fn digits(s: &str, radix: u32) -> bool {
    !s.is_empty()
        && s.chars().enumerate().all(|(i, c)| {
            if c == '_' {
                i > 0
                    && i + 1 < s.len()
                    && s.as_bytes()[i - 1] != b'_'
                    && s.as_bytes()[i + 1] != b'_'
            } else {
                c.is_ascii() && c.is_digit(radix)
            }
        })
}
fn decimal(s: &str) -> bool {
    digits(s, 10) && (s.len() == 1 || !s.starts_with('0'))
}
fn number(token: &str) -> Option<Kind> {
    match token {
        "inf" | "+inf" => return Some(Kind::Float(f64::INFINITY)),
        "-inf" => return Some(Kind::Float(f64::NEG_INFINITY)),
        "nan" | "+nan" | "-nan" => return Some(Kind::Float(f64::NAN)),
        _ => (),
    }
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(n) = token.strip_prefix(prefix) {
            return digits(n, radix)
                .then(|| {
                    i64::from_str_radix(&n.replace('_', ""), radix)
                        .ok()
                        .map(Kind::Integer)
                })
                .flatten();
        }
    }
    let unsigned = token.strip_prefix(['+', '-']).unwrap_or(token);
    let (mantissa, exponent) = unsigned
        .split_once(['e', 'E'])
        .map_or((unsigned, None), |(a, b)| (a, Some(b)));
    if exponent.is_some_and(|s| !digits(s.strip_prefix(['+', '-']).unwrap_or(s), 10)) {
        return None;
    }
    let (whole, fraction) = mantissa
        .split_once('.')
        .map_or((mantissa, None), |(a, b)| (a, Some(b)));
    if !decimal(whole) || fraction.is_some_and(|s| !digits(s, 10)) {
        return None;
    }
    let clean = token.replace('_', "");
    if fraction.is_some() || exponent.is_some() {
        clean.parse().ok().map(Kind::Float)
    } else {
        clean.parse().ok().map(Kind::Integer)
    }
}
fn datetime(s: &str) -> bool {
    fn part(bytes: &[u8], start: usize, n: usize) -> Option<u32> {
        let b = bytes.get(start..start + n)?;
        if !b.iter().all(u8::is_ascii_digit) {
            return None;
        }
        Some(b.iter().fold(0, |a, b| a * 10 + u32::from(b - b'0')))
    }
    let b = s.as_bytes();
    let mut i = 0;
    if b.get(4) == Some(&b'-') {
        let Some(year) = part(b, 0, 4) else {
            return false;
        };
        let Some(month) = part(b, 5, 2) else {
            return false;
        };
        let Some(day) = part(b, 8, 2) else {
            return false;
        };
        if b.get(7) != Some(&b'-') || !(1..=12).contains(&month) {
            return false;
        }
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days = [
            31,
            if leap { 29 } else { 28 },
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ][month as usize - 1];
        if day == 0 || day > days {
            return false;
        }
        if b.len() == 10 {
            return true;
        }
        if !matches!(b.get(10), Some(b'T' | b't' | b' ')) {
            return false;
        }
        i = 11;
    }
    if part(b, i, 2).is_none_or(|x| x > 23)
        || b.get(i + 2) != Some(&b':')
        || part(b, i + 3, 2).is_none_or(|x| x > 59)
        || b.get(i + 5) != Some(&b':')
        || part(b, i + 6, 2).is_none_or(|x| x > 59)
    {
        return false;
    }
    let date = i != 0;
    i += 8;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    if i == b.len() {
        return true;
    }
    if !date {
        return false;
    }
    if matches!(b.get(i), Some(b'Z' | b'z')) {
        return i + 1 == b.len();
    }
    matches!(b.get(i), Some(b'+' | b'-'))
        && b.len() == i + 6
        && part(b, i + 1, 2).is_some_and(|x| x <= 23)
        && b.get(i + 3) == Some(&b':')
        && part(b, i + 4, 2).is_some_and(|x| x <= 59)
}
/// Encode a TOML basic string, including controls that Rust's Debug escapes differently.
pub fn quote(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_ascii_control() => {
                use fmt::Write;
                write!(out, "\\u{:04X}", c as u32).unwrap();
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
