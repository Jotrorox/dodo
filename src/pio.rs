//! The PIO assembler behind `rp_pio.assemble`: programs for the programmable
//! I/O blocks of the RP2040 and RP2350, assembled at compile time into the
//! instruction words and state machine settings that
//! `std/embedded/chip/rp_pio` loads.
//!
//! The language is that of the Pico SDK's pioasm (RP2040 datasheet 3.3 and
//! 3.4, RP2350 datasheet 11.3 and 11.4), for one program: labels, `.define`,
//! expressions, side-set and delay, every instruction, and the directives
//! that configure a state machine (`.side_set`, `.wrap_target`, `.wrap`,
//! `.origin`, `.pio_version`, `.clock_div`, `.fifo`, `.mov_status`, `.in`,
//! `.out`, `.set`). Keywords are case-insensitive, symbols are not. `.word`
//! adds a raw instruction; `.lang_opt` and `% name { ... %}` code blocks are
//! accepted and ignored, so `.pio` files from the SDK assemble unchanged.
//!
//! Differences from pioasm: a program may refer to symbols before their
//! definition anywhere, `.fifo` and `.side_set` apply to the whole program
//! wherever they stand (`.side_set` must still come before the first
//! instruction), `.word` is allowed in programs with a non-optional
//! side-set, and the PIO version defaults to the chip's rather than 0.
use std::collections::{HashMap, HashSet};

/// Instruction memory holds 32 instructions per PIO block.
pub const MAX_INSTRUCTIONS: usize = 32;

/// An assembled program. JMP targets are relative to the first instruction;
/// the loader adds the address it places the program at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub instructions: Vec<u16>,
    /// The address `.origin` fixes, if any.
    pub origin: Option<u32>,
    pub wrap_target: u32,
    pub wrap: u32,
    pub side_set: Option<SideSet>,
    /// The lowest PIO version that runs the program: 1 if it uses an RP2350
    /// instruction or setting, otherwise 0.
    pub version: u8,
    /// `.clock_div`, in 256ths: 256 runs at the system clock.
    pub clock_divider: u32,
    pub fifo: Fifo,
    pub status: Status,
    /// `.in`: the pins IN reads unmasked (32 for all), and input shifting.
    pub input: Shift,
    /// `.out`: the pins OUT drives (0 unless given), and output shifting.
    pub output: Shift,
    /// `.set`: the pins SET drives, 0 unless given.
    pub set_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SideSet {
    /// Side-set value bits, without the enable bit of an optional side-set.
    pub bits: u32,
    pub optional: bool,
    /// Side-set drives pin directions instead of levels.
    pub pindirs: bool,
}

impl SideSet {
    /// The delay/side-set bits it takes: PINCTRL.SIDESET_COUNT.
    pub fn count(self) -> u32 {
        self.bits + u32::from(self.optional)
    }
}

/// `.fifo`: how the TX and RX FIFOs are joined. The last three, RP2350
/// only, turn the RX FIFO into registers the program reads or writes with
/// `mov rxfifo[...]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fifo {
    TxRx = 0,
    Tx = 1,
    Rx = 2,
    TxPut = 3,
    TxGet = 4,
    PutGet = 5,
}

/// `.mov_status`: what `mov x, status` reads as all ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    /// EXECCTRL.STATUS_SEL: 0 for TX level below `n`, 1 for RX level below
    /// `n`, 2 for IRQ flag `n` set (RP2350).
    pub select: u32,
    /// EXECCTRL.STATUS_N; for IRQ flags, 8 + flag for the previous block's
    /// and 16 + flag for the next block's.
    pub n: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shift {
    pub count: u32,
    pub right: bool,
    pub auto: bool,
    pub threshold: u32,
}

/// An assembly error at a line of the source, with byte columns `start` to
/// `end` within it, or at line `usize::MAX` for the program as a whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub message: String,
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pos {
    line: usize,
    start: usize,
    end: usize,
}

impl Pos {
    fn to(self, other: Pos) -> Pos {
        if other.line == self.line {
            Pos {
                end: other.end.max(self.end),
                ..self
            }
        } else {
            self
        }
    }
    fn error<T>(self, message: impl Into<String>) -> Result<T> {
        Err(Error {
            line: self.line,
            start: self.start,
            end: self.end,
            message: message.into(),
        })
    }
}

/// Assemble `source` for a chip with PIO version `chip_version`: 0 for the
/// RP2040, 1 for the RP2350.
pub fn assemble(source: &str, chip_version: u8) -> Result<Program> {
    let tokens = lex(source)?;
    let statements = Parser {
        tokens: &tokens,
        at: 0,
    }
    .program()?;
    Assembler::new(chip_version).run(statements)
}

// ---------------------------------------------------------------- lexing

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Directive(String),
    Int(i64),
    Float(f64),
    Symbol(&'static str),
    Newline,
}

#[derive(Clone, Debug)]
struct Token {
    tok: Tok,
    pos: Pos,
}

const SYMBOLS: [&str; 21] = [
    "<<", ">>", "!=", "--", "::", ",", ":", "[", "]", "(", ")", "+", "-", "*", "/", "|", "&", "^",
    "<", "!", "~",
];

fn lex(source: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut in_comment: Option<Pos> = None;
    let mut in_block: Option<Pos> = None;
    for (line_number, line) in source.split('\n').enumerate() {
        let bytes = line.as_bytes();
        let mut at = 0;
        let pos = |start: usize, end: usize| Pos {
            line: line_number,
            start,
            end,
        };
        if in_block.is_some() {
            // A code block for another language: skipped up to `%}`.
            if line.trim_start().starts_with("%}") {
                in_block = None;
            }
            continue;
        }
        let mut lang_opt = false;
        while at < bytes.len() {
            if in_comment.is_some() {
                match line[at..].find("*/") {
                    Some(end) => {
                        at += end + 2;
                        in_comment = None;
                        continue;
                    }
                    None => break,
                }
            }
            let ch = bytes[at];
            let rest = &line[at..];
            if lang_opt || ch == b';' || rest.starts_with("//") {
                break;
            }
            if ch.is_ascii_whitespace() {
                at += 1;
                continue;
            }
            if rest.starts_with("/*") {
                in_comment = Some(pos(at, at + 2));
                at += 2;
                continue;
            }
            if ch == b'%' {
                if !rest.trim_end().ends_with('{') {
                    return pos(at, at + 1).error(
                        "expected a code block such as `% c-sdk {`, closed by `%}` on its own line",
                    );
                }
                in_block = Some(pos(at, line.len()));
                break;
            }
            let start = at;
            let tok = if ch == b'.' && bytes.get(at + 1).is_some_and(u8::is_ascii_alphabetic) {
                at += 1;
                while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                    at += 1;
                }
                let name = line[start + 1..at].to_ascii_lowercase();
                lang_opt = name == "lang_opt";
                Tok::Directive(name)
            } else if ch.is_ascii_alphabetic() || ch == b'_' {
                while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                    at += 1;
                }
                Tok::Ident(line[start..at].to_owned())
            } else if ch.is_ascii_digit() || ch == b'.' {
                while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'.') {
                    at += 1;
                }
                number(&line[start..at]).ok_or(()).or_else(|()| {
                    pos(start, at).error(format!(
                        "`{}` is not a number; write decimal, 0x hexadecimal, or 0b binary",
                        &line[start..at]
                    ))
                })?
            } else if let Some(symbol) = SYMBOLS.iter().find(|s| rest.starts_with(**s)) {
                at += symbol.len();
                Tok::Symbol(symbol)
            } else {
                let ch = rest.chars().next().unwrap();
                return pos(at, at + ch.len_utf8()).error(format!("unexpected character {ch:?}"));
            };
            tokens.push(Token {
                tok,
                pos: pos(start, at),
            });
        }
        if in_comment.is_none() {
            tokens.push(Token {
                tok: Tok::Newline,
                pos: pos(line.len(), line.len()),
            });
        }
    }
    if let Some(pos) = in_comment {
        return pos.error("unterminated `/*` comment");
    }
    if let Some(pos) = in_block {
        return pos.error("unterminated code block; close it with `%}`");
    }
    Ok(tokens)
}

