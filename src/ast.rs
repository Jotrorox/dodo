//! Source syntax shared by the parser, semantic analysis, and LLVM backend.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Unknown,
    Void,
    Bool,
    Int { signed: bool, bits: u32 }, // bits=0 means pointer-sized
    Float(u32),
    Str,
    Array(usize, Box<Type>),
    Slice(bool, Box<Type>),
    Ref(bool, Box<Type>),
    Raw(bool, Box<Type>),
    Named(String),
    Generic(String, Vec<Type>),
    Result(Box<Type>, Box<Type>),
    Option(Box<Type>),
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
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Program {
    pub package: String,
    pub imports: Vec<String>,
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
    pub public: bool,
    pub unsafe_: bool,
    pub extern_: bool,
    pub generics: Vec<String>,
    pub params: Vec<Param>,
    pub ret: Type,
    pub from: Vec<String>,
    pub body: Option<Block>,
    pub span: Span,
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
    },
    Assign {
        target: Expr,
        op: Option<BinaryOp>,
        value: Expr,
    },
    Expr(Expr),
    Return(Option<Expr>),
    If {
        condition: Expr,
        then_block: Block,
        else_block: Block,
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
    pub body: Block,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub enum Pattern {
    Wildcard,
    Bool(bool),
    Int(u64),
    Variant(String, Vec<String>),
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
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
    Borrow,
    BorrowMut,
    Deref,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
