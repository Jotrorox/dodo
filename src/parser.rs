//! Recursive-descent declarations and statements with a Pratt expression parser.
use crate::ast::*;
use crate::diagnostic::Diagnostic;
use crate::lexer::{Token, TokenKind, lex};

type ParseResult<T> = Result<T, Diagnostic>;

pub fn parse(source: &str) -> ParseResult<Program> {
    Parser {
        tokens: lex(source)?,
        cursor: 0,
        soft_newlines: 0,
        self_type: None,
        angle_splits: Vec::new(),
        depth: 0,
    }
    .program()
}

struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
    soft_newlines: usize,
    self_type: Option<Type>,
    // A small undo log makes speculative generic parsing linear in the input,
    // without cloning the token stream for every expression statement.
    angle_splits: Vec<(usize, Token)>,
    depth: usize,
}

#[derive(Default)]
struct Modifiers {
    public: bool,
    unsafe_: bool,
    extern_: bool,
    repr_c: bool,
}

impl Parser {
    fn nested<T>(&mut self, operation: impl FnOnce(&mut Self) -> ParseResult<T>) -> ParseResult<T> {
        if self.depth >= 128 {
            return Err(self.error("syntax nesting exceeds the supported limit of 128 levels"));
        }
        self.depth += 1;
        let result = operation(self);
        self.depth -= 1;
        result
    }
    fn token(&self) -> &Token {
        &self.tokens[self.cursor.min(self.tokens.len() - 1)]
    }
    fn span(&self) -> Span {
        self.token().span
    }
    fn previous_end(&self) -> usize {
        self.tokens[self.cursor.saturating_sub(1)].span.end
    }
    fn since(&self, start: usize) -> Span {
        Span {
            start,
            end: self.previous_end(),
        }
    }
    fn error(&self, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(self.span(), message)
    }
    fn at(&self, text: &str) -> bool {
        matches!(&self.token().kind, TokenKind::Symbol(s) if *s == text)
            || matches!(&self.token().kind, TokenKind::Ident(s) if s == text)
    }
    fn look(&self, distance: usize, text: &str) -> bool {
        self.tokens
            .get(self.cursor + distance)
            .is_some_and(|token| {
                matches!(&token.kind, TokenKind::Symbol(s) if *s == text)
                    || matches!(&token.kind, TokenKind::Ident(s) if s == text)
            })
    }
    fn bump(&mut self) -> Token {
        let token = self.token().clone();
        if !matches!(token.kind, TokenKind::Eof) {
            self.cursor += 1;
        }
        token
    }
    fn eat(&mut self, text: &str) -> bool {
        if self.at(text) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, text: &str) -> ParseResult<()> {
        if self.eat(text) {
            Ok(())
        } else {
            Err(self.error(format!("expected `{text}`")))
        }
    }
    fn eof(&self) -> bool {
        matches!(self.token().kind, TokenKind::Eof)
    }
    fn newline(&self) -> bool {
        matches!(self.token().kind, TokenKind::Newline)
    }
    fn newlines(&mut self) {
        while self.newline() {
            self.bump();
        }
    }
    fn separators(&mut self) {
        while self.newline() || self.at(";") {
            self.bump();
        }
    }
    fn end_statement(&mut self) -> ParseResult<()> {
        if self.newline() || self.at(";") {
            self.separators();
            Ok(())
        } else if self.at("}") || self.eof() {
            Ok(())
        } else {
            Err(self.error("expected a newline or `;` after the statement"))
        }
    }
    fn identifier(&mut self) -> ParseResult<String> {
        match &self.token().kind {
            TokenKind::Ident(name) if !reserved(name) => {
                let name = name.clone();
                self.bump();
                Ok(name)
            }
            _ => Err(self.error("expected an identifier")),
        }
    }
    fn qualified_name(&mut self) -> ParseResult<String> {
        let mut name = self.identifier()?;
        while self.eat(".") {
            name.push('.');
            name.push_str(&self.identifier()?);
        }
        Ok(name)
    }
    fn program(mut self) -> ParseResult<Program> {
        self.separators();
        self.expect("package")?;
        let mut program = Program {
            package: self.identifier()?,
            ..Program::default()
        };
        self.end_statement()?;
        while !self.eof() {
            self.separators();
            if self.eof() {
                break;
            }
            if self.eat("import") {
                match self.bump() {
                    Token {
                        kind: TokenKind::String(bytes, false),
                        span,
                    } => {
                        let path = String::from_utf8(bytes)
                            .map_err(|_| Diagnostic::new(span, "import path must be UTF-8"))?;
                        if path.is_empty() {
                            return Err(Diagnostic::new(span, "import path cannot be empty"));
                        }
                        program.imports.push(path);
                    }
                    token => {
                        return Err(Diagnostic::new(token.span, "expected a string import path"));
                    }
                }
                self.end_statement()?;
                continue;
            }
            let start = self.span().start;
            let mods = self.modifiers()?;
            if self.at("struct") {
                if mods.unsafe_ || mods.extern_ {
                    return Err(self.error("structs cannot be unsafe or extern"));
                }
                self.struct_decl(&mut program, mods, start)?;
            } else if self.at("enum") {
                if mods.unsafe_ || mods.extern_ || mods.repr_c {
                    return Err(self.error("enum declarations only support the `pub` modifier"));
                }
                program.enums.push(self.enum_decl(mods.public, start)?);
            } else if self.at("fn") {
                if mods.repr_c {
                    return Err(self.error("@repr(C) applies to structs"));
                }
                program.functions.push(self.function(mods, start)?);
            } else if self.at("const") || self.at("static") {
                if mods.unsafe_ || mods.extern_ || mods.repr_c {
                    return Err(self.error("constant and static declarations only support `pub`"));
                }
                program.constants.push(self.constant(mods.public, start)?);
            } else {
                return Err(
                    self.error("expected `fn`, `struct`, `enum`, `const`, or `static` declaration")
                );
            }
        }
        Ok(program)
    }
    fn modifiers(&mut self) -> ParseResult<Modifiers> {
        let mut mods = Modifiers::default();
        loop {
            if self.eat("pub") {
                if mods.public {
                    return Err(self.error("duplicate `pub` modifier"));
                }
                mods.public = true;
            } else if self.eat("unsafe") {
                if mods.unsafe_ {
                    return Err(self.error("duplicate `unsafe` modifier"));
                }
                mods.unsafe_ = true;
            } else if self.eat("extern") {
                if mods.extern_ {
                    return Err(self.error("duplicate `extern` modifier"));
                }
                match self.bump() {
                    Token {
                        kind: TokenKind::String(abi, false),
                        ..
                    } if abi == b"C" => mods.extern_ = true,
                    token => {
                        return Err(Diagnostic::new(
                            token.span,
                            "only the `extern \"C\"` ABI is supported",
                        ));
                    }
                }
            } else if self.eat("@") {
                let name = self.identifier()?;
                if name != "repr" {
                    return Err(self.error(format!(
                        "unknown attribute `@{name}`; supported attribute: @repr(C)"
                    )));
                }
                if mods.repr_c {
                    return Err(self.error("duplicate @repr(C) attribute"));
                }
                self.expect("(")?;
                self.expect("C")?;
                self.expect(")")?;
                mods.repr_c = true;
                self.newlines();
            } else {
                break;
            }
        }
        Ok(mods)
    }
    fn generic_params(&mut self) -> ParseResult<Vec<String>> {
        if !self.eat("<") {
            return Ok(Vec::new());
        }
        self.newlines();
        let mut params = Vec::new();
        loop {
            let name = self.identifier()?;
            if params.contains(&name) {
                return Err(self.error(format!("duplicate generic parameter `{name}`")));
            }
            params.push(name);
            self.newlines();
            if self.eat(",") {
                self.newlines();
                if self.at(">") {
                    break;
                }
            } else {
                break;
            }
        }
        self.close_angle()?;
        Ok(params)
    }
    fn close_angle(&mut self) -> ParseResult<()> {
        if self.eat(">") {
            return Ok(());
        }
        // Keep the second `>` available for an enclosing generic argument list.
        if self.at(">>") {
            self.angle_splits.push((self.cursor, self.token().clone()));
            self.tokens[self.cursor].kind = TokenKind::Symbol(">");
            self.tokens[self.cursor].span.start += 1;
            return Ok(());
        }
        Err(self.error("expected `>` after generic arguments"))
    }
    fn restore_angles(&mut self, checkpoint: usize) {
        while self.angle_splits.len() > checkpoint {
            if let Some((index, original)) = self.angle_splits.pop() {
                self.tokens[index] = original;
            }
        }
    }
    fn type_args(&mut self) -> ParseResult<Vec<Type>> {
        self.expect("<")?;
        self.newlines();
        let mut types = vec![self.ty()?];
        self.newlines();
        while self.eat(",") {
            self.newlines();
            if self.at(">") || self.at(">>") {
                break;
            }
            types.push(self.ty()?);
            self.newlines();
        }
        self.close_angle()?;
        Ok(types)
    }
    fn ty(&mut self) -> ParseResult<Type> {
        self.nested(Self::type_result)
    }
    fn type_result(&mut self) -> ParseResult<Type> {
        let ty = self.type_atom()?;
        if self.eat("!") {
            Ok(Type::Result(Box::new(ty), Box::new(self.ty()?)))
        } else {
            Ok(ty)
        }
    }
    fn type_atom(&mut self) -> ParseResult<Type> {
        self.nested(Self::type_atom_inner)
    }
    fn type_atom_inner(&mut self) -> ParseResult<Type> {
        if self.eat("&") {
            let mutable = self.eat("mut");
            if !mutable && self.eat("str") {
                return Ok(Type::Str);
            }
            if self.at("[")
                && !matches!(
                    self.tokens.get(self.cursor + 1).map(|t| &t.kind),
                    Some(TokenKind::Int(..))
                )
            {
                self.bump();
                self.newlines();
                let inner = self.ty()?;
                self.newlines();
                self.expect("]")?;
                return Ok(Type::Slice(mutable, Box::new(inner)));
            }
            return Ok(Type::Ref(mutable, Box::new(self.type_atom()?)));
        }
        if self.eat("*") {
            let mutable = if self.eat("mut") {
                true
            } else {
                self.expect("const")?;
                false
            };
            return Ok(Type::Raw(mutable, Box::new(self.type_atom()?)));
        }
        if self.eat("[") {
            self.newlines();
            let Token {
                kind: TokenKind::Int(size, None),
                span,
            } = self.bump()
            else {
                return Err(
                    self.error("array lengths must be untyped nonnegative integer literals")
                );
            };
            let size = usize::try_from(size).map_err(|_| {
                Diagnostic::new(
                    span,
                    "array length exceeds the compiler host's address space",
                )
            })?;
            self.newlines();
            self.expect("]")?;
            return Ok(Type::Array(size, Box::new(self.type_atom()?)));
        }
        if self.eat("void") {
            return Ok(Type::Void);
        }
        let start = self.span();
        let name = self.qualified_name()?;
        if name == "Self" {
            return self.self_type.clone().ok_or_else(|| {
                Diagnostic::new(start, "`Self` is only valid inside a struct declaration")
            });
        }
        let primitive = match name.as_str() {
            "bool" => Some(Type::Bool),
            "isize" => Some(Type::isize()),
            "usize" => Some(Type::usize()),
            "u8" => Some(Type::u8()),
            "i8" => Some(Type::Int {
                signed: true,
                bits: 8,
            }),
            "u16" => Some(Type::Int {
                signed: false,
                bits: 16,
            }),
            "i16" => Some(Type::Int {
                signed: true,
                bits: 16,
            }),
            "u32" => Some(Type::Int {
                signed: false,
                bits: 32,
            }),
            "i32" => Some(Type::Int {
                signed: true,
                bits: 32,
            }),
            "u64" => Some(Type::Int {
                signed: false,
                bits: 64,
            }),
            "i64" => Some(Type::Int {
                signed: true,
                bits: 64,
            }),
            "f32" => Some(Type::Float(32)),
            "f64" => Some(Type::Float(64)),
            _ => None,
        };
        if let Some(primitive) = primitive {
            return Ok(primitive);
        }
        if self.at("<") {
            let args = self.type_args()?;
            return match name.as_str() {
                "Result" if args.len() == 2 => Ok(Type::Result(
                    Box::new(args[0].clone()),
                    Box::new(args[1].clone()),
                )),
                "Option" if args.len() == 1 => Ok(Type::Option(Box::new(args[0].clone()))),
                "Result" => Err(Diagnostic::new(start, "Result requires two type arguments")),
                "Option" => Err(Diagnostic::new(start, "Option requires one type argument")),
                _ => Ok(Type::Generic(name, args)),
            };
        }
        if name == "str" {
            return Err(Diagnostic::new(
                start,
                "strings use the borrowed type `&str`",
            ));
        }
        Ok(Type::Named(name))
    }
    fn struct_decl(
        &mut self,
        program: &mut Program,
        mods: Modifiers,
        start: usize,
    ) -> ParseResult<()> {
        self.expect("struct")?;
        let name = self.identifier()?;
        let generics = self.generic_params()?;
        self.self_type = Some(if generics.is_empty() {
            Type::Named(name.clone())
        } else {
            Type::Generic(
                name.clone(),
                generics.iter().cloned().map(Type::Named).collect(),
            )
        });
        self.newlines();
        self.expect("{")?;
        self.separators();
        let mut fields = Vec::new();
        while !self.eat("}") {
            if self.eof() {
                return Err(self.error("unclosed struct declaration; expected `}`"));
            }
            let field_start = self.span().start;
            let field_mods = self.modifiers()?;
            if self.at("fn") {
                if field_mods.repr_c {
                    return Err(self.error("@repr(C) applies to structs"));
                }
                let mut function = self.function(field_mods, field_start)?;
                function.name = format!("{name}.{}", function.name);
                let mut all_generics = generics.clone();
                for generic in &function.generics {
                    if all_generics.contains(generic) {
                        return Err(self.error(format!(
                            "method generic parameter `{generic}` shadows its struct parameter"
                        )));
                    }
                    all_generics.push(generic.clone());
                }
                function.generics = all_generics;
                program.functions.push(function);
            } else {
                if field_mods.unsafe_ || field_mods.extern_ || field_mods.repr_c {
                    return Err(self.error("struct fields only support the `pub` modifier"));
                }
                let ty = self.ty()?;
                let field_name = self.identifier()?;
                fields.push(Field {
                    name: field_name,
                    ty,
                    public: field_mods.public,
                    span: self.since(field_start),
                });
                if self.eat(",") {
                    self.separators();
                } else {
                    self.end_statement()?;
                }
            }
            self.separators();
        }
        self.self_type = None;
        program.structs.push(Struct {
            name,
            public: mods.public,
            generics,
            fields,
            span: self.since(start),
            repr_c: mods.repr_c,
        });
        Ok(())
    }
    fn enum_decl(&mut self, public: bool, start: usize) -> ParseResult<Enum> {
        self.expect("enum")?;
        let name = self.identifier()?;
        let generics = self.generic_params()?;
        self.newlines();
        self.expect("{")?;
        self.newlines();
        let mut variants = Vec::new();
        while !self.eat("}") {
            if self.eof() {
                return Err(self.error("unclosed enum declaration; expected `}`"));
            }
            let variant_start = self.span().start;
            let variant_name = self.identifier()?;
            let mut fields = Vec::new();
            if self.eat("(") {
                self.newlines();
                while !self.eat(")") {
                    let field_start = self.span().start;
                    let (field_name, ty) = if self.look(1, ":") {
                        let name = self.identifier()?;
                        self.expect(":")?;
                        (name, self.ty()?)
                    } else {
                        let ty = self.ty()?;
                        let name = if matches!(self.token().kind, TokenKind::Ident(_)) {
                            self.identifier()?
                        } else {
                            fields.len().to_string()
                        };
                        (name, ty)
                    };
                    fields.push(Field {
                        name: field_name,
                        ty,
                        public: true,
                        span: self.since(field_start),
                    });
                    self.newlines();
                    if self.eat(",") {
                        self.newlines();
                    } else {
                        self.expect(")")?;
                        break;
                    }
                }
            }
            variants.push(Variant {
                name: variant_name,
                fields,
                span: self.since(variant_start),
            });
            if self.eat(",") || self.newline() {
                self.newlines();
            } else if !self.at("}") {
                return Err(self.error("expected `,` or `}` after enum variant"));
            }
        }
        Ok(Enum {
            name,
            public,
            generics,
            variants,
            span: self.since(start),
        })
    }
    fn function(&mut self, mods: Modifiers, start: usize) -> ParseResult<Function> {
        self.expect("fn")?;
        let name = self.identifier()?;
        let generics = self.generic_params()?;
        self.expect("(")?;
        self.newlines();
        let mut params = Vec::new();
        while !self.eat(")") {
            let param_start = self.span().start;
            let name = self.identifier()?;
            self.expect(":")?;
            self.newlines();
            let ty = self.ty()?;
            params.push(Param {
                name,
                ty,
                span: self.since(param_start),
            });
            self.newlines();
            if self.eat(",") {
                self.newlines();
            } else {
                self.expect(")")?;
                break;
            }
        }
        self.expect("->").map_err(|d| {
            d.note("every function needs an explicit return type; use `-> void` for no result")
        })?;
        self.newlines();
        let ret = self.ty()?;
        let mut from = Vec::new();
        if self.eat("from") {
            self.expect("(")?;
            self.newlines();
            loop {
                from.push(if self.eat("static") {
                    "static".to_owned()
                } else {
                    self.identifier()?
                });
                self.newlines();
                if self.eat(",") {
                    self.newlines();
                } else {
                    break;
                }
            }
            self.expect(")")?;
        }
        // Foreign prototypes end at the line; an optional body may start on the next line.
        let saved = self.cursor;
        self.newlines();
        let body = if self.at("{") {
            Some(self.block()?)
        } else if mods.extern_ {
            self.cursor = saved;
            self.end_statement()?;
            None
        } else {
            return Err(self.error("expected a function body enclosed in braces"));
        };
        Ok(Function {
            name,
            public: mods.public,
            unsafe_: mods.unsafe_,
            extern_: mods.extern_,
            generics,
            params,
            ret,
            from,
            body,
            span: self.since(start),
        })
    }
    fn constant(&mut self, public: bool, start: usize) -> ParseResult<Constant> {
        let mutable = if self.eat("static") {
            self.eat("mut")
        } else {
            self.expect("const")?;
            false
        };
        let ty = self.ty()?;
        let name = self.identifier()?;
        self.expect("=")?;
        self.newlines();
        let value = self.expression(true)?;
        let span = self.since(start);
        self.end_statement()?;
        Ok(Constant {
            name,
            ty,
            value,
            public,
            mutable,
            span,
        })
    }
    fn block(&mut self) -> ParseResult<Block> {
        self.nested(Self::block_inner)
    }
    fn block_inner(&mut self) -> ParseResult<Block> {
        self.expect("{")?;
        let previous_soft = self.soft_newlines;
        self.soft_newlines = 0;
        let mut statements = Vec::new();
        self.separators();
        while !self.eat("}") {
            if self.eof() {
                return Err(self.error("unclosed block; expected `}`"));
            }
            statements.push(self.statement()?);
            self.separators();
        }
        self.soft_newlines = previous_soft;
        Ok(statements)
    }
    fn statement(&mut self) -> ParseResult<Stmt> {
        let start = self.span().start;
        let compound;
        let kind = if self.eat("return") {
            compound = false;
            StmtKind::Return(
                if self.newline() || self.at(";") || self.at("}") || self.eof() {
                    None
                } else {
                    Some(self.expression(true)?)
                },
            )
        } else if self.eat("break") {
            compound = false;
            StmtKind::Break
        } else if self.eat("continue") {
            compound = false;
            StmtKind::Continue
        } else if self.eat("if") {
            compound = true;
            self.if_statement()?
        } else if self.eat("for") {
            compound = true;
            self.for_statement()?
        } else if self.eat("match") {
            compound = true;
            self.match_statement()?
        } else if self.eat("unsafe") {
            compound = true;
            self.newlines();
            StmtKind::Unsafe(self.block()?)
        } else if self.at("{") {
            compound = true;
            StmtKind::Block(self.block()?)
        } else {
            compound = false;
            self.simple_statement(true)?
        };
        let span = self.since(start);
        if !compound {
            self.end_statement()?;
        }
        Ok(Stmt { kind, span })
    }
    fn simple_statement(&mut self, allow_struct: bool) -> ParseResult<StmtKind> {
        if self.eat("const") {
            let ty = self.ty()?;
            let name = self.identifier()?;
            self.expect("=")?;
            self.newlines();
            return Ok(StmtKind::Let {
                name,
                ty,
                value: Some(self.expression(allow_struct)?),
                constant: true,
            });
        }
        if self.look(1, ":=") {
            let name = self.identifier()?;
            self.expect(":=")?;
            self.newlines();
            return Ok(StmtKind::Let {
                name,
                ty: Type::Unknown,
                value: Some(self.expression(allow_struct)?),
                constant: false,
            });
        }
        // A type-first declaration is unambiguous once followed by its binding name.
        let saved = self.cursor;
        let saved_angles = self.angle_splits.len();
        if let Ok(ty) = self.ty()
            && matches!(&self.token().kind, TokenKind::Ident(name) if !reserved(name) && name != "as")
        {
            let name = self.identifier()?;
            let value = if self.eat("=") {
                self.newlines();
                Some(self.expression(allow_struct)?)
            } else {
                None
            };
            return Ok(StmtKind::Let {
                name,
                ty,
                value,
                constant: false,
            });
        }
        self.cursor = saved;
        self.restore_angles(saved_angles);
        let target = self.expression(allow_struct)?;
        if self.eat("=") {
            self.newlines();
            return Ok(StmtKind::Assign {
                target,
                op: None,
                value: self.expression(allow_struct)?,
            });
        }
        let op = match &self.token().kind {
            TokenKind::Symbol("+=") => Some(BinaryOp::Add),
            TokenKind::Symbol("-=") => Some(BinaryOp::Sub),
            TokenKind::Symbol("*=") => Some(BinaryOp::Mul),
            TokenKind::Symbol("/=") => Some(BinaryOp::Div),
            TokenKind::Symbol("%=") => Some(BinaryOp::Rem),
            TokenKind::Symbol("&=") => Some(BinaryOp::BitAnd),
            TokenKind::Symbol("|=") => Some(BinaryOp::BitOr),
            TokenKind::Symbol("^=") => Some(BinaryOp::BitXor),
            TokenKind::Symbol("<<=") => Some(BinaryOp::Shl),
            TokenKind::Symbol(">>=") => Some(BinaryOp::Shr),
            _ => None,
        };
        if let Some(op) = op {
            self.bump();
            self.newlines();
            Ok(StmtKind::Assign {
                target,
                op: Some(op),
                value: self.expression(allow_struct)?,
            })
        } else {
            Ok(StmtKind::Expr(target))
        }
    }
    fn if_statement(&mut self) -> ParseResult<StmtKind> {
        self.nested(Self::if_statement_inner)
    }
    fn if_statement_inner(&mut self) -> ParseResult<StmtKind> {
        let condition = self.expression(false)?;
        self.newlines();
        let then_block = self.block()?;
        let saved = self.cursor;
        self.newlines();
        let else_block = if self.eat("else") {
            self.newlines();
            if self.eat("if") {
                let start = self.tokens[self.cursor - 1].span.start;
                let kind = self.if_statement()?;
                vec![Stmt {
                    kind,
                    span: self.since(start),
                }]
            } else {
                self.block()?
            }
        } else {
            self.cursor = saved;
            Vec::new()
        };
        Ok(StmtKind::If {
            condition,
            then_block,
            else_block,
        })
    }
    fn for_statement(&mut self) -> ParseResult<StmtKind> {
        if self.at("{") {
            return Ok(StmtKind::For {
                init: None,
                condition: None,
                step: None,
                body: self.block()?,
            });
        }
        if self.look(1, "in") || self.look(1, ",") {
            let first = self.identifier()?;
            let (index, name) = if self.eat(",") {
                (Some(first), self.identifier()?)
            } else {
                (None, first)
            };
            self.expect("in")?;
            let iterable = self.expression(false)?;
            self.newlines();
            let body = self.block()?;
            return Ok(StmtKind::ForEach {
                index,
                name,
                iterable,
                body,
            });
        }
        let start = self.span().start;
        let first = if self.at(";") {
            None
        } else {
            let kind = self.simple_statement(false)?;
            Some(Stmt {
                kind,
                span: self.since(start),
            })
        };
        if self.eat(";") {
            self.newlines();
            let condition = if self.at(";") {
                None
            } else {
                Some(self.expression(false)?)
            };
            self.expect(";")?;
            self.newlines();
            let step = if self.at("{") {
                None
            } else {
                let start = self.span().start;
                let kind = self.simple_statement(false)?;
                Some(Box::new(Stmt {
                    kind,
                    span: self.since(start),
                }))
            };
            self.newlines();
            let body = self.block()?;
            Ok(StmtKind::For {
                init: first.map(Box::new),
                condition,
                step,
                body,
            })
        } else {
            let condition =
                match first {
                    Some(Stmt {
                        kind: StmtKind::Expr(expression),
                        ..
                    }) => expression,
                    _ => return Err(self.error(
                        "condition loops need an expression; three-part loops need `;` separators",
                    )),
                };
            self.newlines();
            let body = self.block()?;
            Ok(StmtKind::For {
                init: None,
                condition: Some(condition),
                step: None,
                body,
            })
        }
    }
    fn match_statement(&mut self) -> ParseResult<StmtKind> {
        let value = self.expression(false)?;
        self.newlines();
        self.expect("{")?;
        self.separators();
        let mut arms = Vec::new();
        while !self.eat("}") {
            if self.eof() {
                return Err(self.error("unclosed match statement; expected `}`"));
            }
            let start = self.span().start;
            let pattern = self.pattern()?;
            self.expect("=>")?;
            self.newlines();
            let body = self.block()?;
            arms.push(MatchArm {
                pattern,
                body,
                span: self.since(start),
            });
            self.eat(",");
            self.separators();
        }
        Ok(StmtKind::Match { value, arms })
    }
    fn pattern(&mut self) -> ParseResult<Pattern> {
        if self.eat("_") {
            return Ok(Pattern::Wildcard);
        }
        if self.eat("true") {
            return Ok(Pattern::Bool(true));
        }
        if self.eat("false") {
            return Ok(Pattern::Bool(false));
        }
        if self.eat("-") {
            let token = self.bump();
            if let TokenKind::Int(value, _) = token.kind
                && value <= (1u64 << 63)
            {
                return Ok(Pattern::Int(value.wrapping_neg()));
            }
            return Err(Diagnostic::new(
                token.span,
                "expected a signed integer literal after `-` in a pattern",
            ));
        }
        if let TokenKind::Int(value, _) = self.token().kind {
            self.bump();
            return Ok(Pattern::Int(value));
        }
        let name = self.qualified_name()?;
        let mut bindings = Vec::new();
        if self.eat("(") {
            self.newlines();
            while !self.eat(")") {
                bindings.push(self.identifier()?);
                self.newlines();
                if self.eat(",") {
                    self.newlines();
                } else {
                    self.expect(")")?;
                    break;
                }
            }
        }
        Ok(Pattern::Variant(name, bindings))
    }
    fn expression(&mut self, allow_struct: bool) -> ParseResult<Expr> {
        self.expr_bp(0, allow_struct)
    }
    fn expr_bp(&mut self, minimum: u8, allow_struct: bool) -> ParseResult<Expr> {
        self.nested(|parser| parser.expr_bp_inner(minimum, allow_struct))
    }
    fn expr_bp_inner(&mut self, minimum: u8, allow_struct: bool) -> ParseResult<Expr> {
        if self.soft_newlines > 0 {
            self.newlines();
        }
        let start = self.span().start;
        let unary = if self.eat("-") {
            Some(UnaryOp::Neg)
        } else if self.eat("!") {
            Some(UnaryOp::Not)
        } else if self.eat("~") {
            Some(UnaryOp::BitNot)
        } else if self.eat("*") {
            Some(UnaryOp::Deref)
        } else if self.eat("&") {
            Some(if self.eat("mut") {
                UnaryOp::BorrowMut
            } else {
                UnaryOp::Borrow
            })
        } else {
            None
        };
        let mut lhs = if let Some(op) = unary {
            self.newlines();
            let rhs = self.expr_bp(23, allow_struct)?;
            Expr::new(ExprKind::Unary(op, Box::new(rhs)), self.since(start))
        } else {
            self.primary(allow_struct)?
        };
        loop {
            if self.soft_newlines > 0 {
                self.newlines();
            }
            if 25 >= minimum {
                if self.eat("?") {
                    lhs = Expr::new(ExprKind::Try(Box::new(lhs)), self.since(start));
                    continue;
                }
                if self.eat("[") {
                    self.soft_newlines += 1;
                    let index = self.expression(true)?;
                    self.newlines();
                    self.expect("]")?;
                    self.soft_newlines -= 1;
                    lhs = Expr::new(
                        ExprKind::Index(Box::new(lhs), Box::new(index)),
                        self.since(start),
                    );
                    continue;
                }
                if self.eat(".") {
                    let field = self.identifier()?;
                    lhs = Expr::new(ExprKind::Field(Box::new(lhs), field), self.since(start));
                    continue;
                }
                if self.at("(") {
                    let args = self.arguments()?;
                    lhs = self.call(lhs, Vec::new(), args, start)?;
                    continue;
                }
                if allow_struct
                    && self.at("{")
                    && let Some(name) = expression_path(&lhs)
                {
                    lhs = self.struct_literal(name, start)?;
                    continue;
                }
                if self.at("::")
                    || (self.at("<") && matches!(lhs.kind, ExprKind::Name(_) | ExprKind::Field(..)))
                {
                    let explicit = self.eat("::");
                    let saved_cursor = self.cursor;
                    let saved_angles = self.angle_splits.len();
                    match self.type_args() {
                        Ok(types) if self.at("(") => {
                            let args = self.arguments()?;
                            lhs = self.call(lhs, types, args, start)?;
                            continue;
                        }
                        Ok(types) if allow_struct && self.at("{") => {
                            let name = expression_path(&lhs)
                                .ok_or_else(|| self.error("expected a named struct type"))?;
                            lhs =
                                self.struct_literal(Type::Generic(name, types).to_string(), start)?;
                            continue;
                        }
                        Ok(_) if explicit => {
                            return Err(self.error("expected `(` after function type arguments"));
                        }
                        Err(error) if explicit => return Err(error),
                        _ => {
                            self.cursor = saved_cursor;
                            self.restore_angles(saved_angles);
                        }
                    }
                }
            }
            if self.at("as") && 21 >= minimum {
                self.bump();
                self.newlines();
                let ty = self.ty()?;
                lhs = Expr::new(ExprKind::Cast(Box::new(lhs), ty), self.since(start));
                continue;
            }
            let Some((op, left, right)) = binary(&self.token().kind) else {
                break;
            };
            if left < minimum {
                break;
            }
            self.bump();
            self.newlines();
            let rhs = self.expr_bp(right, allow_struct)?;
            lhs = Expr::new(
                ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)),
                self.since(start),
            );
        }
        Ok(lhs)
    }
    fn call(
        &self,
        callee: Expr,
        type_args: Vec<Type>,
        args: Vec<Expr>,
        start: usize,
    ) -> ParseResult<Expr> {
        let kind = match callee.kind {
            ExprKind::Name(name) => ExprKind::Call {
                name,
                type_args,
                args,
            },
            ExprKind::Field(receiver, name) => {
                if !type_args.is_empty() {
                    let Some(prefix) = expression_path(&receiver) else {
                        return Err(Diagnostic::new(
                            callee.span,
                            "generic method calls require a qualified type/function name",
                        ));
                    };
                    ExprKind::Call {
                        name: format!("{prefix}.{name}"),
                        type_args,
                        args,
                    }
                } else {
                    ExprKind::MethodCall {
                        receiver,
                        name,
                        args,
                    }
                }
            }
            _ => {
                return Err(Diagnostic::new(
                    callee.span,
                    "only named functions and methods can be called; Dodo has no function values",
                ));
            }
        };
        Ok(Expr::new(kind, self.since(start)))
    }
    fn arguments(&mut self) -> ParseResult<Vec<Expr>> {
        self.expect("(")?;
        self.soft_newlines += 1;
        self.newlines();
        let mut args = Vec::new();
        while !self.eat(")") {
            args.push(self.expression(true)?);
            self.newlines();
            if self.eat(",") {
                self.newlines();
            } else {
                self.expect(")")?;
                break;
            }
        }
        self.soft_newlines -= 1;
        Ok(args)
    }
    fn primary(&mut self, allow_struct: bool) -> ParseResult<Expr> {
        let start = self.span().start;
        let token = self.bump();
        let kind = match token.kind {
            TokenKind::Int(value, ty) => ExprKind::Int(value, ty),
            TokenKind::Float(value, ty) => ExprKind::Float(value, ty),
            TokenKind::String(value, byte) => ExprKind::String(value, byte),
            TokenKind::Ident(name) if name == "true" || name == "false" => {
                ExprKind::Bool(name == "true")
            }
            TokenKind::Ident(name) if !reserved(&name) => {
                if name == "Self" && self.self_type.is_none() {
                    return Err(Diagnostic::new(
                        token.span,
                        "`Self` is only valid inside a struct declaration",
                    ));
                }
                let name = if name == "Self" {
                    match &self.self_type {
                        Some(Type::Named(name) | Type::Generic(name, _)) => name.clone(),
                        _ => name,
                    }
                } else {
                    name
                };
                if allow_struct && self.at("{") {
                    return self.struct_literal(name, start);
                } else {
                    ExprKind::Name(name)
                }
            }
            TokenKind::Symbol("(") => {
                self.soft_newlines += 1;
                let mut value = self.expression(true)?;
                self.newlines();
                self.expect(")")?;
                self.soft_newlines -= 1;
                value.span = self.since(start);
                return Ok(value);
            }
            TokenKind::Symbol("[") => {
                self.cursor -= 1;
                let ty = self.ty()?;
                if !matches!(ty, Type::Array(..)) {
                    return Err(self.error("array literal requires a fixed array type"));
                }
                self.expect("{")?;
                self.soft_newlines += 1;
                self.newlines();
                let mut values = Vec::new();
                while !self.eat("}") {
                    values.push(self.expression(true)?);
                    self.newlines();
                    if self.eat(",") {
                        self.newlines();
                    } else {
                        self.expect("}")?;
                        break;
                    }
                }
                self.soft_newlines -= 1;
                ExprKind::Array(ty, values)
            }
            _ => return Err(Diagnostic::new(token.span, "expected an expression")),
        };
        Ok(Expr::new(kind, self.since(start)))
    }
    fn struct_literal(&mut self, name: String, start: usize) -> ParseResult<Expr> {
        self.expect("{")?;
        self.soft_newlines += 1;
        self.newlines();
        let mut fields = Vec::new();
        while !self.eat("}") {
            let field = self.identifier()?;
            self.expect(":")?;
            self.newlines();
            let value = self.expression(true)?;
            fields.push((field, value));
            self.newlines();
            if self.eat(",") {
                self.newlines();
            } else {
                self.expect("}")?;
                break;
            }
        }
        self.soft_newlines -= 1;
        Ok(Expr::new(ExprKind::Struct(name, fields), self.since(start)))
    }
}