fn number(text: &str) -> Option<Tok> {
    let lower = text.to_ascii_lowercase();
    let value = if let Some(hex) = lower.strip_prefix("0x") {
        i64::from_str_radix(hex, 16).ok()?
    } else if let Some(binary) = lower.strip_prefix("0b") {
        i64::from_str_radix(binary, 2).ok()?
    } else if lower.contains('.') {
        let value: f64 = lower.parse().ok()?;
        return lower
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.')
            .then_some(Tok::Float(value));
    } else {
        lower.parse().ok()?
    };
    (value <= u32::MAX as i64).then_some(Tok::Int(value))
}

const RESERVED: &[&str] = &[
    "jmp", "wait", "in", "out", "push", "pull", "mov", "irq", "set", "nop", "public", "opt",
    "optional", "side", "sideset", "side_set", "pin", "gpio", "osre", "pins", "null", "pindirs",
    "x", "y", "pc", "exec", "isr", "osr", "status", "block", "noblock", "iffull", "ifempty", "rel",
    "clear", "nowait", "jmppin", "next", "prev", "txrx", "tx", "rx", "txput", "txget", "putget",
    "rp2040", "rp2350", "rxfifo", "txfifo", "left", "right", "auto", "manual",
];

fn reserved(name: &str) -> bool {
    RESERVED.contains(&name.to_ascii_lowercase().as_str())
}

// ---------------------------------------------------------------- parsing

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Or,
    And,
    Xor,
    Shl,
    Shr,
}

#[derive(Clone, Debug)]
enum Expr {
    Int(i64, Pos),
    Symbol(String, Pos),
    Negate(Box<Expr>, Pos),
    Reverse(Box<Expr>, Pos),
    Binary(Op, Box<Expr>, Box<Expr>, Pos),
}

