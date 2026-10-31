//! Source syntax shared by the parser, semantic analysis, and LLVM backend.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// Integer constant syntax retained until package names and target width are known.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LengthExpr {
    Int(u64, Option<Box<Type>>),
    Name(String),
    Unary(UnaryOp, Box<LengthExpr>),
    Binary(BinaryOp, Box<LengthExpr>, Box<LengthExpr>),
    Cast(Box<LengthExpr>, Box<Type>),
}
impl LengthExpr {
    pub fn from_expr(e: Expr) -> Result<Self, crate::diagnostic::Diagnostic> {
        let invalid = || {
            crate::diagnostic::Diagnostic::new(
                e.span,
                "array length must be an integer constant expression",
            )
        };
        Ok(match e.kind {
            ExprKind::Int(n, t) => Self::Int(n, t.map(Box::new)),
            ExprKind::Name(n) => Self::Name(n),
            ExprKind::Field(base, field) => match Self::from_expr(*base)? {
                Self::Name(base) => Self::Name(format!("{base}.{field}")),
                _ => return Err(invalid()),
            },
            ExprKind::Unary(op, value) => Self::Unary(op, Box::new(Self::from_expr(*value)?)),
            ExprKind::Binary(op, a, b) => Self::Binary(
                op,
                Box::new(Self::from_expr(*a)?),
                Box::new(Self::from_expr(*b)?),
            ),
            ExprKind::Cast(value, ty) => {
                Self::Cast(Box::new(Self::from_expr(*value)?), Box::new(ty))
            }
            _ => return Err(invalid()),
        })
    }
    pub fn expression(&self, span: Span) -> Expr {
        Expr::new(
            match self {
                Self::Int(n, t) => ExprKind::Int(*n, t.as_deref().cloned()),
                Self::Name(n) => ExprKind::Name(n.clone()),
                Self::Unary(op, value) => ExprKind::Unary(*op, Box::new(value.expression(span))),
                Self::Binary(op, a, b) => ExprKind::Binary(
                    *op,
                    Box::new(a.expression(span)),
                    Box::new(b.expression(span)),
                ),
                Self::Cast(value, ty) => {
                    ExprKind::Cast(Box::new(value.expression(span)), *ty.clone())
                }
            },
            span,
        )
    }
    pub fn text(&self) -> String {
        match self {
            Self::Int(n, t) => {
                format!("{n}{}", t.as_ref().map_or(String::new(), |t| t.to_string()))
            }
            Self::Name(n) => n.clone(),
            Self::Unary(op, e) => format!(
                "{}({})",
                match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "!",
                    UnaryOp::BitNot => "~",
                    UnaryOp::Borrow => "&",
                    UnaryOp::BorrowMut => "&mut ",
                    UnaryOp::Deref => "*",
                },
                e.text()
            ),
            Self::Binary(op, a, b) => format!(
                "({} {} {})",
                a.text(),
                match op {
                    BinaryOp::Add => "+",
                    BinaryOp::Sub => "-",
                    BinaryOp::Mul => "*",
                    BinaryOp::Div => "/",
                    BinaryOp::Rem => "%",
                    BinaryOp::Eq => "==",
                    BinaryOp::Ne => "!=",
                    BinaryOp::Lt => "<",
                    BinaryOp::Le => "<=",
                    BinaryOp::Gt => ">",
                    BinaryOp::Ge => ">=",
                    BinaryOp::And => "&&",
                    BinaryOp::Or => "||",
                    BinaryOp::BitAnd => "&",
                    BinaryOp::BitOr => "|",
                    BinaryOp::BitXor => "^",
                    BinaryOp::Shl => "<<",
                    BinaryOp::Shr => ">>",
                },
                b.text()
            ),
            Self::Cast(e, ty) => format!("({} as {ty})", e.text()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Unknown,
    Void,
    Bool,
    Int {
        signed: bool,
        bits: u32,
    }, // bits=0 means pointer-sized
    Float(u32),
    Str,
    Array(usize, Box<Type>),
    ArrayExpr(Box<LengthExpr>, Box<Type>),
    Slice(bool, Box<Type>),
    Ref(bool, Box<Type>),
    Raw(bool, Box<Type>),
    Named(String),
    Generic(String, Vec<Type>),
    Result(Box<Type>, Box<Type>),
    Option(Box<Type>),
    /// Opaque storage with the layout of T; never implicitly drops its contents.
    MaybeUninit(Box<Type>),
    /// A value of an unknown type implementing the named interface. It has no
    /// fixed size, so it only appears behind a reference: `&dyn I`, `&mut dyn I`.
    Dyn(String),
    /// `fn(A, B) -> R`: a plain function pointer with no captured state.
    Fn(Vec<Type>, Box<Type>),
    /// `dyn fn(A, B) -> R`: an unknown callable, which may capture locals. It
    /// has no fixed size, so it only appears behind a reference: `&dyn fn(..)`
    /// and `&mut dyn fn(..)` pair code with a borrowed environment.
    DynFn(Vec<Type>, Box<Type>),
    /// `extern "C" fn(A, B) -> R`: the address of code using the platform C
    /// calling convention, such as a pointer `dlsym`, `vkGetInstanceProcAddr`
    /// or a COM vtable supplies. Calling one is unsafe.
    ExternFn(Vec<Type>, Box<Type>),
}
impl Type {
    pub fn isize() -> Self {
        Self::Int {
            signed: true,
            bits: 0,
        }
    }
    pub fn usize() -> Self {
        Self::Int {
            signed: false,
            bits: 0,
        }
    }
    pub fn u8() -> Self {
        Self::Int {
            signed: false,
            bits: 8,
        }
    }
    pub fn is_integer(&self) -> bool {
        matches!(self, Self::Int { .. })
    }
    pub fn is_numeric(&self) -> bool {
        self.is_integer() || matches!(self, Self::Float(_))
    }
    pub fn is_copy(&self) -> bool {
        matches!(
            self,
            Self::Void
                | Self::Bool
                | Self::Int { .. }
                | Self::Float(_)
                | Self::Str
                | Self::Ref(false, _)
                | Self::Slice(false, _)
                | Self::Raw(..)
                | Self::Fn(..)
                | Self::ExternFn(..)
        )
    }
    /// The signature of a closure reference `&dyn fn(..)` or `&mut dyn fn(..)`.
    pub fn closure_signature(&self) -> Option<(bool, &[Type], &Type)> {
        match self {
            Self::Ref(mutable, inner) => match inner.as_ref() {
                Self::DynFn(params, ret) => Some((*mutable, params, ret)),
                _ => None,
            },
            _ => None,
        }
    }
    /// The signature a call of this value uses, looking through references to
    /// function pointers and closure references.
    pub fn call_signature(&self) -> Option<(&[Type], &Type)> {
        match self {
            Self::Ref(_, inner) if !matches!(inner.as_ref(), Self::DynFn(..)) => {
                inner.call_signature()
            }
            _ => self.callable_signature(),
        }
    }
    /// The signature of any callable value: a function pointer, a C function
    /// pointer or a closure reference.
    pub fn callable_signature(&self) -> Option<(&[Type], &Type)> {
        match self {
            Self::Fn(params, ret) | Self::ExternFn(params, ret) => Some((params, ret)),
            _ => self.closure_signature().map(|(_, p, r)| (p, r)),
        }
    }
    pub fn carries_borrow(&self) -> bool {
        match self {
            Self::Ref(..) | Self::Slice(..) | Self::Str => true,
            Self::Array(_, t) | Self::Option(t) => t.carries_borrow(),
            Self::Result(t, e) => t.carries_borrow() || e.carries_borrow(),
            _ => false,
        }
    }
}
impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => write!(f, "<inferred>"),
            Self::Void => write!(f, "void"),
            Self::Bool => write!(f, "bool"),
            Self::Int { signed, bits } => write!(
                f,
                "{}{}",
                if *signed { "i" } else { "u" },
                if *bits == 0 {
                    "size".to_owned()
                } else {
                    bits.to_string()
                }
            ),
            Self::Float(n) => write!(f, "f{n}"),
            Self::Str => write!(f, "&str"),
            Self::Array(n, t) => write!(f, "[{n}]{t}"),
            Self::ArrayExpr(n, t) => write!(f, "[{}]{t}", n.text()),
            Self::Slice(m, t) => write!(f, "&{}[{t}]", if *m { "mut " } else { "" }),
            Self::Ref(m, t) => write!(f, "&{}{t}", if *m { "mut " } else { "" }),
            Self::Raw(m, t) => write!(f, "*{} {t}", if *m { "mut" } else { "const" }),
            Self::Named(n) => write!(f, "{n}"),
            Self::Generic(n, ts) if tuple_arity(n) == Some(ts.len()) => {
                write!(f, "(")?;
                for (i, t) in ts.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{t}")?;
                }
                write!(f, ")")
            }
            Self::Generic(n, ts) => {
                write!(f, "{n}<")?;
                for (i, t) in ts.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{t}")?;
                }
                write!(f, ">")
            }
            Self::Result(t, e) => write!(f, "Result<{t}, {e}>"),
            Self::Option(t) => write!(f, "Option<{t}>"),
            Self::MaybeUninit(t) => write!(f, "MaybeUninit<{t}>"),
            Self::Dyn(n) => write!(f, "dyn {n}"),
            Self::Fn(params, ret) | Self::DynFn(params, ret) | Self::ExternFn(params, ret) => {
                if matches!(self, Self::DynFn(..)) {
                    write!(f, "dyn ")?;
                }
                if matches!(self, Self::ExternFn(..)) {
                    write!(f, "extern \"C\" ")?;
                }
                write!(f, "fn(")?;
                for (i, t) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{t}")?;
                }
                write!(f, ")")?;
                if **ret != Self::Void {
                    write!(f, " -> {ret}")?;
                }
                Ok(())
            }
        }
    }
}

