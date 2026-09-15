//! The only heterogeneous call facility: reserved std/fmt and std/console
//! printing declarations. Lower to ordinary fixed signatures and Formatter
//! methods before the production checker. Neither LLVM nor the runtime needs
//! variadics, type erasure, a format parser, or an allocator.
use super::*;

pub(super) fn is_place(e: &Expr) -> bool {
    matches!(
        e.kind,
        ExprKind::Name(_)
            | ExprKind::Field(..)
            | ExprKind::Index(..)
            | ExprKind::Unary(UnaryOp::Deref, _)
    )
}

#[derive(Default, Debug)]
struct Options {
    fill: Option<u8>,
    align: Option<u8>,
    sign: Option<u8>,
    zero: bool,
    width: Option<u32>,
    precision: Option<u32>,
    style: Option<u8>,
    explicit: bool,
}
#[derive(Debug)]
enum Part {
    Text(Vec<u8>),
    Field(Options),
}
fn invalid(span: Span, message: &str) -> Diagnostic {
    Diagnostic::new(span, format!("invalid print format: {message}"))
}
fn number(bytes: &[u8], at: &mut usize, span: Span) -> Check<Option<u32>> {
    let start = *at;
    let mut value = 0u32;
    while let Some(digit @ b'0'..=b'9') = bytes.get(*at) {
        value = value
            .checked_mul(10)
            .and_then(|n| n.checked_add((digit - b'0') as u32))
            .ok_or_else(|| invalid(span, "width or precision exceeds u32"))?;
        *at += 1;
    }
    Ok((*at != start).then_some(value))
}
fn options(bytes: &[u8], span: Span) -> Check<Options> {
    let mut out = Options {
        explicit: !bytes.is_empty(),
        ..Options::default()
    };
    let mut at = 0;
    let alignment = |b: u8| matches!(b, b'<' | b'>' | b'^');
    if bytes.len() >= 2 && alignment(bytes[1]) {
        if !bytes[0].is_ascii() || matches!(bytes[0], b'{' | b'}') {
            return Err(invalid(
                span,
                "fill must be an ASCII byte other than a brace",
            ));
        }
        out.fill = Some(bytes[0]);
        out.align = Some(bytes[1]);
        at = 2;
    } else if bytes.first().is_some_and(|b| alignment(*b)) {
        out.align = Some(bytes[0]);
        at = 1;
    }
    if bytes
        .get(at)
        .is_some_and(|b| matches!(b, b'+' | b'-' | b' '))
    {
        out.sign = Some(bytes[at]);
        at += 1;
    }
    if bytes.get(at) == Some(&b'0') {
        out.zero = true;
        at += 1;
    }
    out.width = number(bytes, &mut at, span)?;
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        out.precision = Some(
            number(bytes, &mut at, span)?
                .ok_or_else(|| invalid(span, "expected precision after '.'"))?,
        );
        if out.precision.unwrap() > 324 {
            return Err(invalid(span, "float precision must be in 0..=324"));
        }
    }
    if let Some(style) = bytes.get(at) {
        if !matches!(
            style,
            b'd' | b'b' | b'o' | b'x' | b'X' | b'f' | b'F' | b'e' | b'E' | b's' | b'c'
        ) {
            return Err(invalid(span, "unsupported formatting option"));
        }
        out.style = Some(*style);
        at += 1;
    }
    if at != bytes.len() {
        return Err(invalid(span, "unexpected formatting options"));
    }
    if out.zero && out.align.is_some() {
        return Err(invalid(
            span,
            "zero padding cannot be combined with alignment or fill",
        ));
    }
    Ok(out)
}
fn parse(bytes: &[u8], span: Span) -> Check<Vec<Part>> {
    let mut parts = vec![];
    let mut text = vec![];
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            brace @ (b'{' | b'}') if bytes.get(at + 1) == Some(&brace) => {
                text.push(brace);
                at += 2;
            }
            b'{' => {
                if !text.is_empty() {
                    parts.push(Part::Text(std::mem::take(&mut text)));
                }
                at += 1;
                let start = at;
                while at < bytes.len() && bytes[at] != b'}' {
                    if bytes[at] == b'{' {
                        return Err(invalid(span, "nested '{' in placeholder"));
                    }
                    at += 1;
                }
                if at == bytes.len() {
                    return Err(invalid(span, "unclosed '{'"));
                }
                let field = &bytes[start..at];
                let spec = if field.is_empty() {
                    field
                } else {
                    field.strip_prefix(b":").ok_or_else(|| invalid(span, "placeholders are '{}' or '{:options}'; named and numbered arguments are unsupported"))?
                };
                parts.push(Part::Field(options(spec, span)?));
                at += 1;
            }
            b'}' => return Err(invalid(span, "unmatched '}'; use '}}' for a literal brace")),
            byte => {
                text.push(byte);
                at += 1;
            }
        }
    }
    if !text.is_empty() {
        parts.push(Part::Text(text));
    }
    Ok(parts)
}