impl Expr {
    fn pos(&self) -> Pos {
        match self {
            Self::Int(_, pos)
            | Self::Symbol(_, pos)
            | Self::Negate(_, pos)
            | Self::Reverse(_, pos)
            | Self::Binary(.., pos) => *pos,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Condition {
    Always = 0,
    XZero = 1,
    XDecrement = 2,
    YZero = 3,
    YDecrement = 4,
    XNotY = 5,
    Pin = 6,
    OsrNotEmpty = 7,
}

/// Which IRQ flag an `irq` or `wait irq` names: the flag itself, one
/// relative to the state machine, or one of the neighbouring block's
/// (RP2350), encoded in bits 4:3 of the index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IrqKind {
    Absolute = 0,
    Previous = 1,
    Relative = 2,
    Next = 3,
}

#[derive(Clone, Debug)]
enum WaitSource {
    Gpio(Expr),
    Pin(Expr),
    Irq(IrqKind, Expr),
    JmpPin(Option<Expr>),
}

/// The RX FIFO entry a v1 `mov rxfifo[...]` names: Y or a constant.
#[derive(Clone, Debug)]
enum FifoIndex {
    Y,
    Index(Expr),
}

#[derive(Clone, Debug)]
enum Instr {
    Jmp(Condition, Expr),
    Wait(Option<Expr>, WaitSource),
    In(u16, Expr),
    Out(u16, Expr),
    Push {
        if_full: bool,
        block: bool,
    },
    Pull {
        if_empty: bool,
        block: bool,
    },
    Mov {
        destination: u16,
        op: u16,
        source: u16,
    },
    MovToRx(FifoIndex),
    MovFromRx(FifoIndex),
    Irq {
        kind: IrqKind,
        mode: u16,
        index: Expr,
    },
    Set(u16, Expr),
    Word(Expr),
}

#[derive(Clone, Debug)]
struct Instruction {
    instr: Instr,
    side: Option<Expr>,
    delay: Option<Expr>,
    pos: Pos,
    /// Where an RP2350-only feature is used, and its name.
    v1: Option<(Pos, &'static str)>,
}

#[derive(Clone, Debug)]
enum Statement {
    Program(Pos),
    Define(String, Expr, Pos),
    Label(String, Pos),
    Origin(Expr, Pos),
    SideSet(Expr, bool, bool, Pos),
    WrapTarget(Pos),
    Wrap(Pos),
    PioVersion(i64, Pos),
    ClockDiv(f64, Pos),
    Fifo(Fifo, Pos),
    MovStatus(u32, IrqKind, Expr, Pos),
    InOut(bool, Expr, bool, bool, Option<Expr>, Pos),
    SetCount(Expr, Pos),
    Instruction(Box<Instruction>),
    /// `.lang_opt`, for other languages' output.
    Ignored,
}

struct Parser<'a> {
    tokens: &'a [Token],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> &Token {
        &self.tokens[self.at.min(self.tokens.len() - 1)]
    }
    fn next(&mut self) -> Token {
        let token = self.peek().clone();
        if self.at < self.tokens.len() {
            self.at += 1;
        }
        token
    }
    fn pos(&self) -> Pos {
        self.peek().pos
    }
    fn previous(&self) -> Pos {
        self.tokens[self.at.saturating_sub(1)].pos
    }
    fn at_keyword(&self, keyword: &str) -> bool {
        matches!(&self.peek().tok, Tok::Ident(name) if name.eq_ignore_ascii_case(keyword))
    }
    fn at_symbol(&self, symbol: &str) -> bool {
        matches!(&self.peek().tok, Tok::Symbol(s) if *s == symbol)
    }
    fn eat_keyword(&mut self, keyword: &str) -> bool {
        let found = self.at_keyword(keyword);
        if found {
            self.at += 1;
        }
        found
    }
    fn eat_symbol(&mut self, symbol: &str) -> bool {
        let found = self.at_symbol(symbol);
        if found {
            self.at += 1;
        }
        found
    }
    fn at_line_end(&self) -> bool {
        matches!(self.peek().tok, Tok::Newline)
    }
    fn expect_symbol(&mut self, symbol: &str) -> Result<()> {
        if self.eat_symbol(symbol) {
            Ok(())
        } else {
            self.pos().error(format!("expected `{symbol}`"))
        }
    }
    /// The keyword at the cursor, lower-cased, if it is one of `choices`.
    fn choice(&mut self, choices: &[&'static str]) -> Option<&'static str> {
        let Tok::Ident(name) = &self.peek().tok else {
            return None;
        };
        let found = choices
            .iter()
            .find(|choice| name.eq_ignore_ascii_case(choice))?;
        self.at += 1;
        Some(found)
    }
    fn expect_choice(&mut self, choices: &[&'static str], what: &str) -> Result<&'static str> {
        self.choice(choices).map_or_else(
            || {
                self.pos().error(format!(
                    "expected {what}: {}",
                    choices
                        .iter()
                        .map(|c| format!("`{c}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            },
            Ok,
        )
    }
    fn comma(&mut self) {
        self.eat_symbol(",");
    }

    fn program(mut self) -> Result<Vec<Statement>> {
        let mut statements = Vec::new();
        while self.at < self.tokens.len() {
            if self.at_line_end() {
                self.at += 1;
                continue;
            }
            self.line(&mut statements)?;
            if !self.at_line_end() {
                return self
                    .pos()
                    .error("unexpected text; each line holds one directive or instruction");
            }
        }
        Ok(statements)
    }

    fn line(&mut self, statements: &mut Vec<Statement>) -> Result<()> {
        if let Tok::Directive(name) = &self.peek().tok {
            let name = name.clone();
            let pos = self.next().pos;
            statements.push(self.directive(&name, pos)?);
            return Ok(());
        }
        // A label: `name:`, `public name:`, or `*name:`.
        let save = self.at;
        let public = self.eat_keyword("public") || self.eat_symbol("*");
        if let Tok::Ident(name) = &self.peek().tok
            && matches!(
                self.tokens.get(self.at + 1),
                Some(Token {
                    tok: Tok::Symbol(":"),
                    ..
                })
            )
        {
            let name = name.clone();
            let pos = self.next().pos;
            if reserved(&name) {
                return pos.error(format!("`{name}` is a keyword and cannot be a label"));
            }
            self.at += 1;
            statements.push(Statement::Label(name, pos));
            if self.at_line_end() {
                return Ok(());
            }
        } else if public {
            self.at = save;
            return self.pos().error("expected a label after `public`");
        }
        statements.push(Statement::Instruction(Box::new(self.instruction()?)));
        Ok(())
    }

    fn directive(&mut self, name: &str, pos: Pos) -> Result<Statement> {
        Ok(match name {
            "program" => {
                let Tok::Ident(_) = self.next().tok else {
                    return self
                        .previous()
                        .error("expected the program's name after .program");
                };
                Statement::Program(pos)
            }
            "define" => {
                let _ = self.eat_keyword("public") || self.eat_symbol("*");
                let Tok::Ident(symbol) = self.next().tok else {
                    return self.previous().error("expected a name after .define");
                };
                let at = self.previous();
                if reserved(&symbol) {
                    return at.error(format!("`{symbol}` is a keyword and cannot be defined"));
                }
                Statement::Define(symbol, self.expression()?, at)
            }
            "origin" => Statement::Origin(self.value()?, pos),
            "side_set" => {
                let count = self.value()?;
                let optional = self.choice(&["opt", "optional"]).is_some();
                let pindirs = self.eat_keyword("pindirs");
                Statement::SideSet(count, optional, pindirs, pos)
            }
            "wrap_target" => Statement::WrapTarget(pos),
            "wrap" => Statement::Wrap(pos),
            "word" => {
                let value = self.value()?;
                Statement::Instruction(Box::new(Instruction {
                    pos: pos.to(value.pos()),
                    instr: Instr::Word(value),
                    side: None,
                    delay: None,
                    v1: None,
                }))
            }
            "lang_opt" => Statement::Ignored,
            "pio_version" => {
                let token = self.next();
                let version = match &token.tok {
                    Tok::Int(version) => *version,
                    Tok::Ident(name) if name.eq_ignore_ascii_case("rp2040") => 0,
                    Tok::Ident(name) if name.eq_ignore_ascii_case("rp2350") => 1,
                    _ => {
                        return token
                            .pos
                            .error("expected a PIO version: 0, 1, `rp2040`, or `rp2350`");
                    }
                };
                Statement::PioVersion(version, token.pos)
            }
            "clock_div" => {
                let token = self.next();
                let divider = match token.tok {
                    Tok::Int(value) => value as f64,
                    Tok::Float(value) => value,
                    _ => return token.pos.error("expected a clock divider, such as 2.5"),
                };
                Statement::ClockDiv(divider, token.pos)
            }
            "fifo" => {
                let at = self.pos();
                let fifo = match self.expect_choice(
                    &["txrx", "tx", "rx", "txput", "txget", "putget"],
                    "a FIFO configuration",
                )? {
                    "txrx" => Fifo::TxRx,
                    "tx" => Fifo::Tx,
                    "rx" => Fifo::Rx,
                    "txput" => Fifo::TxPut,
                    "txget" => Fifo::TxGet,
                    _ => Fifo::PutGet,
                };
                Statement::Fifo(fifo, at.to(self.previous()))
            }
            "mov_status" => {
                let at = self.pos();
                match self.choice(&["txfifo", "rxfifo", "irq"]) {
                    Some("irq") => {
                        let kind = match self.choice(&["next", "prev"]) {
                            Some("next") => IrqKind::Next,
                            Some(_) => IrqKind::Previous,
                            None => IrqKind::Absolute,
                        };
                        if !self.eat_keyword("set") {
                            return self
                                .pos()
                                .error("expected `set`, as in `.mov_status irq set 2`");
                        }
                        Statement::MovStatus(2, kind, self.value()?, at.to(self.previous()))
                    }
                    Some(fifo) => {
                        self.expect_symbol("<")?;
                        let select = u32::from(fifo == "rxfifo");
                        Statement::MovStatus(select, IrqKind::Absolute, self.value()?, at)
                    }
                    None => {
                        return at.error(
                            "expected `txfifo < N`, `rxfifo < N`, or `irq [next|prev] set N`",
                        );
                    }
                }
            }
            "in" | "out" => {
                let count = self.value()?;
                let right = self.choice(&["left", "right"]) != Some("left");
                let auto = self.choice(&["auto", "manual"]) == Some("auto");
                let threshold = if self.at_line_end() {
                    None
                } else {
                    Some(self.value()?)
                };
                Statement::InOut(name == "in", count, right, auto, threshold, pos)
            }
            "set" => Statement::SetCount(self.value()?, pos),
            _ => return pos.error(format!("unknown directive `.{name}`")),
        })
    }

    fn instruction(&mut self) -> Result<Instruction> {
        let pos = self.pos();
        let Tok::Ident(name) = &self.peek().tok else {
            return pos.error("expected an instruction, a label, or a directive");
        };
        let name = name.to_ascii_lowercase();
        self.at += 1;
        let mut v1 = None;
        let instr = match name.as_str() {
            "nop" => Instr::Mov {
                destination: 2,
                op: 0,
                source: 2,
            },
            "jmp" => {
                let condition = if self.eat_symbol("!") || self.eat_symbol("~") {
                    match self.expect_choice(&["x", "y", "osre"], "a JMP condition")? {
                        "x" => Condition::XZero,
                        "y" => Condition::YZero,
                        _ => Condition::OsrNotEmpty,
                    }
                } else if self.eat_keyword("pin") {
                    Condition::Pin
                } else if let Some(register) = self.choice(&["x", "y"]) {
                    if self.eat_symbol("--") {
                        if register == "x" {
                            Condition::XDecrement
                        } else {
                            Condition::YDecrement
                        }
                    } else if register == "x" && self.eat_symbol("!=") {
                        if !self.eat_keyword("y") {
                            return self.pos().error("expected `y`, as in `jmp x!=y label`");
                        }
                        Condition::XNotY
                    } else {
                        return self
                            .previous()
                            .error("expected a JMP condition: `!x`, `x--`, `!y`, `y--`, `x!=y`, `pin`, or `!osre`");
                    }
                } else {
                    Condition::Always
                };
                self.comma();
                Instr::Jmp(condition, self.expression()?)
            }
            "wait" => {
                let polarity = if self.at_keyword("gpio")
                    || self.at_keyword("pin")
                    || self.at_keyword("irq")
                    || self.at_keyword("jmppin")
                {
                    None
                } else {
                    Some(self.value()?)
                };
                let at = self.pos();
                let source =
                    match self.expect_choice(&["gpio", "pin", "irq", "jmppin"], "a WAIT source")? {
                        "gpio" => {
                            self.comma();
                            WaitSource::Gpio(self.value()?)
                        }
                        "pin" => {
                            self.comma();
                            WaitSource::Pin(self.value()?)
                        }
                        "irq" => {
                            let kind = self.irq_neighbour(&mut v1, "wait irq prev/next");
                            self.comma();
                            let index = self.value()?;
                            WaitSource::Irq(self.irq_relative(kind)?, index)
                        }
                        _ => {
                            v1 = Some((at, "wait jmppin"));
                            WaitSource::JmpPin(if self.eat_symbol("+") {
                                Some(self.value()?)
                            } else {
                                None
                            })
                        }
                    };
                Instr::Wait(polarity, source)
            }
            "in" => {
                let source = match self
                    .expect_choice(&["pins", "x", "y", "null", "isr", "osr"], "an IN source")?
                {
                    "pins" => 0,
                    "x" => 1,
                    "y" => 2,
                    "null" => 3,
                    "isr" => 6,
                    _ => 7,
                };
                self.comma();
                Instr::In(source, self.value()?)
            }
            "out" => {
                let destination = match self.expect_choice(
                    &["pins", "x", "y", "null", "pindirs", "pc", "isr", "exec"],
                    "an OUT destination",
                )? {
                    "pins" => 0,
                    "x" => 1,
                    "y" => 2,
                    "null" => 3,
                    "pindirs" => 4,
                    "pc" => 5,
                    "isr" => 6,
                    _ => 7,
                };
                self.comma();
                Instr::Out(destination, self.value()?)
            }
            "push" => {
                let if_full = self.eat_keyword("iffull");
                let block = self.choice(&["block", "noblock"]) != Some("noblock");
                Instr::Push { if_full, block }
            }
            "pull" => {
                let if_empty = self.eat_keyword("ifempty");
                let block = self.choice(&["block", "noblock"]) != Some("noblock");
                Instr::Pull { if_empty, block }
            }
            "mov" => {
                let at = self.pos();
                if self.eat_keyword("rxfifo") {
                    v1 = Some((at, "mov rxfifo[]"));
                    let index = self.fifo_index()?;
                    self.comma();
                    let source = self.pos();
                    if !self.eat_keyword("isr") {
                        return source.error("`mov rxfifo[...]` takes its value from `isr`");
                    }
                    Instr::MovToRx(index)
                } else {
                    let destination = match self.expect_choice(
                        &["pins", "x", "y", "pindirs", "exec", "pc", "isr", "osr"],
                        "a MOV destination",
                    )? {
                        "pins" => 0,
                        "x" => 1,
                        "y" => 2,
                        "pindirs" => {
                            v1 = Some((at, "mov pindirs"));
                            3
                        }
                        "exec" => 4,
                        "pc" => 5,
                        "isr" => 6,
                        _ => 7,
                    };
                    self.comma();
                    let op = if self.eat_symbol("!") || self.eat_symbol("~") {
                        1
                    } else if self.eat_symbol("::") {
                        2
                    } else {
                        0
                    };
                    let source_pos = self.pos();
                    if self.eat_keyword("rxfifo") {
                        v1 = Some((source_pos, "mov osr, rxfifo[]"));
                        if destination != 7 || op != 0 {
                            return at
                                .to(source_pos)
                                .error("`rxfifo[...]` can only be moved into `osr`, unchanged");
                        }
                        Instr::MovFromRx(self.fifo_index()?)
                    } else {
                        let source = match self.expect_choice(
                            &["pins", "x", "y", "null", "status", "isr", "osr"],
                            "a MOV source",
                        )? {
                            "pins" => 0,
                            "x" => 1,
                            "y" => 2,
                            "null" => 3,
                            "status" => 5,
                            "isr" => 6,
                            _ => 7,
                        };
                        Instr::Mov {
                            destination,
                            op,
                            source,
                        }
                    }
                }
            }
            "irq" => {
                let kind = self.irq_neighbour(&mut v1, "irq prev/next");
                let mode = match self.choice(&["set", "nowait", "wait", "clear"]) {
                    Some("wait") => 1,
                    Some("clear") => 2,
                    _ => 0,
                };
                let index = self.value()?;
                Instr::Irq {
                    kind: self.irq_relative(kind)?,
                    mode,
                    index,
                }
            }
            "set" => {
                let destination = match self
                    .expect_choice(&["pins", "x", "y", "pindirs"], "a SET destination")?
                {
                    "pins" => 0,
                    "x" => 1,
                    "y" => 2,
                    _ => 4,
                };
                self.comma();
                Instr::Set(destination, self.value()?)
            }
            _ => {
                return pos.error(format!(
                    "unknown instruction `{name}`; PIO has jmp, wait, in, out, push, pull, mov, irq, set, and nop"
                ));
            }
        };
        let (mut side, mut delay) = (None, None);
        loop {
            if side.is_none() && self.choice(&["side", "sideset", "side_set"]).is_some() {
                side = Some(self.value()?);
            } else if delay.is_none() && self.eat_symbol("[") {
                delay = Some(self.expression()?);
                self.expect_symbol("]")?;
            } else {
                break;
            }
        }
        Ok(Instruction {
            instr,
            side,
            delay,
            pos: pos.to(self.previous()),
            v1,
        })
    }

    /// `prev` or `next` after `irq`: a flag of the neighbouring PIO block.
    fn irq_neighbour(
        &mut self,
        v1: &mut Option<(Pos, &'static str)>,
        feature: &'static str,
    ) -> IrqKind {
        let at = self.pos();
        match self.choice(&["prev", "next"]) {
            Some(neighbour) => {
                *v1 = Some((at, feature));
                if neighbour == "prev" {
                    IrqKind::Previous
                } else {
                    IrqKind::Next
                }
            }
            None => IrqKind::Absolute,
        }
    }

    /// A trailing `rel`, which neighbouring blocks' flags do not take.
    fn irq_relative(&mut self, kind: IrqKind) -> Result<IrqKind> {
        let at = self.pos();
        if !self.eat_keyword("rel") {
            return Ok(kind);
        }
        if kind != IrqKind::Absolute {
            return at.error("`rel` does not apply to the flags of `prev` or `next` blocks");
        }
        Ok(IrqKind::Relative)
    }

    fn fifo_index(&mut self) -> Result<FifoIndex> {
        self.expect_symbol("[")?;
        let index = if self.eat_keyword("y") {
            FifoIndex::Y
        } else {
            FifoIndex::Index(self.value()?)
        };
        self.expect_symbol("]")?;
        Ok(index)
    }

    /// A number, a symbol, or a parenthesized expression.
    fn value(&mut self) -> Result<Expr> {
        let token = self.next();
        match token.tok {
            Tok::Int(value) => Ok(Expr::Int(value, token.pos)),
            Tok::Ident(name) if !reserved(&name) => Ok(Expr::Symbol(name, token.pos)),
            Tok::Symbol("(") => {
                let inner = self.expression()?;
                self.expect_symbol(")")?;
                Ok(inner)
            }
            Tok::Symbol("-") => token
                .pos
                .error("a negative value needs parentheses, as in `(-1)`"),
            _ => token
                .pos
                .error("expected a number, a symbol, or a parenthesized expression"),
        }
    }

    /// Operators by pioasm's precedence, loosest first: `::` (prefix), `<<`
    /// and `>>`, `+` and `-`, `*` and `/`, then `&`, `|`, and `^`, all left
    /// associative. Prefix `-` binds like binary `-`.
    fn expression(&mut self) -> Result<Expr> {
        self.binary(0)
    }

    fn binary(&mut self, min: u8) -> Result<Expr> {
        let start = self.pos();
        let mut left = if self.eat_symbol("-") {
            let operand = self.binary(3)?;
            let pos = start.to(operand.pos());
            Expr::Negate(Box::new(operand), pos)
        } else if self.eat_symbol("::") {
            let operand = self.binary(1)?;
            let pos = start.to(operand.pos());
            Expr::Reverse(Box::new(operand), pos)
        } else {
            self.value()?
        };
        loop {
            let (op, precedence) = match &self.peek().tok {
                Tok::Symbol("<<") => (Op::Shl, 1),
                Tok::Symbol(">>") => (Op::Shr, 1),
                Tok::Symbol("+") => (Op::Add, 2),
                Tok::Symbol("-") => (Op::Sub, 2),
                Tok::Symbol("*") => (Op::Mul, 3),
                Tok::Symbol("/") => (Op::Div, 3),
                Tok::Symbol("&") => (Op::And, 4),
                Tok::Symbol("|") => (Op::Or, 4),
                Tok::Symbol("^") => (Op::Xor, 4),
                _ => break,
            };
            if precedence < min {
                break;
            }
            self.at += 1;
            let right = self.binary(precedence + 1)?;
            let pos = start.to(right.pos());
            left = Expr::Binary(op, Box::new(left), Box::new(right), pos);
        }
        Ok(left)
    }
}

// ---------------------------------------------------------------- assembling

enum Symbol {
    Label(u32),
    Define(Expr),
}

struct Assembler {
    chip_version: u8,
    symbols: HashMap<String, (Symbol, Pos)>,
    resolving: HashSet<String>,
}

/// A directive that may appear once, and where it did.
fn once(slot: &mut Option<Pos>, pos: Pos, name: &str) -> Result<()> {
    if let Some(first) = slot {
        return pos.error(format!(
            "{name} is already given on line {}",
            first.line + 1
        ));
    }
    *slot = Some(pos);
    Ok(())
}

impl Assembler {
    fn new(chip_version: u8) -> Self {
        Self {
            chip_version,
            symbols: HashMap::new(),
            resolving: HashSet::new(),
        }
    }

    fn evaluate(&mut self, expr: &Expr) -> Result<i64> {
        let overflow = || expr.pos().error("the expression overflows");
        Ok(match expr {
            Expr::Int(value, _) => *value,
            Expr::Symbol(name, pos) => {
                let Some((symbol, _)) = self.symbols.get(name) else {
                    return pos.error(format!("undefined symbol `{name}`"));
                };
                match symbol {
                    Symbol::Label(address) => i64::from(*address),
                    Symbol::Define(value) => {
                        let value = value.clone();
                        if !self.resolving.insert(name.clone()) {
                            return pos.error(format!("`{name}` is defined in terms of itself"));
                        }
                        let result = self.evaluate(&value);
                        self.resolving.remove(name);
                        result?
                    }
                }
            }
            Expr::Negate(operand, _) => -self.evaluate(operand)?,
            Expr::Reverse(operand, _) => i64::from((self.evaluate(operand)? as u32).reverse_bits()),
            Expr::Binary(op, left, right, pos) => {
                let (left, right) = (self.evaluate(left)?, self.evaluate(right)?);
                match op {
                    Op::Add => left.checked_add(right).map_or_else(overflow, Ok)?,
                    Op::Sub => left.checked_sub(right).map_or_else(overflow, Ok)?,
                    Op::Mul => left.checked_mul(right).map_or_else(overflow, Ok)?,
                    Op::Div if right == 0 => return pos.error("division by zero"),
                    Op::Div => left / right,
                    Op::Or => left | right,
                    Op::And => left & right,
                    Op::Xor => left ^ right,
                    Op::Shl | Op::Shr if !(0..32).contains(&right) => {
                        return pos.error("shift amounts run from 0 to 31");
                    }
                    Op::Shl => i64::from((left as u32) << right),
                    Op::Shr => i64::from((left as u32) >> right),
                }
            }
        })
    }

    /// Evaluate `expr` and check that it lies in `min..=max`.
    fn ranged(&mut self, expr: &Expr, min: i64, max: i64, what: &str) -> Result<u32> {
        let value = self.evaluate(expr)?;
        if value < min || value > max {
            return expr
                .pos()
                .error(format!("{what} must be {min} to {max}, not {value}"));
        }
        Ok(value as u32)
    }

    fn run(mut self, statements: Vec<Statement>) -> Result<Program> {
        // Pass 1: symbols, and the directives that shape every instruction.
        let mut count = 0u32;
        let mut first_instruction: Option<Pos> = None;
        let (mut program, mut side_set_at, mut version_at) = (None, None, None);
        let mut side_set = None;
        let mut version = None;
        for statement in &statements {
            match statement {
                Statement::Program(pos) => {
                    if program.is_some() {
                        return pos.error(
                            "rp_pio.assemble takes one program; assemble each .program separately",
                        );
                    }
                    if first_instruction.is_some() {
                        return pos.error(".program must come before the program's instructions");
                    }
                    program = Some(*pos);
                }
                Statement::Label(name, pos) | Statement::Define(name, _, pos) => {
                    let symbol = match statement {
                        Statement::Label(..) => Symbol::Label(count),
                        Statement::Define(_, value, _) => Symbol::Define(value.clone()),
                        _ => unreachable!(),
                    };
                    if let Some((_, first)) = self.symbols.get(name) {
                        return pos.error(format!(
                            "`{name}` is already defined on line {}",
                            first.line + 1
                        ));
                    }
                    self.symbols.insert(name.clone(), (symbol, *pos));
                }
                Statement::SideSet(bits, optional, pindirs, pos) => {
                    once(&mut side_set_at, *pos, ".side_set")?;
                    if first_instruction.is_some() {
                        return pos.error(".side_set must come before the first instruction");
                    }
                    side_set = Some((bits.clone(), *optional, *pindirs));
                }
                Statement::PioVersion(value, pos) => {
                    once(&mut version_at, *pos, ".pio_version")?;
                    version = Some((*value, *pos));
                }
                Statement::Instruction(instruction) => {
                    first_instruction.get_or_insert(instruction.pos);
                    count += 1;
                    if count as usize > MAX_INSTRUCTIONS {
                        return instruction.pos.error(format!(
                            "a PIO program holds at most {MAX_INSTRUCTIONS} instructions"
                        ));
                    }
                }
                _ => {}
            }
        }
        if count == 0 {
            return Pos {
                line: usize::MAX,
                start: 0,
                end: 0,
            }
            .error("a PIO program needs at least one instruction");
        }
        let allowed = match version {
            Some((value, pos)) if !(0..=1).contains(&value) => {
                return pos.error("PIO versions are 0 (RP2040) and 1 (RP2350)");
            }
            Some((value, pos)) if value as u8 > self.chip_version => {
                return pos.error(
                    "this program needs PIO version 1 (the RP2350), but the build's chip, the RP2040, has version 0",
                );
            }
            Some((value, _)) => value as u8,
            None => self.chip_version,
        };
        let side_set = match side_set {
            Some((bits, optional, pindirs)) => {
                let max = if optional { 4 } else { 5 };
                let bits = self.ranged(
                    &bits,
                    1,
                    max,
                    if optional {
                        "an optional side-set's bit count"
                    } else {
                        "the side-set bit count"
                    },
                )?;
                Some(SideSet {
                    bits,
                    optional,
                    pindirs,
                })
            }
            None => None,
        };

        // Pass 2: everything else.
        let mut result = Program {
            instructions: Vec::with_capacity(count as usize),
            origin: None,
            wrap_target: 0,
            wrap: count - 1,
            side_set,
            version: 0,
            clock_divider: 256,
            fifo: Fifo::TxRx,
            status: Status { select: 0, n: 0 },
            input: Shift {
                count: 32,
                right: true,
                auto: false,
                threshold: 32,
            },
            output: Shift {
                count: 0,
                right: true,
                auto: false,
                threshold: 32,
            },
            set_count: 0,
        };
        let mut seen: [Option<Pos>; 9] = [None; 9];
        let [
            wrap_at,
            target_at,
            origin_at,
            clock_at,
            fifo_at,
            status_at,
            in_at,
            out_at,
            set_at,
        ] = &mut seen;
        let mut fifo_users: Vec<(Pos, &'static str, &'static [Fifo])> = Vec::new();
        let need_v1 = |pos: Pos, what: &str| -> Result<()> {
            if allowed >= 1 {
                return Ok(());
            }
            let reason = if version.is_some() {
                "the program declares .pio_version 0"
            } else {
                "the build's chip, the RP2040, has PIO version 0"
            };
            pos.error(format!(
                "`{what}` needs PIO version 1 (the RP2350), but {reason}"
            ))
        };
        let mut used_v1 = false;
        for statement in &statements {
            let address = result.instructions.len() as u32;
            match statement {
                Statement::WrapTarget(pos) => {
                    once(target_at, *pos, ".wrap_target")?;
                    if address >= count {
                        return pos.error(".wrap_target must come before an instruction");
                    }
                    result.wrap_target = address;
                }
                Statement::Wrap(pos) => {
                    once(wrap_at, *pos, ".wrap")?;
                    if address == 0 {
                        return pos.error(".wrap must come after an instruction");
                    }
                    result.wrap = address - 1;
                }
                Statement::Origin(value, pos) => {
                    once(origin_at, *pos, ".origin")?;
                    let origin =
                        self.ranged(value, 0, MAX_INSTRUCTIONS as i64 - 1, "the origin")?;
                    if origin + count > MAX_INSTRUCTIONS as u32 {
                        return value.pos().error(format!(
                            "a program of {count} instructions at origin {origin} runs past the end of instruction memory"
                        ));
                    }
                    result.origin = Some(origin);
                }
                Statement::ClockDiv(divider, pos) => {
                    once(clock_at, *pos, ".clock_div")?;
                    if !(1.0..65536.0).contains(divider) {
                        return pos.error("the clock divider must be at least 1 and below 65536");
                    }
                    result.clock_divider = (divider * 256.0) as u32;
                }
                Statement::Fifo(fifo, pos) => {
                    once(fifo_at, *pos, ".fifo")?;
                    if matches!(fifo, Fifo::TxPut | Fifo::TxGet | Fifo::PutGet) {
                        need_v1(
                            *pos,
                            &format!(".fifo {}", ["txput", "txget", "putget"][*fifo as usize - 3]),
                        )?;
                        used_v1 = true;
                    }
                    result.fifo = *fifo;
                }
                Statement::MovStatus(select, kind, value, pos) => {
                    once(status_at, *pos, ".mov_status")?;
                    result.status = if *select == 2 {
                        need_v1(*pos, ".mov_status irq")?;
                        used_v1 = true;
                        let flag = self.ranged(value, 0, 7, "the IRQ flag")?;
                        let block = match kind {
                            IrqKind::Previous => 8,
                            IrqKind::Next => 16,
                            _ => 0,
                        };
                        Status {
                            select: 2,
                            n: block + flag,
                        }
                    } else {
                        Status {
                            select: *select,
                            n: self.ranged(value, 0, 31, "the FIFO level")?,
                        }
                    };
                }
                Statement::InOut(input, pins, right, auto, threshold, pos) => {
                    let (slot, min) = if *input {
                        (&mut *in_at, 1)
                    } else {
                        (&mut *out_at, 0)
                    };
                    once(slot, *pos, if *input { ".in" } else { ".out" })?;
                    let pins = self.ranged(pins, min, 32, "the pin count")?;
                    if *input && pins != 32 {
                        need_v1(*pos, ".in with fewer than 32 pins")?;
                        used_v1 = true;
                    }
                    let threshold = match threshold {
                        Some(threshold) => self.ranged(threshold, 1, 32, "the threshold")?,
                        None => 32,
                    };
                    let shift = Shift {
                        count: pins,
                        right: *right,
                        auto: *auto,
                        threshold,
                    };
                    if *input {
                        result.input = shift;
                        if *auto {
                            fifo_users.push((*pos, "autopush", &[Fifo::TxRx, Fifo::Tx, Fifo::Rx]));
                        }
                    } else {
                        result.output = shift;
                    }
                }
                Statement::SetCount(pins, pos) => {
                    once(set_at, *pos, ".set")?;
                    result.set_count = self.ranged(pins, 0, 5, "the SET pin count")?;
                }
                Statement::Instruction(instruction) => {
                    if let Some((pos, what)) = instruction.v1 {
                        need_v1(pos, what)?;
                        used_v1 = true;
                    }
                    match &instruction.instr {
                        Instr::Push { .. } => {
                            fifo_users.push((instruction.pos, "push", &[Fifo::TxRx, Fifo::Rx]))
                        }
                        Instr::MovToRx(_) => fifo_users.push((
                            instruction.pos,
                            "mov rxfifo[...], isr",
                            &[Fifo::TxPut, Fifo::PutGet],
                        )),
                        Instr::MovFromRx(_) => fifo_users.push((
                            instruction.pos,
                            "mov osr, rxfifo[...]",
                            &[Fifo::TxGet, Fifo::PutGet],
                        )),
                        _ => {}
                    }
                    let word = self.encode(instruction, count, &result)?;
                    result.instructions.push(word);
                }
                _ => {}
            }
        }
        for (pos, what, allowed) in fifo_users {
            if !allowed.contains(&result.fifo) {
                let names = ["txrx", "tx", "rx", "txput", "txget", "putget"];
                let mut choices: Vec<_> =
                    allowed.iter().map(|fifo| names[*fifo as usize]).collect();
                let last = choices.pop().unwrap();
                let choices = match choices.len() {
                    0 => last.to_owned(),
                    1 => format!("{} or {last}", choices[0]),
                    _ => format!("{}, or {last}", choices.join(", ")),
                };
                return pos.error(format!(
                    "`{what}` needs .fifo {choices}, but the program sets .fifo {}",
                    names[result.fifo as usize]
                ));
            }
        }
        result.version = u8::from(used_v1);
        Ok(result)
    }

    fn encode(&mut self, instruction: &Instruction, count: u32, program: &Program) -> Result<u16> {
        let (kind, arg1, arg2): (u16, u16, u16) = match &instruction.instr {
            Instr::Word(value) => {
                return Ok(self.ranged(value, 0, 0xFFFF, "a .word")? as u16);
            }
            Instr::Jmp(condition, target) => {
                let address = self.evaluate(target)?;
                if address < 0 || address >= i64::from(count) {
                    return target.pos().error(format!(
                        "jump target {address} is outside the program's {count} instructions"
                    ));
                }
                (0, *condition as u16, address as u16)
            }
            Instr::Wait(polarity, source) => {
                let polarity = match polarity {
                    Some(value) => self.ranged(value, 0, 1, "the WAIT polarity")? as u16,
                    None => 1,
                };
                let (source, index) = match source {
                    WaitSource::Gpio(index) => (0, self.ranged(index, 0, 31, "the GPIO number")?),
                    WaitSource::Pin(index) => (1, self.ranged(index, 0, 31, "the pin number")?),
                    WaitSource::Irq(kind, index) => (
                        2,
                        (*kind as u32) << 3 | self.ranged(index, 0, 7, "the IRQ flag")?,
                    ),
                    WaitSource::JmpPin(offset) => (
                        3,
                        match offset {
                            Some(offset) => self.ranged(offset, 0, 3, "the jmppin offset")?,
                            None => 0,
                        },
                    ),
                };
                (1, polarity << 2 | source, index as u16)
            }
            Instr::In(source, bits) => (
                2,
                *source,
                (self.ranged(bits, 1, 32, "the IN bit count")? & 31) as u16,
            ),
            Instr::Out(destination, bits) => (
                3,
                *destination,
                (self.ranged(bits, 1, 32, "the OUT bit count")? & 31) as u16,
            ),
            Instr::Push { if_full, block } => (4, u16::from(*if_full) << 1 | u16::from(*block), 0),
            Instr::Pull { if_empty, block } => {
                (4, 4 | u16::from(*if_empty) << 1 | u16::from(*block), 0)
            }
            Instr::MovToRx(index) => (4, 0, 0x10 | self.fifo_index(index)?),
            Instr::MovFromRx(index) => (4, 4, 0x10 | self.fifo_index(index)?),
            Instr::Mov {
                destination,
                op,
                source,
            } => (5, *destination, op << 3 | source),
            Instr::Irq { kind, mode, index } => (
                6,
                *mode,
                (*kind as u16) << 3 | self.ranged(index, 0, 7, "the IRQ flag")? as u16,
            ),
            Instr::Set(destination, value) => (
                7,
                *destination,
                self.ranged(value, 0, 31, "the SET value")? as u16,
            ),
        };
        let mut field = 0u16;
        let delay_bits = 5 - program.side_set.map_or(0, SideSet::count);
        match (&instruction.side, program.side_set) {
            (Some(side), None) => {
                return side.pos().error("`side` needs a .side_set directive");
            }
            (None, Some(side_set)) if !side_set.optional => {
                return instruction.pos.error(
                    "every instruction needs `side`, since the program's .side_set is not `opt`",
                );
            }
            (Some(side), Some(side_set)) => {
                let value = self.ranged(side, 0, (1 << side_set.bits) - 1, "the side-set value")?;
                field |= (value as u16) << delay_bits;
                if side_set.optional {
                    field |= 0x10;
                }
            }
            (None, _) => {}
        }
        if let Some(delay) = &instruction.delay {
            let max = (1 << delay_bits) - 1;
            let value = self.evaluate(delay)?;
            if value < 0 || value > max {
                let reason = if program.side_set.is_some() {
                    format!(", since side-set takes {} of its 5 bits", 5 - delay_bits)
                } else {
                    String::new()
                };
                return delay
                    .pos()
                    .error(format!("the delay must be 0 to {max}{reason}, not {value}"));
            }
            field |= value as u16;
        }
        Ok(kind << 13 | field << 8 | arg1 << 5 | arg2)
    }

    fn fifo_index(&mut self, index: &FifoIndex) -> Result<u16> {
        Ok(match index {
            FifoIndex::Y => 0,
            FifoIndex::Index(index) => 8 | self.ranged(index, 0, 3, "the RX FIFO index")? as u16,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(source: &str) -> Vec<u16> {
        assemble(source, 1).unwrap().instructions
    }

    fn error(source: &str, version: u8) -> String {
        assemble(source, version).unwrap_err().message
    }

    #[test]
    fn datasheet_programs_match_pioasm() {
        // RP2040 datasheet 3.6.1.
        assert_eq!(
            words(
                ".program squarewave\n    set pindirs, 1   ; Set pin to output\nagain:\n    set pins, 1 [1]  ; Drive pin high and then delay for one cycle\n    set pins, 0      ; Drive pin low\n    jmp again        ; Set PC to label `again`\n"
            ),
            [0xe081, 0xe101, 0xe000, 0x0001]
        );
        // pico-examples uart_tx.pio.
        let uart = assemble(
            ".program uart_tx\n.side_set 1 opt\n    pull       side 1 [7]\n    set x, 7   side 0 [7]\nbitloop:\n    out pins, 1\n    jmp x-- bitloop   [6]\n",
            0,
        )
        .unwrap();
        assert_eq!(uart.instructions, [0x9fa0, 0xf727, 0x6001, 0x0642]);
        assert_eq!(
            uart.side_set,
            Some(SideSet {
                bits: 1,
                optional: true,
                pindirs: false
            })
        );
        assert_eq!((uart.wrap_target, uart.wrap, uart.version), (0, 3, 0));
        // pico-examples ws2812.pio.
        let ws2812 = assemble(
            ".program ws2812\n.side_set 1\n.define public T1 3\n.define public T2 3\n.define public T3 4\n.lang_opt python sideset_init = pico.PIO.OUT_HIGH\n.wrap_target\nbitloop:\n    out x, 1       side 0 [T3 - 1] ; Side-set still takes place when instruction stalls\n    jmp !x do_zero side 1 [T1 - 1] ; Branch on the bit we shifted out. Positive pulse\ndo_one:\n    jmp  bitloop   side 1 [T2 - 1] ; Continue driving high, for a long pulse\ndo_zero:\n    nop            side 0 [T2 - 1] ; Or drive low, for a short pulse\n.wrap\n% c-sdk {\n#include \"hardware/clocks.h\"\n%}\n",
            0,
        )
        .unwrap();
        assert_eq!(ws2812.instructions, [0x6321, 0x1223, 0x1200, 0xa242]);
        assert_eq!((ws2812.wrap_target, ws2812.wrap), (0, 3));
    }

    #[test]
    fn every_instruction_encodes() {
        let cases: &[(&str, u16)] = &[
            ("jmp 0", 0x0000),
            ("jmp !x, 0", 0x0020),
            ("jmp x-- 0", 0x0040),
            ("jmp !y 0", 0x0060),
            ("jmp y--, 0", 0x0080),
            ("jmp x!=y 0", 0x00a0),
            ("jmp pin 0", 0x00c0),
            ("jmp !osre 0", 0x00e0),
            ("wait 1 gpio 5", 0x2085),
            ("wait 0 pin, 3", 0x2023),
            ("wait irq 3 rel", 0x20d3),
            ("wait 1 irq next 2", 0x20da),
            ("wait 0 irq prev, 1", 0x2049),
            ("wait 1 jmppin", 0x20e0),
            ("wait 0 jmppin + 2", 0x2062),
            ("in pins, 32", 0x4000),
            ("in osr 8", 0x40e8),
            ("out pindirs, 1", 0x6081),
            ("out exec, 16", 0x60f0),
            ("out pc 5", 0x60a5),
            ("push", 0x8020),
            ("push iffull noblock", 0x8040),
            ("pull", 0x80a0),
            ("pull ifempty block", 0x80e0),
            ("pull noblock", 0x8080),
            ("mov x, ~y", 0xa02a),
            ("mov isr, ::osr", 0xa0d7),
            ("mov pins, !null", 0xa00b),
            ("mov osr, status", 0xa0e5),
            ("mov pindirs, x", 0xa061),
            ("mov exec, x", 0xa081),
            ("nop", 0xa042),
            ("mov rxfifo[y], isr", 0x8010),
            ("mov rxfifo[2], isr", 0x801a),
            ("mov osr, rxfifo[y]", 0x8090),
            ("mov osr, rxfifo[3]", 0x809b),
            ("irq 3", 0xc003),
            ("irq set 3", 0xc003),
            ("irq nowait 3", 0xc003),
            ("irq wait 3", 0xc023),
            ("irq clear 0 rel", 0xc050),
            ("irq next 3", 0xc01b),
            ("irq prev wait 1", 0xc029),
            ("set x, 31", 0xe03f),
            ("set y 0", 0xe040),
            ("SET PINDIRS, 0b11", 0xe083),
            (".word 0xbeef", 0xbeef),
        ];
        for (source, expected) in cases {
            let fifo = if source.contains("rxfifo[y], isr") || source.contains("rxfifo[2]") {
                ".fifo txput\n"
            } else if source.contains("rxfifo") {
                ".fifo txget\n"
            } else {
                ""
            };
            assert_eq!(words(&format!("{fifo}{source}")), [*expected], "{source}");
        }
    }

    #[test]
    fn side_set_and_delay_share_five_bits() {
        assert_eq!(words(".side_set 1 opt\nset pins, 1 side 1 [3]"), [0xfb01]);
        assert_eq!(words(".side_set 1 opt\nset pins, 1 [3]"), [0xe301]);
        assert_eq!(words(".side_set 2\nnop [1] side 3"), [0xb942]);
        assert_eq!(words(".side_set 5\nnop side 31"), [0xbf42]);
        assert_eq!(words(".side_set 2 opt pindirs\nnop side 2"), [0xb842]);
        assert_eq!(words("nop [31]"), [0xbf42]);
        assert!(error(".side_set 2\nnop side 3 [8]", 1).contains("0 to 7, since side-set takes 2"));
        assert!(error(".side_set 1\nnop", 1).contains("needs `side`"));
        assert!(error("nop side 1", 1).contains("needs a .side_set"));
        assert!(error(".side_set 1\nnop side 2", 1).contains("side-set value must be 0 to 1"));
        assert!(error(".side_set 5 opt\nnop", 1).contains("1 to 4"));
        assert!(error("nop\n.side_set 1 opt", 1).contains("before the first instruction"));
    }

    #[test]
    fn symbols_expressions_and_wrap() {
        let program = assemble(
            "start:\n  set x, (N + 1)\n.wrap_target\nloop:\n  jmp x-- loop [D]\n  jmp end\n.wrap\nend:\n  jmp start\n.define N ((1 << 3) - 2)\n.define D (::0x80000000 * 2 | 1)",
            0,
        )
        .unwrap();
        assert_eq!(program.instructions, [0xe027, 0x0141, 0x0003, 0x0000]);
        assert_eq!((program.wrap_target, program.wrap), (1, 2));
        // pioasm's precedence: & binds tighter than +, << looser than +.
        assert_eq!(words(".define A 1 + 2 & 3\nset x, A"), [0xe023]);
        assert_eq!(words(".define A 1 << 2 + 1\nset x, A"), [0xe028]);
        assert_eq!(words(".define A -2 + 5\nset x, A"), [0xe023]);
        assert!(error("set x, B\n.define B B", 1).contains("in terms of itself"));
        assert!(error("set x, nowhere", 1).contains("undefined symbol `nowhere`"));
        assert!(error("set x, (1 / 0)", 1).contains("division by zero"));
        assert!(error("a:\na: nop", 1).contains("already defined on line 1"));
        assert!(error("jmp 2\nnop", 1).contains("outside the program's 2 instructions"));
        assert!(error("x: nop", 1).contains("keyword"));
    }

    #[test]
    fn directives_configure_the_state_machine() {
        let program = assemble(
            ".program configured\n.origin 4\n.clock_div 2.5\n.fifo putget\n.mov_status irq next set 3\n.in 5 left manual 8\n.out 3 right auto\n.set 2\nmov rxfifo[0], isr\nmov osr, rxfifo[1]\n",
            1,
        )
        .unwrap();
        assert_eq!(program.origin, Some(4));
        assert_eq!(program.clock_divider, 640);
        assert_eq!(program.fifo, Fifo::PutGet);
        assert_eq!(program.status, Status { select: 2, n: 19 });
        assert_eq!(
            program.input,
            Shift {
                count: 5,
                right: false,
                auto: false,
                threshold: 8
            }
        );
        assert_eq!(
            program.output,
            Shift {
                count: 3,
                right: true,
                auto: true,
                threshold: 32
            }
        );
        assert_eq!((program.set_count, program.version), (2, 1));
        let status = assemble(".mov_status rxfifo < 2\nnop", 0).unwrap();
        assert_eq!(status.status, Status { select: 1, n: 2 });
        assert_eq!(status.version, 0);
        assert!(error(".origin 30\nnop\nnop\nnop", 1).contains("past the end"));
        assert!(error(".fifo tx\npush", 1).contains("needs .fifo txrx or rx"));
        assert!(error(".fifo txget\n.in 32 auto\nnop", 1).contains("needs .fifo txrx, tx, or rx"));
        let autopush = assemble(".fifo rx\n.in 32 right auto 24\nin pins, 8", 0).unwrap();
        assert!(autopush.input.auto && autopush.input.threshold == 24);
        assert!(error("mov rxfifo[y], isr", 1).contains("needs .fifo txput or putget"));
        assert!(error(".fifo rx\n.fifo tx\nnop", 1).contains(".fifo is already given on line 1"));
        assert!(error(".clock_div 0.5\nnop", 1).contains("at least 1"));
        assert!(error(".wrap\nnop", 1).contains("after an instruction"));
        assert!(error("nop\n.wrap_target", 1).contains("before an instruction"));
        assert!(error(".frobnicate\nnop", 1).contains("unknown directive"));
        assert!(error(".program a\nnop\n.program b\nnop", 1).contains("takes one program"));
        assert!(error("nop\n.program late", 1).contains("before the program's instructions"));
        assert!(error("", 1).contains("at least one instruction"));
        let long = "nop\n".repeat(33);
        assert!(error(&long, 1).contains("at most 32"));
    }

    #[test]
    fn rp2350_features_need_pio_version_1() {
        for source in [
            "mov pindirs, x",
            "irq next 1",
            "wait 1 irq prev 2",
            "wait 1 jmppin",
            ".fifo txget\nmov osr, rxfifo[y]",
            ".mov_status irq set 1\nnop",
            ".in 4\nnop",
        ] {
            let message = error(source, 0);
            assert!(
                message.contains("needs PIO version 1"),
                "{source}: {message}"
            );
            assert!(message.contains("RP2040"), "{source}: {message}");
            assert_eq!(assemble(source, 1).unwrap().version, 1, "{source}");
        }
        assert!(error(".pio_version 0\nmov pindirs, x", 1).contains("declares .pio_version 0"));
        assert!(error(".pio_version rp2350\nnop", 0).contains("has version 0"));
        assert_eq!(assemble(".pio_version 1\nnop", 1).unwrap().version, 0);
    }

    #[test]
    fn errors_point_at_their_line_and_columns() {
        let failure = assemble("nop\n  set x, 32\n", 1).unwrap_err();
        assert_eq!((failure.line, failure.start, failure.end), (1, 9, 11));
        let failure = assemble("nop /* a\ncomment */\njmp 5", 1).unwrap_err();
        assert_eq!((failure.line, failure.start, failure.end), (2, 4, 5));
        // A block comment joins its lines, as in pioasm.
        assert!(error("nop /* a\ncomment */ jmp 0", 1).contains("one directive or instruction"));
        let failure = assemble("jmp nowhere\n", 1).unwrap_err();
        assert_eq!((failure.line, failure.start, failure.end), (0, 4, 11));
        let failure = assemble("set x, 1 extra\n", 1).unwrap_err();
        assert_eq!((failure.line, failure.start), (0, 9));
        assert!(failure.message.contains("one directive or instruction"));
        assert!(error("set x, -1", 1).contains("parentheses"));
        assert!(error("frob x", 1).contains("unknown instruction `frob`"));
        assert!(error("in status, 1", 1).contains("expected an IN source"));
        assert!(error("mov rxfifo[y], x", 1).contains("from `isr`"));
        assert!(error("set x, @", 1).contains("unexpected character"));
    }
}