/// The type of an inline assembly expression with these outputs.
pub fn asm_result(mut outputs: Vec<Type>) -> Type {
    match outputs.len() {
        0 => Type::Void,
        1 => outputs.pop().unwrap(),
        n => Type::Generic(tuple_name(n), outputs),
    }
}

/// Tuples are anonymous generic structs: `(A, B)` is `Type::Generic("#2", [A, B])`
/// and its elements are the public fields `0` and `1`. Source identifiers cannot
/// start with `#`, so the name never collides with, or is qualified like, a
/// declaration; generic specialization supplies the struct declaration.
pub fn tuple_name(arity: usize) -> String {
    format!("#{arity}")
}
/// The element count of a tuple's (possibly specialized) struct name.
pub fn tuple_arity(name: &str) -> Option<usize> {
    let digits = name.strip_prefix('#')?;
    let digits = digits.split_once('$').map_or(digits, |(arity, _)| arity);
    digits.parse().ok().filter(|arity| *arity >= 2)
}

/// Generic specialization encodes type arguments as hexadecimal after `$`.
/// Present the source notation, including nested specialized types and tuples,
/// wherever a user reads a type: diagnostics, hovers, and debug information.
pub fn source_names(text: &str) -> String {
    let Some(start) = text.find('$') else {
        return text.to_owned();
    };
    let suffix = &text[start + 1..];
    let end = suffix
        .find(|ch: char| !ch.is_ascii_hexdigit() && ch != '_')
        .unwrap_or(suffix.len());
    let arguments: Option<Vec<_>> = suffix[..end]
        .split('_')
        .map(|argument| {
            if argument.is_empty() || !argument.len().is_multiple_of(2) {
                return None;
            }
            let bytes: Option<Vec<_>> = (0..argument.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&argument[i..i + 2], 16).ok())
                .collect();
            String::from_utf8(bytes?)
                .ok()
                .map(|argument| source_names(&argument))
        })
        .collect();
    let prefix = &text[..start];
    let digits = prefix.len() - prefix.bytes().rev().take_while(u8::is_ascii_digit).count();
    let tuple = prefix[..digits]
        .strip_suffix('#')
        .filter(|_| digits < prefix.len());
    match (arguments, tuple) {
        (Some(arguments), Some(before))
            if tuple_arity(&prefix[digits - 1..]) == Some(arguments.len()) =>
        {
            format!(
                "{before}({}){}",
                arguments.join(", "),
                source_names(&suffix[end..])
            )
        }
        (Some(arguments), _) => format!(
            "{prefix}<{}>{}",
            arguments.join(", "),
            source_names(&suffix[end..])
        ),
        (None, _) => format!("{prefix}${}", source_names(suffix)),
    }
}