// All synthesized nodes use the original call span. The arguments themselves
// retain their source spans, including expressions inside generic callers.
struct Ast(Span);
impl Ast {
    fn expr(&self, kind: ExprKind) -> Expr {
        Expr::new(kind, self.0)
    }
    fn name(&self, name: &str) -> Expr {
        // Source identifiers cannot contain '$'. Keep generated bindings clear
        // of caller globals as well as caller locals, even before full checking.
        self.expr(ExprKind::Name(if name.contains('.') {
            name.into()
        } else {
            format!("$print_{name}")
        }))
    }
    fn int(&self, n: u32) -> Expr {
        self.expr(ExprKind::Int(n as u64, None))
    }
    fn string(&self, bytes: Vec<u8>) -> Expr {
        self.expr(ExprKind::String(bytes, false))
    }
    fn unary(&self, op: UnaryOp, e: Expr) -> Expr {
        self.expr(ExprKind::Unary(op, Box::new(e)))
    }
    fn field(&self, e: Expr, name: &str) -> Expr {
        self.expr(ExprKind::Field(Box::new(e), name.into()))
    }
    fn call(&self, name: &str, args: Vec<Expr>) -> Expr {
        self.expr(ExprKind::Call {
            name: name.into(),
            type_args: vec![],
            args,
        })
    }
    fn method(&self, receiver: Expr, name: &str, args: Vec<Expr>) -> Expr {
        self.expr(ExprKind::MethodCall {
            receiver: Box::new(receiver),
            name: name.into(),
            args,
        })
    }
    fn stmt(&self, kind: StmtKind) -> Stmt {
        Stmt { kind, span: self.0 }
    }
    fn let_(&self, name: &str, value: Expr) -> Stmt {
        self.stmt(StmtKind::Let {
            name: format!("$print_{name}"),
            ty: Type::Unknown,
            value: Some(value),
            constant: false,
            mutable: true,
        })
    }
    fn write(&self, method: &str, args: Vec<Expr>) -> Stmt {
        self.stmt(StmtKind::Expr(self.expr(ExprKind::Try(Box::new(
            self.method(self.name("output"), method, args),
        )))))
    }
    fn set(&self, field: &str, value: Expr) -> Stmt {
        self.stmt(StmtKind::Assign {
            target: self.field(self.name("options"), field),
            value,
            op: None,
        })
    }
}

struct Field<'a> {
    value: Expr,
    ty: &'a Type,
    options: Options,
    span: Span,
}

