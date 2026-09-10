//! Target-width-aware evaluation of compile-time scalar expressions.
use crate::ast::{BinaryOp as B, Expr, ExprKind as E, Type, UnaryOp as U};

#[derive(Clone, Copy, Debug)]
pub enum Scalar {
    Int(i128),
    Float(f64),
    Bool(bool),
}
fn range(ty: &Type, pointer_bits: u32) -> Option<(i128, i128)> {
    if let Type::Int { signed, bits } = ty {
        let bits = if *bits == 0 { pointer_bits } else { *bits };
        Some(if *signed {
            (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
        } else {
            (0, (1i128 << bits) - 1)
        })
    } else {
        None
    }
}
fn fit(value: Scalar, ty: &Type, bits: u32) -> Result<Scalar, String> {
    match (value, ty) {
        (Scalar::Int(v), Type::Int { .. }) => {
            let (lo, hi) = range(ty, bits).unwrap();
            if v < lo || v > hi {
                Err(format!(
                    "constant {v} is outside {ty}; checked arithmetic cannot wrap"
                ))
            } else {
                Ok(value)
            }
        }
        (Scalar::Float(v), Type::Float(32)) => Ok(Scalar::Float((v as f32) as f64)),
        _ => Ok(value),
    }
}
pub fn eval(e: &Expr, bits: u32) -> Result<Scalar, String> {
    let value = match &e.kind {
        E::Constant(value, _) => eval(value, bits)?,
        E::Int(v, _) => Scalar::Int(*v as i128),
        E::Float(v, _) => Scalar::Float(*v),
        E::Bool(v) => Scalar::Bool(*v),
        E::Unary(U::Neg, x) => match &x.kind {
            E::Int(v, _) => Scalar::Int(-(*v as i128)),
            _ => match eval(x, bits)? {
                Scalar::Int(v) => Scalar::Int(-v),
                Scalar::Float(v) => Scalar::Float(-v),
                _ => return Err("invalid constant negation".into()),
            },
        },
        E::Unary(U::Not, x) => match eval(x, bits)? {
            Scalar::Bool(v) => Scalar::Bool(!v),
            _ => return Err("invalid constant Boolean operation".into()),
        },
        E::Unary(U::BitNot, x) => {
            let Scalar::Int(v) = eval(x, bits)? else {
                return Err("invalid constant bitwise operation".into());
            };
            let (_, max) = range(&e.ty, bits).unwrap();
            Scalar::Int(if matches!(e.ty, Type::Int { signed: false, .. }) {
                (!v) & max
            } else {
                !v
            })
        }
        E::Binary(op, a, b) => {
            let a = eval(a, bits)?;
            if (*op == B::And && matches!(a, Scalar::Bool(false)))
                || (*op == B::Or && matches!(a, Scalar::Bool(true)))
            {
                return Ok(a);
            }
            let b = eval(b, bits)?;
            match (a, b) {
                (Scalar::Int(a), Scalar::Int(b)) => match op {
                    B::Add => Scalar::Int(a.checked_add(b).ok_or("constant addition overflow")?),
                    B::Sub => Scalar::Int(a.checked_sub(b).ok_or("constant subtraction overflow")?),
                    B::Mul => {
                        Scalar::Int(a.checked_mul(b).ok_or("constant multiplication overflow")?)
                    }
                    B::Div | B::Rem => {
                        if b == 0 {
                            return Err("division by zero in constant".into());
                        }
                        let (min, _) = range(&e.ty, bits).unwrap();
                        if a == min && b == -1 {
                            return Err("constant division overflow".into());
                        }
                        Scalar::Int(if *op == B::Div { a / b } else { a % b })
                    }
                    B::BitAnd => Scalar::Int(a & b),
                    B::BitOr => Scalar::Int(a | b),
                    B::BitXor => Scalar::Int(a ^ b),
                    B::Shl | B::Shr => {
                        let Type::Int { bits: width, .. } = e.ty else {
                            return Err("invalid shift type".into());
                        };
                        let width = if width == 0 { bits } else { width };
                        if b < 0 || b >= width as i128 {
                            return Err("invalid shift count in constant".into());
                        }
                        Scalar::Int(if *op == B::Shl { a << b } else { a >> b })
                    }
                    B::Eq => Scalar::Bool(a == b),
                    B::Ne => Scalar::Bool(a != b),
                    B::Lt => Scalar::Bool(a < b),
                    B::Le => Scalar::Bool(a <= b),
                    B::Gt => Scalar::Bool(a > b),
                    B::Ge => Scalar::Bool(a >= b),
                    _ => return Err("invalid constant integer operation".into()),
                },
                (Scalar::Float(a), Scalar::Float(b)) => match op {
                    B::Add => Scalar::Float(a + b),
                    B::Sub => Scalar::Float(a - b),
                    B::Mul => Scalar::Float(a * b),
                    B::Div => Scalar::Float(a / b),
                    B::Rem => Scalar::Float(a % b),
                    B::Eq => Scalar::Bool(a == b),
                    B::Ne => Scalar::Bool(a != b),
                    B::Lt => Scalar::Bool(a < b),
                    B::Le => Scalar::Bool(a <= b),
                    B::Gt => Scalar::Bool(a > b),
                    B::Ge => Scalar::Bool(a >= b),
                    _ => return Err("invalid constant floating operation".into()),
                },
                (Scalar::Bool(a), Scalar::Bool(b)) => match op {
                    B::And => Scalar::Bool(a && b),
                    B::Or => Scalar::Bool(a || b),
                    B::Eq => Scalar::Bool(a == b),
                    B::Ne => Scalar::Bool(a != b),
                    _ => return Err("invalid constant Boolean operation".into()),
                },
                _ => return Err("incompatible constant operand types".into()),
            }
        }
        E::Cast(x, t) => match (eval(x, bits)?, t) {
            (Scalar::Int(v), Type::Float(_)) => Scalar::Float(v as f64),
            (Scalar::Float(v), Type::Int { .. }) => {
                let (lo, hi) = range(t, bits).unwrap();
                let upper = (hi + 1) as f64;
                if !v.is_finite() || v < (lo as f64) || v >= upper {
                    return Err("constant numeric conversion out of range".into());
                }
                Scalar::Int(v.trunc() as i128)
            }
            (Scalar::Float(v), Type::Float(32)) => {
                if !v.is_finite() || v.abs() > f32::MAX as f64 {
                    return Err("constant floating conversion out of range".into());
                }
                Scalar::Float(v)
            }
            (v, _) => v,
        },
        _ => return Err("expression is not a scalar constant".into()),
    };
    fit(value, &e.ty, bits)
}