#[derive(Clone, Debug, Default)]
pub struct Program {
    pub package: String,
    pub imports: Vec<String>,
    /// Explicit local names, paired with their original import paths.
    pub import_aliases: Vec<(String, String)>,
    /// Compiler-generated aliases, normalized across sibling source files.
    pub implicit_import_aliases: Vec<(String, String)>,
    pub structs: Vec<Struct>,
    pub enums: Vec<Enum>,
    pub functions: Vec<Function>,
    pub constants: Vec<Constant>,
    pub interfaces: Vec<Interface>,
    pub aliases: Vec<TypeAlias>,
    /// The vector names a firmware build's chip accepts in `@interrupt`; set
    /// by package loading for a board or chip, `None` elsewhere.
    pub interrupt_vectors: Option<&'static [&'static str]>,
    /// Whether the chip's runtime calls `@interrupt` handlers as ordinary C
    /// functions (see `hardware::InterruptAbi::Dispatched`), so that RISC-V
    /// handlers are not machine-mode trap handlers.
    pub dispatched_interrupts: bool,
    /// The PIO version of a firmware build's chip, for `rp_pio.assemble`: 0
    /// on the RP2040, 1 on the RP2350, `None` on chips without PIO and
    /// outside firmware builds.
    pub pio_version: Option<u8>,
}
impl Program {
    /// Whether a value of `ty` holds an interior-mutable struct (see
    /// `Struct::interior_mutable`), directly or in a field, element, or payload.
    pub fn holds_interior_mutable(&self, ty: &Type) -> bool {
        fn visit<'a>(program: &'a Program, ty: &'a Type, seen: &mut Vec<&'a str>) -> bool {
            match ty {
                Type::Array(_, element) | Type::ArrayExpr(_, element) => {
                    visit(program, element, seen)
                }
                Type::Option(inner) | Type::MaybeUninit(inner) => visit(program, inner, seen),
                Type::Result(value, error) => {
                    visit(program, value, seen) || visit(program, error, seen)
                }
                Type::Named(name) | Type::Generic(name, _) => {
                    if seen.contains(&name.as_str()) {
                        return false;
                    }
                    seen.push(name);
                    if let Some(item) = program.structs.iter().find(|s| s.name == *name) {
                        return item.interior_mutable
                            || item.fields.iter().any(|f| visit(program, &f.ty, seen));
                    }
                    program
                        .enums
                        .iter()
                        .find(|e| e.name == *name)
                        .is_some_and(|item| {
                            item.variants
                                .iter()
                                .any(|v| v.fields.iter().any(|f| visit(program, &f.ty, seen)))
                        })
                }
                _ => false,
            }
        }
        visit(self, ty, &mut Vec::new())
    }
}
/// `type Name<T> = Type` introduces another name for a type. Aliases are
/// transparent: semantic analysis replaces every use with the aliased type.
#[derive(Clone, Debug)]
pub struct TypeAlias {
    pub name: String,
    pub public: bool,
    pub generics: Vec<String>,
    pub ty: Type,
    pub span: Span,
}
/// A generic parameter's required interfaces: `T: io.Reader + io.Writer`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bound {
    pub param: String,
    pub interfaces: Vec<String>,
    pub span: Span,
}
/// A named set of method signatures. Any type whose methods match satisfies it;
/// `dyn Name` values dispatch these methods through a table of function pointers.
#[derive(Clone, Debug)]
pub struct Interface {
    pub name: String,
    pub public: bool,
    /// Interfaces whose methods this one includes.
    pub parents: Vec<String>,
    /// Bodiless signatures; a receiver's `Self` is the placeholder `Type::Named("Self")`.
    pub methods: Vec<Function>,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct Struct {
    pub name: String,
    pub public: bool,
    pub generics: Vec<String>,
    pub bounds: Vec<Bound>,
    /// Interfaces the declaration states it implements, checked where declared.
    pub implements: Vec<(String, Span)>,
    pub fields: Vec<Field>,
    pub span: Span,
    pub repr_c: bool,
    /// Explicit unsafe contracts for moving ownership / sharing access across
    /// native threads. Checked borrows are never erased by these contracts.
    pub unsafe_send: bool,
    pub unsafe_sync: bool,
    /// Changes through shared references: `std/sync/atomic`'s `Atomic`, set
    /// by package loading. A non-mut static holding one is placed in writable
    /// memory, and a `const`, which is copied at each use, cannot hold one.
    pub interior_mutable: bool,
}
#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    pub ty: Type,
    pub public: bool,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct Enum {
    pub name: String,
    pub public: bool,
    pub generics: Vec<String>,
    pub bounds: Vec<Bound>,
    pub variants: Vec<Variant>,
    pub span: Span,
}
impl Enum {
    /// Variants whose only payload is a `source` value. `?` wraps a `source`
    /// error in this enum when exactly one such variant exists.
    pub fn wrapping_variants(&self, source: &Type) -> Vec<&Variant> {
        self.variants
            .iter()
            .filter(|v| matches!(v.fields.as_slice(), [field] if field.ty == *source))
            .collect()
    }
}
#[derive(Clone, Debug)]
pub struct Variant {
    pub name: String,
    pub fields: Vec<Field>,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    /// Defined in a dependency of this compilation unit; set by package loading.
    pub imported: bool,
    pub test: bool,
    pub ignore: Option<String>,
    pub public: bool,
    pub unsafe_: bool,
    pub extern_: bool,
    /// Reserved printing entry points; retained on fixed helpers for editor signatures.
    pub printing: Option<Printing>,
    /// `@compiler(pio)`: std/embedded/chip/rp_pio's `assemble`, whose calls
    /// are assembled at compile time into a `Program` literal.
    pub pio: bool,
    pub generics: Vec<String>,
    pub bounds: Vec<Bound>,
    /// Compiler-generated specialization: declared borrow sources may become
    /// borrow-free after type substitution and then contribute no dependencies.
    pub generic_instance: bool,
    pub params: Vec<Param>,
    pub ret: Type,
    pub ret_span: Span,
    pub from: Vec<String>,
    /// Checked dependency deposition: target followed by source parameters.
    pub stores: Vec<String>,
    /// A specialized method is callable only for borrow/Result-free types.
    pub requires_plain: Vec<Type>,
    pub from_span: Option<Span>,
    pub body: Option<Block>,
    pub span: Span,
    /// A closure body lifted out of its enclosing function: the number of
    /// leading parameters that reference captured bindings through its
    /// environment. Such functions use the closure calling convention.
    pub environment: Option<usize>,
    /// `@interrupt(VECTOR)`: the hardware runs this function for the named
    /// exception or interrupt. It is exported under the vector's symbol name
    /// and cannot be called from Dodo code.
    pub interrupt: Option<String>,
    /// `@link_name("symbol")`: the C symbol an extern declaration binds in
    /// place of its own name. Several declarations may bind one symbol with
    /// different signatures, as Objective-C does with `objc_msgSend`.
    pub link_name: Option<String>,
    pub placement: Placement,
}
impl Function {
    /// The C symbol of an extern function: its `@link_name`, or its own name.
    pub fn foreign_symbol(&self) -> &str {
        self.link_name
            .as_deref()
            .unwrap_or_else(|| self.name.rsplit('.').next().unwrap_or(&self.name))
    }
}
/// Linker placement chosen by `@section("name")` and `@align(N)` on functions
/// and statics.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Placement {
    pub section: Option<String>,
    /// A power of two, in bytes.
    pub align: Option<u32>,
}
impl Placement {
    pub fn is_empty(&self) -> bool {
        self.section.is_none() && self.align.is_none()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Printing {
    Print,
    Println,
    Printf,
}
#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct Constant {
    pub name: String,
    pub ty: Type,
    pub value: Expr,
    pub public: bool,
    /// Declared with `static` rather than `const`.
    pub static_: bool,
    pub mutable: bool,
    pub placement: Placement,
    pub span: Span,
}
pub type Block = Vec<Stmt>;
#[derive(Clone, Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub enum StmtKind {
    Let {
        name: String,
        ty: Type,
        value: Option<Expr>,
        constant: bool,
        mutable: bool,
    },
    Assign {
        target: Expr,
        op: Option<BinaryOp>,
        value: Expr,
    },
    Expr(Expr),
    // Internal exit from a value block, distinct from a function return.
    Yield(Expr),
    Return(Option<Expr>),
    If {
        condition: Expr,
        then_block: Block,
        else_block: Block,
    },
    IfLet {
        pattern: Pattern,
        value: Expr,
        then_block: Block,
        else_block: Block,
    },
    LetPattern {
        pattern: Pattern,
        ty: Type,
        value: Expr,
        else_block: Option<Block>,
    },
    For {
        init: Option<Box<Stmt>>,
        condition: Option<Expr>,
        step: Option<Box<Stmt>>,
        body: Block,
    },
    ForEach {
        index: Option<String>,
        name: String,
        /// Destructure a shared element reference and copy its copyable value.
        copy: bool,
        iterable: Expr,
        body: Block,
    },
    Match {
        value: Expr,
        arms: Vec<MatchArm>,
    },
    Break,
    Continue,
    Block(Block),
    Unsafe(Block),
}
#[derive(Clone, Debug)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Block,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub enum Pattern {
    Wildcard,
    Binding(String),
    Bool(bool),
    Int(u64),
    Variant(String, Vec<Pattern>),
    Struct(String, Vec<(String, Pattern)>, bool),
    Range(u64, u64, bool),
    Or(Vec<Pattern>),
}
impl Pattern {
    /// Binding names introduced by this pattern. Alternatives share one scope,
    /// so semantic analysis checks that every alternative binds this same set.
    pub fn bindings(&self) -> Vec<String> {
        match self {
            Self::Binding(name) => vec![name.clone()],
            Self::Variant(_, fields) => fields.iter().flat_map(Self::bindings).collect(),
            Self::Struct(_, fields, _) => fields.iter().flat_map(|(_, p)| p.bindings()).collect(),
            Self::Or(alternatives) => alternatives.first().map_or_else(Vec::new, Self::bindings),
            Self::Wildcard | Self::Bool(_) | Self::Int(_) | Self::Range(..) => Vec::new(),
        }
    }
    pub fn rename_bindings(&mut self, rename: &impl Fn(&str) -> String) {
        match self {
            Self::Binding(name) => *name = rename(name),
            Self::Variant(_, fields) => fields.iter_mut().for_each(|p| p.rename_bindings(rename)),
            Self::Struct(_, fields, _) => fields
                .iter_mut()
                .for_each(|(_, p)| p.rename_bindings(rename)),
            Self::Or(alternatives) => alternatives
                .iter_mut()
                .for_each(|p| p.rename_bindings(rename)),
            Self::Wildcard | Self::Bool(_) | Self::Int(_) | Self::Range(..) => {}
        }
    }
}
#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
    pub ty: Type,
}
impl Expr {
    pub fn new(kind: ExprKind, span: Span) -> Self {
        Self {
            kind,
            span,
            ty: Type::Unknown,
        }
    }
}
#[derive(Clone, Debug)]
pub enum ExprKind {
    Int(u64, Option<Type>),
    Float(f64, Option<Type>),
    Bool(bool),
    String(Vec<u8>, bool),
    Name(String),
    Array(Type, Vec<Expr>),
    Repeat(Box<Expr>, Type),
    Constant(Box<Expr>, Type),
    ValueBlock(Block),
    Range(Box<Expr>, Box<Expr>),
    Slice {
        base: Box<Expr>,
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        mutable: bool,
    },
    Struct(String, Vec<(String, Expr)>),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Call {
        name: String,
        type_args: Vec<Type>,
        args: Vec<Expr>,
    },
    MethodCall {
        receiver: Box<Expr>,
        name: String,
        args: Vec<Expr>,
    },
    Field(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Cast(Box<Expr>, Type),
    Try(Box<Expr>),
    Unwrap(Box<Expr>),
    /// `fn(x: T) -> R { body }`, an anonymous function that may capture locals.
    Closure(Box<Closure>),
    /// A call through a function value: `callback(x)`, `(table[i])(x)`.
    Invoke {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    /// `asm("template", operands...)`: inline assembly, valid only in unsafe code.
    Asm(Box<InlineAsm>),
}
/// Inline assembly. The template refers to operands as `{0}`, `{1}`, ... in
/// the order written (or `{}` for the next one), with an optional LLVM operand
/// modifier: `{0:w}`. `{{` and `}}` are literal braces. The expression's value
/// is `void` without outputs, the output's type with one, and a tuple of the
/// output types, in order, with several.
#[derive(Clone, Debug)]
pub struct InlineAsm {
    pub template: String,
    pub operands: Vec<AsmOperand>,
    /// Register names and `memory`, `cc`.
    pub clobbers: Vec<String>,
    /// `options(pure)`: no side effects beyond its outputs, so it may be
    /// removed or merged; requires an output.
    pub pure: bool,
    /// `options(nomem)`: reads and writes no memory the program can see.
    pub nomem: bool,
    /// Set by generic specialization when there are several outputs: the
    /// concrete tuple type.
    pub tuple: Option<Type>,
    pub span: Span,
}
impl InlineAsm {
    /// Input values, in operand order: `in` and `inout` operands.
    pub fn inputs(&self) -> impl Iterator<Item = &Expr> {
        self.operands
            .iter()
            .filter_map(|operand| match &operand.kind {
                AsmOperandKind::In(value) | AsmOperandKind::InOut(value) => Some(value),
                AsmOperandKind::Out(_) => None,
            })
    }
    pub fn inputs_mut(&mut self) -> impl Iterator<Item = &mut Expr> {
        self.operands
            .iter_mut()
            .filter_map(|operand| match &mut operand.kind {
                AsmOperandKind::In(value) | AsmOperandKind::InOut(value) => Some(value),
                AsmOperandKind::Out(_) => None,
            })
    }
    /// Declared `out` types, with the span of their operand.
    pub fn output_types_mut(&mut self) -> impl Iterator<Item = (&mut Type, Span)> {
        self.operands
            .iter_mut()
            .filter_map(|operand| match &mut operand.kind {
                AsmOperandKind::Out(ty) => Some((ty, operand.span)),
                _ => None,
            })
    }
    /// The types the expression returns, in order: `out` types and the
    /// checked types of `inout` values.
    pub fn outputs(&self) -> Vec<Type> {
        self.operands
            .iter()
            .filter_map(|operand| match &operand.kind {
                AsmOperandKind::Out(ty) => Some(ty.clone()),
                AsmOperandKind::InOut(value) => Some(value.ty.clone()),
                AsmOperandKind::In(_) => None,
            })
            .collect()
    }
}
#[derive(Clone, Debug)]
pub struct AsmOperand {
    pub kind: AsmOperandKind,
    pub register: AsmRegister,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub enum AsmOperandKind {
    /// `in(reg) value`
    In(Expr),
    /// `out(reg) Type`: a value the expression returns.
    Out(Type),
    /// `inout(reg) value`: passed in, and its final value returned.
    InOut(Expr),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AsmRegister {
    /// `reg`: any general-purpose register the compiler picks.
    Any,
    /// `"r0"`: exactly this register.
    Named(String),
}
#[derive(Clone, Debug)]
pub struct Closure {
    /// Parameter types are `Unknown` when omitted and taken from the expected type.
    pub params: Vec<Param>,
    /// `Unknown` when omitted: taken from the expected type or the body.
    pub ret: Type,
    pub ret_span: Span,
    /// Final expressions are `Yield` statements until the result type is known.
    pub body: Block,
    /// Set by semantic analysis: the lifted function holding the body.
    pub function: String,
    /// Set by semantic analysis: captured bindings, in environment order.
    pub captures: Vec<Capture>,
}
/// A local binding a closure reads (`mutable == false`) or modifies through its environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capture {
    pub name: String,
    pub mutable: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
    Borrow,
    BorrowMut,
    Deref,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}
