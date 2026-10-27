//! Inline assembly: operand types, register conflicts, and the template's
//! operand references. The backend lowers the checked expression with
//! [`llvm_template`], so both agree on how `{N}` placeholders are numbered.
use super::ownership::Value;
use super::{Check, Checker};
use crate::ast::*;
use crate::diagnostic::Diagnostic;
use std::collections::HashSet;

/// LLVM numbers outputs before inputs, while the source numbers operands in
/// the order written. Returns, for each source operand, its LLVM operand index.
pub(crate) fn llvm_operand_indices(asm: &InlineAsm) -> Vec<usize> {
    let outputs = asm
        .operands
        .iter()
        .filter(|operand| !matches!(operand.kind, AsmOperandKind::In(_)))
        .count();
    let (mut output, mut input) = (0, outputs);
    asm.operands
        .iter()
        .map(|operand| {
            let slot = if matches!(operand.kind, AsmOperandKind::In(_)) {
                &mut input
            } else {
                &mut output
            };
            *slot += 1;
            *slot - 1
        })
        .collect()
}

/// Translate a source template, which names operands `{N}`, `{}`, or `{N:modifier}`
/// and escapes braces as `{{` and `}}`, into LLVM's `$N` / `${N:modifier}` form.
pub(crate) fn llvm_template(template: &str, indices: &[usize]) -> Result<String, String> {
    let mut result = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    let mut next = 0;
    while let Some(ch) = chars.next() {
        match ch {
            '$' => result.push_str("$$"),
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
                let Some(index) = indices.get(operand) else {
                    return Err(format!(
                        "assembly template refers to operand {operand}, but the asm has {} operand{}",
                        indices.len(),
                        if indices.len() == 1 { "" } else { "s" }
                    ));
                };
                match modifier {
                    Some(modifier)
                        if !modifier.is_empty()
                            && modifier.chars().all(|ch| ch.is_ascii_alphanumeric()) =>
                    {
                        result.push_str(&format!("${{{index}:{modifier}}}"));
                    }
                    Some(_) => {
                        return Err(format!(
                            "`{{{reference}}}` has an invalid operand modifier; modifiers are letters, such as `{{0:w}}`"
                        ));
                    }
                    None => result.push_str(&format!("${index}")),
                }
            }
            _ => result.push(ch),
        }
    }
    Ok(result)
}

impl Checker<'_> {
    pub(super) fn inline_asm(&mut self, asm: &mut InlineAsm, span: Span) -> Check<Value> {
        self.unsafe_required(span, "inline assembly")?;
        llvm_template(&asm.template, &llvm_operand_indices(asm))
            .map_err(|message| Diagnostic::new(span, message))?;
        let bits = self.context.pointer_bits;
        let supported = |ty: &Type| match ty {
            Type::Int { bits: width, .. } => *width <= bits,
            Type::Raw(..) | Type::Fn(..) => true,
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
            let writes = !matches!(operand.kind, AsmOperandKind::In(_));
            let reads = !matches!(operand.kind, AsmOperandKind::Out(_));
            if let Some(name) = &named {
                if (writes && !written.insert(name.clone()))
                    || (reads && !read.insert(name.clone()))
                {
                    return Err(Diagnostic::new(
                        operand.span,
                        format!("register \"{name}\" is used by two asm operands"),
                    ));
                }
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
            deps: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_number_outputs_before_inputs() {
        // in, out, inout: LLVM sees out=0, inout=1, in=2.
        let indices = [2, 0, 1];
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
}