fn expression_path(expression: &Expr) -> Option<String> {
    match &expression.kind {
        ExprKind::Name(name) => Some(name.clone()),
        ExprKind::Field(receiver, name) => Some(format!("{}.{name}", expression_path(receiver)?)),
        _ => None,
    }
}

fn reserved(name: &str) -> bool {
    matches!(
        name,
        "package"
            | "import"
            | "pub"
            | "fn"
            | "struct"
            | "enum"
            | "const"
            | "return"
            | "if"
            | "else"
            | "for"
            | "in"
            | "break"
            | "continue"
            | "match"
            | "unsafe"
            | "extern"
            | "as"
            | "from"
            | "static"
            | "void"
            | "mut"
            | "true"
            | "false"
    )
}

fn binary(token: &TokenKind) -> Option<(BinaryOp, u8, u8)> {
    let (operator, precedence) = match token {
        TokenKind::Symbol("||") => (BinaryOp::Or, 1),
        TokenKind::Symbol("&&") => (BinaryOp::And, 3),
        TokenKind::Symbol("|") => (BinaryOp::BitOr, 5),
        TokenKind::Symbol("^") => (BinaryOp::BitXor, 7),
        TokenKind::Symbol("&") => (BinaryOp::BitAnd, 9),
        TokenKind::Symbol("==") => (BinaryOp::Eq, 11),
        TokenKind::Symbol("!=") => (BinaryOp::Ne, 11),
        TokenKind::Symbol("<") => (BinaryOp::Lt, 13),
        TokenKind::Symbol("<=") => (BinaryOp::Le, 13),
        TokenKind::Symbol(">") => (BinaryOp::Gt, 13),
        TokenKind::Symbol(">=") => (BinaryOp::Ge, 13),
        TokenKind::Symbol("<<") => (BinaryOp::Shl, 15),
        TokenKind::Symbol(">>") => (BinaryOp::Shr, 15),
        TokenKind::Symbol("+") => (BinaryOp::Add, 17),
        TokenKind::Symbol("-") => (BinaryOp::Sub, 17),
        TokenKind::Symbol("*") => (BinaryOp::Mul, 19),
        TokenKind::Symbol("/") => (BinaryOp::Div, 19),
        TokenKind::Symbol("%") => (BinaryOp::Rem, 19),
        _ => return None,
    };
    Some((operator, precedence, precedence + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn body(source: &str) -> Block {
        parse(&format!(
            "package test\nfn main() -> void {{\n{source}\n}}\n"
        ))
        .unwrap()
        .functions
        .remove(0)
        .body
        .unwrap()
    }
    #[test]
    fn precedence_casts_and_postfix() {
        let statements = body("x := 1 + 2 * 3\ny := f(1)? as u32 + 1");
        let StmtKind::Let {
            value: Some(value), ..
        } = &statements[0].kind
        else {
            panic!("expected local");
        };
        assert!(
            matches!(&value.kind, ExprKind::Binary(BinaryOp::Add, _, rhs) if matches!(rhs.kind, ExprKind::Binary(BinaryOp::Mul, ..)))
        );
        let StmtKind::Let {
            value: Some(value), ..
        } = &statements[1].kind
        else {
            panic!("expected local");
        };
        assert!(
            matches!(&value.kind, ExprKind::Binary(BinaryOp::Add, lhs, _) if matches!(&lhs.kind, ExprKind::Cast(inner, _) if matches!(inner.kind, ExprKind::Try(_))))
        );
    }
    #[test]
    fn all_for_forms() {
        let statements = body(
            "for { break }\nfor true { continue }\nfor i := 0; i < 10; i += 1 {}\nfor ; ; {}\nfor value in values {}\nfor i, value in &mut values {}",
        );
        assert_eq!(statements.len(), 6);
        assert!(matches!(
            statements[0].kind,
            StmtKind::For {
                condition: None,
                ..
            }
        ));
        assert!(matches!(
            statements[2].kind,
            StmtKind::For {
                init: Some(_),
                condition: Some(_),
                step: Some(_),
                ..
            }
        ));
        assert!(
            matches!(&statements[5].kind, StmtKind::ForEach { index: Some(index), iterable, .. } if index == "i" && matches!(iterable.kind, ExprKind::Unary(UnaryOp::BorrowMut, _)))
        );
    }
    #[test]
    fn structs_methods_and_borrow_contracts() {
        let program = parse("package sample\n@repr(C)\npub struct Sample {\n pub [4]u16 values\n pub fn view(self: &Self) -> &[u16] from(self) { return &self.values }\n}\n").unwrap();
        assert!(program.structs[0].repr_c);
        assert_eq!(program.functions[0].name, "Sample.view");
        assert_eq!(
            program.functions[0].params[0].ty,
            Type::Ref(false, Box::new(Type::Named("Sample".into())))
        );
        assert_eq!(program.functions[0].from, ["self"]);
    }
    #[test]
    fn generic_types_functions_and_nested_closing_angles() {
        let program = parse("package sample\nstruct Box<T> { T value\n fn get(self: &Self) -> &T { return &self.value }\n}\nfn id<T>(value: T) -> T { return value }\nfn demo(x: Option<Result<u8, bool>>) -> void {\n y := id<u8>(1)\n z := id::<Option<u8>>(none)\n}\n").unwrap();
        assert_eq!(program.functions[0].generics, ["T"]);
        assert!(matches!(program.functions[2].params[0].ty, Type::Option(_)));
    }
    #[test]
    fn result_binds_outside_borrow_types() {
        let program = parse("package p\nfn f(a: &u8) -> &u8!bool { return ok(a) }\n").unwrap();
        assert_eq!(
            program.functions[0].ret,
            Type::Result(
                Box::new(Type::Ref(false, Box::new(Type::u8()))),
                Box::new(Type::Bool)
            )
        );
    }
    #[test]
    fn foreign_prototypes_and_enum_payload_patterns() {
        let program = parse("package p\nunsafe extern \"C\" fn send(p: *const u8, n: usize) -> i32\npub enum Error { Empty, Invalid(u8 byte), Other(code: u32) }\nfn f(e: Error) -> void { match e { Error.Empty => {} Error.Invalid(byte) => {} _ => {} } }\n").unwrap();
        assert!(program.functions[0].body.is_none());
        assert_eq!(program.enums[0].variants[1].fields[0].name, "byte");
        assert_eq!(program.enums[0].variants[2].fields[0].name, "code");
    }
    #[test]
    fn arrays_strings_nested_blocks_and_multiline_calls() {
        let statements = body(
            "a := [3]u8{1,\n2, 3,}\ns := b\"hi\"\nf(\n 1 +\n 2,\n a[\n0\n],\n)\nif true {\n{ x := 1 }\n} else if false {} else {}\nunsafe {}\nu32 count\nconst u32 LIMIT = 3",
        );
        assert_eq!(statements.len(), 7);
    }
    #[test]
    fn rejects_missing_terminators_and_implicit_return_signatures() {
        for source in [
            "package p\nfn f() {}",
            "fn f() -> void {}",
            "package p\nfn f() -> void { x := 1 y := 2 }",
            "package p\nfn f() -> void {",
            "package p\nfn f() -> void { return (1 }",
            "package p\n@unknown fn f() -> void {}",
            "package p\nfn f() -> Self {}",
        ] {
            assert!(parse(source).is_err(), "accepted {source:?}");
        }
    }
    #[test]
    fn malformed_token_sequences_do_not_panic() {
        let pieces = [
            "fn", "struct", "enum", "for", "if", "match", "return", "(", ")", "{", "}", "[", "]",
            "<", ">>", "&", "*", "x", "1", "\n",
        ];
        for first in pieces {
            for second in pieces {
                let source = format!("package p\nfn f() -> void {{ {first} {second} }}");
                assert!(
                    std::panic::catch_unwind(|| parse(&source)).is_ok(),
                    "panicked for {source:?}"
                );
            }
        }
    }
    #[test]
    fn excessive_nesting_has_a_diagnostic() {
        let source = format!(
            "package p\nfn f() -> void {{ x := {}1{} }}",
            "(".repeat(300),
            ")".repeat(300)
        );
        assert!(parse(&source).unwrap_err().message.contains("nesting"));
    }
    #[test]
    fn complete_worked_examples_from_the_specification_parse() {
        let spec = include_str!("../docs/language-spec-0.1.md");
        let mut examples = 0;
        for block in spec.split("```dodo\n").skip(1) {
            let source = block.split("```").next().unwrap();
            if !source.starts_with("package ") {
                continue;
            }
            parse(source)
                .unwrap_or_else(|error| panic!("{}", error.render("language-spec-0.1.md", source)));
            examples += 1;
        }
        assert!(
            examples >= 4,
            "expected the package example and all three worked examples"
        );
    }
    #[test]
    fn qualified_generic_struct_literals_and_negative_patterns() {
        let statements =
            body("x := lib.Box<Option<u8>>{value: none}\nmatch value { -1 => {} _ => {} }");
        assert!(
            matches!(&statements[0].kind, StmtKind::Let { value: Some(Expr { kind: ExprKind::Struct(name, _), .. }), .. } if name == "lib.Box<Option<u8>>")
        );
        assert!(
            matches!(&statements[1].kind, StmtKind::Match { arms, .. } if matches!(arms[0].pattern, Pattern::Int(u64::MAX)))
        );
    }
}