impl Expander {
    pub(super) fn printing_call(
        &mut self,
        name: &mut String,
        type_args: &[Type],
        args: &mut Vec<Expr>,
        template: &Function,
        substitutions: &HashMap<String, Type>,
        span: Span,
    ) -> Check<Type> {
        let operation = template.printing.unwrap();
        if !type_args.is_empty() {
            return Err(Diagnostic::new(
                span,
                "printing calls infer their types; explicit type arguments are unsupported",
            ));
        }
        let has_sink = template
            .params
            .first()
            .is_some_and(|p| matches!(p.name.as_str(), "sink" | "self"));
        let offset = usize::from(has_sink);
        let required = offset + 1;
        if args.len() < required || operation != Printing::Printf && args.len() != required {
            return Err(Diagnostic::new(
                span,
                format!(
                    "printing call expects {}{} arguments, found {}",
                    if operation == Printing::Printf {
                        "at least "
                    } else {
                        ""
                    },
                    required,
                    args.len()
                ),
            ));
        }
        let mut parts = if operation == Printing::Printf {
            let ExprKind::String(bytes, false) = &args[offset].kind else {
                return Err(Diagnostic::new(
                    args[offset].span,
                    "printf requires a literal format string",
                ));
            };
            let parts = parse(bytes, args[offset].span)?;
            let fields = parts.iter().filter(|p| matches!(p, Part::Field(_))).count();
            if fields != args.len() - required {
                return Err(Diagnostic::new(
                    span,
                    format!(
                        "printf has {fields} placeholders but {} arguments",
                        args.len() - required
                    ),
                ));
            }
            // The format is a compile-time input, with no runtime evaluation.
            args.remove(offset);
            parts
        } else {
            vec![Part::Field(Options::default())]
        };
        if operation == Printing::Println {
            parts.push(Part::Text(b"\n".to_vec()));
        }

        let fmt = self
            .signatures
            .values()
            .find(|f| {
                !f.generic_instance
                    && f.printing == Some(Printing::Print)
                    && f.params.first().is_some_and(|p| p.name == "sink")
            })
            .and_then(|f| f.name.strip_suffix(".print"))
            .ok_or_else(|| Diagnostic::new(span, "printing requires std/fmt"))?
            .to_owned();
        let ast = Ast(span);
        let mut parameters = vec![];
        let mut types = vec![];
        for (index, arg) in args.iter_mut().enumerate() {
            let mut ty = self.expression(arg, substitutions, None)?;
            // Preserve custom values: places are borrowed; owned temporaries
            // become helper parameters and are dropped normally, exactly once.
            if index >= offset && matches!(ty, Type::Named(_)) && is_place(arg) {
                *arg = ast.unary(UnaryOp::Borrow, arg.clone());
                ty = Type::Ref(false, Box::new(ty));
            }
            parameters.push(Param {
                name: if has_sink && index == 0 {
                    format!("$print_{}", template.params[0].name)
                } else {
                    format!("$print_arg{index}")
                },
                ty: ty.clone(),
                span: arg.span,
            });
            types.push(ty);
        }
        let mut body = vec![];
        let (writer, sink) = if has_sink {
            match &types[0] {
                Type::Ref(true, writer) => (*writer.clone(), ast.name(&template.params[0].name)),
                Type::Named(_) if template.params[0].name == "self" => (
                    types[0].clone(),
                    ast.unary(UnaryOp::BorrowMut, ast.name(&template.params[0].name)),
                ),
                _ => {
                    return Err(Diagnostic::new(
                        args[0].span,
                        "printing sink must be a mutable writer borrow",
                    ));
                }
            }
        } else {
            let console = template.name.rsplit_once('.').unwrap().0;
            body.push(ast.let_("sink", ast.call(&format!("{console}.stdout"), vec![])));
            (
                Type::Named(format!("{console}.Output")),
                ast.unary(UnaryOp::BorrowMut, ast.name("sink")),
            )
        };
        body.push(ast.let_(
            "output",
            ast.expr(ExprKind::Call {
                name: format!("{fmt}.Formatter.new"),
                type_args: vec![writer],
                args: vec![sink],
            }),
        ));
        let mut index = offset;
        for part in parts {
            match part {
                Part::Text(bytes) => body.push(ast.write("string", vec![ast.string(bytes)])),
                Part::Field(options) => {
                    let mut value = ast.name(&format!("arg{index}"));
                    let mut ty = &types[index];
                    while let Type::Ref(_, inner) = ty {
                        value = ast.unary(UnaryOp::Deref, value);
                        ty = inner;
                    }
                    let arg_span = args[index].span;
                    index += 1;
                    let mut field = vec![];
                    let Type::Result(_, error) = &template.ret else {
                        unreachable!()
                    };
                    self.printing_field(
                        &ast,
                        &fmt,
                        &mut field,
                        Field {
                            value,
                            ty,
                            options,
                            span: arg_span,
                        },
                        error,
                    )?;
                    body.push(ast.stmt(StmtKind::Block(field)));
                }
            }
        }
        body.push(ast.stmt(StmtKind::Return(Some(ast.call(
            "ok",
            vec![ast.method(ast.name("output"), "written", vec![])],
        )))));
        self.printing_count += 1;
        // '$print' cannot be spelled by source identifiers or collide with
        // generic specialization's hexadecimal names.
        let generated = format!("{}$print{}", template.name, self.printing_count);
        let mut function = template.clone();
        function.name = generated.clone();
        // Retain source API metadata for editor signatures. generic_instance
        // distinguishes this fixed helper from an unexpanded prototype.
        function.printing = Some(operation);
        function.generics.clear();
        function.generic_instance = true;
        function.params = parameters;
        function.body = Some(body);
        function.span = span;
        function.ret_span = span;
        self.function(&mut function, &HashMap::new())?;
        self.functions.push(function);
        *name = generated;
        Ok(template.ret.clone())
    }

