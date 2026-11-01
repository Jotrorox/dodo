//! `gpu.wgsl(...)`, the reserved `@compiler(wgsl)` declaration of std/gpu:
//! its string literals form one WGSL module, which [`crate::wgsl`]
//! translates for every backend. The call becomes a `ShaderSource` literal
//! before the production checker runs. Its SPIR-V words go into a hidden
//! constant array of the program, so the literal borrows static storage and
//! may be returned from functions declared `from(static)`.
use super::*;
use crate::wgsl;

impl Expander {
    pub(super) fn wgsl_shader(
        &mut self,
        template: &Function,
        type_args: &[Type],
        args: &[Expr],
        span: Span,
    ) -> Check<ExprKind> {
        if !type_args.is_empty() {
            return Err(Diagnostic::new(span, "gpu.wgsl takes no type arguments"));
        }
        if args.is_empty() {
            return Err(Diagnostic::new(
                span,
                "gpu.wgsl takes the shader as string literals",
            ));
        }
        // Remember where each literal's text starts in the module, to point
        // diagnostics into it; a literal with escapes points at the whole
        // literal instead.
        let mut source = String::new();
        let mut pieces = Vec::new();
        for arg in args {
            let ExprKind::String(bytes, false) = &arg.kind else {
                return Err(Diagnostic::new(
                    arg.span,
                    "gpu.wgsl takes string literals, since it translates the shader at compile time",
                ));
            };
            let text = std::str::from_utf8(bytes)
                .map_err(|_| Diagnostic::new(arg.span, "a WGSL shader must be UTF-8"))?;
            if !source.is_empty() {
                source.push('\n');
            }
            let exact = arg.span.end - arg.span.start == bytes.len() + 2;
            pieces.push((source.len(), text.len(), arg.span, exact));
            source.push_str(text);
        }
        let translation = wgsl::translate(&source).map_err(|error| {
            let place = error.span.and_then(|(start, end)| {
                let &(base, length, literal, exact) =
                    pieces.iter().rev().find(|(base, ..)| *base <= start)?;
                if !exact || start > base + length {
                    return Some(literal);
                }
                let end = end.min(base + length).max(start + 1);
                Some(Span {
                    start: literal.start + 1 + (start - base),
                    end: literal.start + 1 + (end - base),
                })
            });
            Diagnostic::new(
                place.unwrap_or(args[0].span),
                format!("invalid WGSL shader: {}", error.message),
            )
        })?;
        let Type::Named(name) = &template.ret else {
            return Err(Diagnostic::new(span, "gpu.wgsl must return ShaderSource"));
        };
        let u32_type = Type::Int {
            signed: false,
            bits: 32,
        };
        let expr = |kind| Expr::new(kind, span);
        let length = translation.spirv.len();
        let words = translation
            .spirv
            .into_iter()
            .map(|word| expr(ExprKind::Int(word.into(), Some(u32_type.clone()))))
            .collect();
        let array_type = Type::Array(length, Box::new(u32_type));
        let constant = format!("wgsl$spirv${}", self.wgsl_constants.len());
        self.constants.insert(constant.clone(), array_type.clone());
        self.wgsl_constants.push(Constant {
            name: constant.clone(),
            ty: array_type.clone(),
            value: expr(ExprKind::Array(array_type, words)),
            public: false,
            static_: false,
            mutable: false,
            placement: Placement::default(),
            span,
        });
        let spirv = expr(ExprKind::Unary(
            UnaryOp::Borrow,
            Box::new(expr(ExprKind::Name(constant))),
        ));
        let text = |value: String| expr(ExprKind::String(value.into_bytes(), false));
        Ok(ExprKind::Struct(
            name.to_owned(),
            vec![
                ("spirv".to_owned(), spirv),
                ("msl".to_owned(), text(translation.msl)),
                ("glsl".to_owned(), text(translation.glsl)),
            ],
        ))
    }
}
