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
        )
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
        }
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
}
#[derive(Clone, Debug)]
pub struct Struct {
    pub name: String,
    pub public: bool,
    pub generics: Vec<String>,
    pub fields: Vec<Field>,
    pub span: Span,
    pub repr_c: bool,
    /// Explicit unsafe contracts for moving ownership / sharing access across
    /// native threads. Checked borrows are never erased by these contracts.
    pub unsafe_send: bool,
    pub unsafe_sync: bool,
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
    pub variants: Vec<Variant>,
    pub span: Span,
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
    pub generics: Vec<String>,
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
    pub mutable: bool,
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
