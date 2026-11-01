//! Assembly: operand types, register conflicts, and the template's operand
//! references, for inline assembly, naked function bodies, and top-level
//! (global) assembly. The backend lowers the checked template with
//! [`render_template`], so both agree on how `{N}` placeholders are numbered.
use super::ownership::Value;
use super::{Check, Checker, constant_expression};
use crate::ast::*;
use crate::diagnostic::Diagnostic;
use std::collections::HashSet;

/// What a template's `{N}` reference to one source operand becomes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TemplateOperand {
    /// An LLVM inline asm operand, by its LLVM operand number.
    Register(usize),
    /// Text written in place of the reference: a `sym` operand's assembler
    /// name or a `const` operand's decimal value.
    Text(String),
}

/// LLVM numbers outputs before inputs, while the source numbers operands in
/// the order written. Returns, for each source operand, its LLVM operand
/// index, or `None` for `sym` and `const` operands, which are not LLVM
/// operands but text in the template.
pub(crate) fn llvm_operand_indices(asm: &InlineAsm) -> Vec<Option<usize>> {
    let outputs = asm
        .operands
        .iter()
        .filter(|operand| {
            matches!(
                operand.kind,
                AsmOperandKind::Out(_) | AsmOperandKind::InOut(_)
            )
        })
        .count();
    let (mut output, mut input) = (0, outputs);
    asm.operands
        .iter()
        .map(|operand| {
            let slot = match operand.kind {
                AsmOperandKind::In(_) => &mut input,
                AsmOperandKind::Out(_) | AsmOperandKind::InOut(_) => &mut output,
                AsmOperandKind::Sym(_) | AsmOperandKind::Const(_) => return None,
            };
            *slot += 1;
            Some(*slot - 1)
        })
        .collect()
}

/// Translate a source template, which names operands `{N}`, `{}`, or
/// `{N:modifier}` and escapes braces as `{{` and `}}`. Inline assembly
/// (`inline`) becomes LLVM's `$N` / `${N:modifier}` form, with literal `$`
/// doubled; module-level assembly is passed to the assembler verbatim, so
/// it keeps `$` and has only text operands.
pub(crate) fn render_template(
    template: &str,
    operands: &[TemplateOperand],
    inline: bool,
) -> Result<String, String> {
    let mut result = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    let mut next = 0;
    let dollars = |text: &str| {
        if inline {
            text.replace('$', "$$")
        } else {
            text.to_owned()
        }
    };
    while let Some(ch) = chars.next() {
        match ch {
            '$' if inline => result.push_str("$$"),
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                result.push('}');
            }
            '}' => {
                return Err(
                    "unmatched `}` in assembly template; write `}}` for a literal brace".into(),
                );
            }
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                result.push('{');
            }
            '{' => {
                let mut reference = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(ch) => reference.push(ch),
                        None => {
                            return Err(
                                "unclosed `{` in assembly template; write `{{` for a literal brace"
                                    .into(),
                            );
                        }
                    }
                }
                let (number, modifier) = match reference.split_once(':') {
                    Some((number, modifier)) => (number, Some(modifier)),
                    None => (reference.as_str(), None),
                };
                let operand = if number.is_empty() {
                    next += 1;
                    next - 1
                } else {
                    number.parse::<usize>().map_err(|_| {
                        format!("`{{{reference}}}` is not an operand reference; use `{{0}}`, `{{1}}`, ... or `{{}}`")
                    })?
                };
                let Some(target) = operands.get(operand) else {
                    return Err(format!(
                        "assembly template refers to operand {operand}, but the asm has {} operand{}",
                        operands.len(),
                        if operands.len() == 1 { "" } else { "s" }
                    ));
                };
                match (target, modifier) {
                    (TemplateOperand::Text(text), None) => result.push_str(&dollars(text)),
                    (TemplateOperand::Text(_), Some(_)) => {
                        return Err(format!(
                            "`{{{reference}}}` names a `sym` or `const` operand, which takes no modifier"
                        ));
                    }
                    (TemplateOperand::Register(index), Some(modifier))
                        if !modifier.is_empty()
                            && modifier.chars().all(|ch| ch.is_ascii_alphanumeric()) =>
                    {
                        result.push_str(&format!("${{{index}:{modifier}}}"));
                    }
                    (TemplateOperand::Register(_), Some(_)) => {
                        return Err(format!(
                            "`{{{reference}}}` has an invalid operand modifier; modifiers are letters, such as `{{0:w}}`"
                        ));
                    }
                    (TemplateOperand::Register(index), None) => {
                        result.push_str(&format!("${index}"))
                    }
                }
            }
            _ => result.push(ch),
        }
    }
    Ok(result)
}