    fn printing_field(
        &self,
        ast: &Ast,
        fmt: &str,
        body: &mut Block,
        field: Field<'_>,
        error: &Type,
    ) -> Check<()> {
        let Field {
            value,
            ty,
            options,
            span,
        } = field;
        let numeric = ty.is_numeric();
        let compatible = match ty {
            Type::Int { .. } => {
                options.precision.is_none()
                    && options.style.is_none_or(|s| {
                        matches!(s, b'd' | b'b' | b'o' | b'x' | b'X')
                            || s == b'c'
                                && *ty
                                    == Type::Int {
                                        signed: false,
                                        bits: 32,
                                    }
                    })
            }
            Type::Float(_) => options
                .style
                .is_none_or(|s| matches!(s, b'f' | b'F' | b'e' | b'E')),
            Type::Str => options.precision.is_none() && options.style.is_none_or(|s| s == b's'),
            Type::Bool => options.precision.is_none() && options.style.is_none(),
            Type::Named(owner) if !options.explicit => {
                let method = self.signatures.get(&format!("{owner}.format")).or_else(|| {
                    let Type::Generic(template, _) = self.concrete_types.get(owner)? else {
                        return None;
                    };
                    self.signatures.get(&format!("{template}.format"))
                });
                if !method.is_some_and(|f| {
                    f.public
                        && !f.unsafe_
                        && !f.extern_
                        && f.params.len() == 2
                        && matches!(f.params[0].ty, Type::Ref(false, _))
                        && matches!(f.params[1].ty, Type::Ref(true, _))
                }) {
                    return Err(Diagnostic::new(
                        span,
                        format!(
                            "type `{ty}` must implement the public formatting contract: fn format<W>(&self, output: &mut Formatter<W>) -> void!io.Error"
                        ),
                    ));
                }
                // Let ordinary specialization and checking verify the return
                // type, including error types supplied by a generic owner.
                body.push(ast.stmt(StmtKind::Let {
                    name: "$print_result".into(),
                    ty: Type::Result(Box::new(Type::Void), Box::new(error.clone())),
                    value: Some(ast.method(
                        value,
                        "format",
                        vec![ast.unary(UnaryOp::BorrowMut, ast.name("output"))],
                    )),
                    constant: false,
                    mutable: false,
                }));
                body.push(ast.stmt(StmtKind::Expr(
                    ast.expr(ExprKind::Try(Box::new(ast.name("result")))),
                )));
                return Ok(());
            }
            _ => false,
        };
        if !compatible
            || (!numeric || options.style == Some(b'c')) && (options.sign.is_some() || options.zero)
        {
            return Err(Diagnostic::new(
                span,
                format!(
                    "format options are incompatible with argument type `{ty}`; custom values support only '{{}}'"
                ),
            ));
        }
        body.push(ast.let_("options", ast.call(&format!("{fmt}.defaults"), vec![])));
        if let Some(fill) = options.fill {
            body.push(ast.set("fill", ast.int(fill as u32)));
        }
        if let Some(width) = options.width {
            body.push(ast.set("width", ast.int(width)));
        }
        if let Some(align) = options.align {
            let variant = match align {
                b'<' => "Left",
                b'^' => "Center",
                _ => "Right",
            };
            body.push(ast.set(
                "alignment",
                ast.field(ast.name(&format!("{fmt}.Alignment")), variant),
            ));
        }
        if let Some(sign) = options.sign {
            let variant = match sign {
                b'+' => "Always",
                b' ' => "Space",
                _ => "NegativeOnly",
            };
            body.push(ast.set("sign", ast.field(ast.name(&format!("{fmt}.Sign")), variant)));
        }
        if options.zero {
            body.push(ast.set("zero_pad", ast.expr(ExprKind::Bool(true))));
        }
        let style = options.style.unwrap_or(if matches!(ty, Type::Float(_)) {
            if options.precision.is_some() {
                b'f'
            } else {
                b'e'
            }
        } else {
            b'd'
        });
        if matches!(style, b'X' | b'E' | b'F') {
            body.push(ast.set("uppercase", ast.expr(ExprKind::Bool(true))));
        }
        let radix = match style {
            b'b' => 2,
            b'o' => 8,
            b'x' | b'X' => 16,
            _ => 10,
        };
        if radix != 10 {
            body.push(ast.set("radix", ast.int(radix)));
        }
        let mut arguments = vec![];
        let method = match ty {
            Type::Int { .. } if style == b'c' => {
                arguments.push(value);
                "codepoint"
            }
            Type::Int { signed, .. } => {
                arguments.push(ast.expr(ExprKind::Cast(
                    Box::new(value),
                    Type::Int {
                        signed: *signed,
                        bits: 64,
                    },
                )));
                if *signed { "signed" } else { "unsigned" }
            }
            Type::Float(_) => {
                arguments.push(ast.expr(ExprKind::Cast(Box::new(value), Type::Float(64))));
                arguments.push(ast.int(options.precision.unwrap_or(6)));
                arguments.push(ast.field(
                    ast.name(&format!("{fmt}.FloatStyle")),
                    if matches!(style, b'e' | b'E') {
                        "Scientific"
                    } else {
                        "Fixed"
                    },
                ));
                "floating"
            }
            Type::Bool => {
                arguments.push(value);
                "boolean"
            }
            Type::Str => {
                arguments.push(value);
                "padded_string"
            }
            _ => unreachable!(),
        };
        arguments.push(ast.unary(UnaryOp::Borrow, ast.name("options")));
        // Each field has its own options binding, released before the next field.
        body.push(ast.write(method, arguments));
        Ok(())
    }
}