/// The template operands of `asm`, with `text` supplying the text of each
/// `sym` or `const` operand.
pub(crate) fn template_operands(
    asm: &InlineAsm,
    mut text: impl FnMut(&AsmOperandKind) -> Result<String, String>,
) -> Result<Vec<TemplateOperand>, String> {
    asm.operands
        .iter()
        .zip(llvm_operand_indices(asm))
        .map(|(operand, index)| match index {
            Some(index) => Ok(TemplateOperand::Register(index)),
            None => text(&operand.kind).map(TemplateOperand::Text),
        })
        .collect()
}

/// The value of a checked `const` operand.
pub(crate) fn constant_operand(value: &Expr, bits: u32) -> Result<i128, String> {
    match crate::consteval::eval(value, bits)? {
        crate::consteval::Scalar::Int(value) => Ok(value),
        _ => Err("an asm `const` operand must be an integer constant expression".into()),
    }
}

impl Checker<'_> {
    pub(super) fn inline_asm(&mut self, asm: &mut InlineAsm, span: Span) -> Check<Value> {
        self.unsafe_required(span, "inline assembly")?;
        Self::asm_template(asm, span)?;
        let bits = self.context.pointer_bits;
        let supported = |ty: &Type| match ty {
            Type::Int { bits: width, .. } => *width <= bits,
            Type::Raw(..) | Type::Fn(..) | Type::ExternFn(..) => true,
            _ => false,
        };
        let unsupported = |ty: &Type, at: Span| {
            Diagnostic::new(
                at,
                format!(
                    "asm operands must be integers of at most {bits} bits, raw pointers, or function pointers, not `{ty}`"
                ),
            )
        };
        let mut outputs = vec![];
        let mut written = HashSet::new();
        let mut read = HashSet::new();
        for operand in &mut asm.operands {
            let named = match &operand.register {
                AsmRegister::Named(name) => Some(name.to_ascii_lowercase()),
                AsmRegister::Any => None,
            };
            let writes = matches!(
                operand.kind,
                AsmOperandKind::Out(_) | AsmOperandKind::InOut(_)
            );
            let reads = matches!(
                operand.kind,
                AsmOperandKind::In(_) | AsmOperandKind::InOut(_)
            );
            if let Some(name) = &named
                && ((writes && !written.insert(name.clone()))
                    || (reads && !read.insert(name.clone())))
            {
                return Err(Diagnostic::new(
                    operand.span,
                    format!("register \"{name}\" is used by two asm operands"),
                ));
            }
            match &mut operand.kind {
                AsmOperandKind::In(value) | AsmOperandKind::InOut(value) => {
                    let value = self.expr(value, None, true)?;
                    if !supported(&value.ty) {
                        return Err(unsupported(&value.ty, operand.span));
                    }
                    if writes {
                        outputs.push(value.ty);
                    }
                }
                AsmOperandKind::Out(ty) => {
                    if !supported(ty) {
                        return Err(unsupported(ty, operand.span));
                    }
                    outputs.push(ty.clone());
                }
                AsmOperandKind::Sym(path) => self.asm_symbol(path, operand.span)?,
                AsmOperandKind::Const(value) => self.asm_constant(value, operand.span)?,
            }
        }
        for clobber in &asm.clobbers {
            let name = clobber.to_ascii_lowercase();
            if written.contains(&name) {
                return Err(Diagnostic::new(
                    span,
                    format!("register \"{clobber}\" is both an asm output and a clobber"),
                ));
            }
        }
        if asm.pure && outputs.is_empty() {
            return Err(Diagnostic::new(
                span,
                "options(pure) asm must have an output; without one its only effect would be removed",
            ));
        }
        let several = outputs.len() > 1;
        Ok(Value {
            ty: match &asm.tuple {
                Some(tuple) if several => tuple.clone(),
                _ => asm_result(outputs),
            },
        })
    }
    /// Check assembly that runs outside the compiler's register allocation:
    /// global assembly (`what` is "global asm") and the body of a naked
    /// function. Its operands can only be text: `sym` and `const`.
    pub(super) fn textual_asm(&mut self, asm: &mut InlineAsm, what: &str) -> Check<()> {
        for operand in &mut asm.operands {
            match &mut operand.kind {
                AsmOperandKind::Sym(path) => self.asm_symbol(path, operand.span)?,
                AsmOperandKind::Const(value) => self.asm_constant(value, operand.span)?,
                _ => {
                    return Err(Diagnostic::new(
                        operand.span,
                        format!(
                            "{what} takes only `sym` and `const` operands; it has no registers allocated for it"
                        ),
                    ));
                }
            }
        }
        if !asm.clobbers.is_empty() || asm.pure || asm.nomem {
            return Err(Diagnostic::new(
                asm.span,
                format!("{what} has no clobbers or options; it is not part of the code around it"),
            ));
        }
        Self::asm_template(asm, asm.span)
    }
    /// Check the template's operand references and braces.
    fn asm_template(asm: &InlineAsm, span: Span) -> Check<()> {
        template_operands(asm, |_| Ok(String::new()))
            .and_then(|operands| render_template(&asm.template, &operands, true))
            .map(|_| ())
            .map_err(|message| Diagnostic::new(span, message))
    }
    /// Resolve a `sym` operand to the qualified name of a function or static.
    fn asm_symbol(&mut self, path: &mut String, span: Span) -> Check<()> {
        let first = path.split('.').next().unwrap_or_default();
        let local = self.lookup(first).is_some();
        if !local && let Some(constant) = self.context.constants.get(path.as_str()) {
            if !constant.static_ {
                return Err(Diagnostic::new(
                    span,
                    format!(
                        "`sym` names a function or a static; `const` `{path}` has no storage of its own"
                    ),
                ));
            }
            if !constant.public && super::type_namespace(path) != self.namespace {
                return Err(Diagnostic::new(
                    span,
                    format!("static `{path}` is private to its package"),
                ));
            }
            return Ok(());
        }
        if !local && let Some(signature) = self.context.functions.get(path.as_str()) {
            if let Some((owner, _)) = path.rsplit_once('.')
                && self.context.interfaces.contains_key(owner)
            {
                return Err(Diagnostic::new(
                    span,
                    format!("interface method `{path}` has no symbol of its own"),
                ));
            }
            if !signature.public && self.context.function_namespace(path) != self.namespace {
                return Err(Diagnostic::new(
                    span,
                    format!("function `{path}` is private to its package"),
                ));
            }
            if let Some(vector) = &signature.interrupt {
                return Err(super::interrupt_handler_use(path, vector, span));
            }
            if signature.unavailable || path.contains(['$', '#']) {
                return Err(Diagnostic::new(
                    span,
                    format!("`sym` cannot name the generic instance `{path}`"),
                ));
            }
            return Ok(());
        }
        Err(Diagnostic::new(
            span,
            format!(
                "`sym` needs a non-generic function or a static, and `{path}` is not one in scope"
            ),
        ))
    }
    /// Check a `const` operand: an integer constant expression.
    fn asm_constant(&mut self, value: &mut Expr, span: Span) -> Check<()> {
        let ty = self.expr(value, None, false)?.ty;
        if !ty.is_integer() || !constant_expression(value) {
            return Err(Diagnostic::new(
                span,
                "an asm `const` operand must be an integer constant expression",
            ));
        }
        constant_operand(value, self.context.pointer_bits)
            .map(|_| ())
            .map_err(|message| Diagnostic::new(span, message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_number_outputs_before_inputs() {
        // in, out, inout: LLVM sees out=0, inout=1, in=2.
        let indices = [2, 0, 1].map(TemplateOperand::Register);
        let llvm_template = |template: &str, operands: &[TemplateOperand]| {
            render_template(template, operands, true)
        };
        assert_eq!(
            llvm_template("add {1}, {0}, {2}", &indices).unwrap(),
            "add $0, $2, $1"
        );
        assert_eq!(llvm_template("mov {}, {}", &indices).unwrap(), "mov $2, $0");
        assert_eq!(
            llvm_template("ldr {1:w}, [{{x}}] $5", &indices).unwrap(),
            "ldr ${0:w}, [{x}] $$5"
        );
        assert!(
            llvm_template("{3}", &indices)
                .unwrap_err()
                .contains("operand 3")
        );
        assert!(
            llvm_template("{", &indices)
                .unwrap_err()
                .contains("unclosed")
        );
        assert!(
            llvm_template("}", &indices)
                .unwrap_err()
                .contains("unmatched")
        );
        assert!(
            llvm_template("{x}", &indices)
                .unwrap_err()
                .contains("not an operand")
        );
        assert!(
            llvm_template("{0:}", &indices)
                .unwrap_err()
                .contains("modifier")
        );
    }

    #[test]
    fn text_operands_are_written_into_the_template() {
        let operands = [
            TemplateOperand::Text("_dodo.app.f".into()),
            TemplateOperand::Register(0),
            TemplateOperand::Text("a$b".into()),
        ];
        assert_eq!(
            render_template("bl {0}\nmov {1}, {2}", &operands, true).unwrap(),
            "bl _dodo.app.f\nmov $0, a$$b"
        );
        // Module-level assembly is not an LLVM template: `$` stays literal.
        assert_eq!(
            render_template("call {} # $5 {{}}", &operands[..1], false).unwrap(),
            "call _dodo.app.f # $5 {}"
        );
        assert!(
            render_template("{0:w}", &operands, true)
                .unwrap_err()
                .contains("takes no modifier")
        );
    }
}
