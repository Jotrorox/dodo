//! C calling-convention lowering of `@repr(C)` structs passed and returned by
//! value. LLVM leaves aggregate classification to the frontend, so each
//! struct becomes the IR operands the target's C compiler would use:
//!
//! - x86-64 System V: up to two eightbytes, each INTEGER or SSE, travel in
//!   registers when enough remain for the whole struct; otherwise, and above
//!   16 bytes, the struct is copied to the stack (`byval`). Results that are
//!   not returned in registers use caller storage (`sret`).
//! - AArch64 (AAPCS64, including Apple and Windows): homogeneous aggregates
//!   of up to four floats or doubles use vector registers; other structs of up
//!   to 16 bytes travel as one or two general registers; larger ones are
//!   passed as a pointer to a caller copy and returned through `sret`.
//! - Windows x64: structs of 1, 2, 4 or 8 bytes travel as that integer; all
//!   others are passed as a pointer to a caller copy and returned via `sret`.
//!
//! Primitive operands are unchanged. Other targets reject by-value structs.
use super::*;

/// How one C parameter or result crosses the call.
#[derive(Clone, Debug)]
pub(super) enum Pass<'ctx> {
    /// The Dodo value itself.
    Direct,
    /// The value's bytes reinterpreted as these IR operands, which lie at
    /// consecutive eightbytes. A result with two pieces returns them as one
    /// literal struct.
    Coerce(Vec<LlvmType<'ctx>>),
    /// A pointer to a copy: the caller's (`byval` with this alignment) or one
    /// the caller owns and passes explicitly. As a result, `sret` storage.
    Indirect { byval: Option<u32> },
}
#[derive(Clone, Debug)]
pub(super) struct Lowering<'ctx> {
    pub ret: Pass<'ctx>,
    pub params: Vec<Pass<'ctx>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Scalar {
    Integer,
    Float,
    Double,
}

#[derive(Clone, Copy, PartialEq)]
enum Abi {
    SysV,
    Win64,
    Aapcs64,
    Other,
}

impl<'a, 'ctx> Codegen<'a, 'ctx> {
    fn c_abi_kind(&self) -> Abi {
        let triple = self.module.get_triple();
        let triple = triple.as_str().to_string_lossy();
        let arch = triple.split('-').next().unwrap_or_default();
        if arch == "x86_64" {
            if triple.contains("windows") || triple.contains("uefi") {
                Abi::Win64
            } else {
                Abi::SysV
            }
        } else if arch == "aarch64" || arch == "arm64" {
            Abi::Aapcs64
        } else {
            Abi::Other
        }
    }
    /// The lowering of a C signature, or None when every operand is direct.
    pub(super) fn c_lowering(&self, f: &Function) -> Result<Option<Lowering<'ctx>>> {
        let by_value = |ty: &Type| matches!(ty, Type::Named(_));
        if !by_value(&f.ret) && !f.params.iter().any(|p| by_value(&p.ty)) {
            return Ok(None);
        }
        let abi = self.c_abi_kind();
        if abi == Abi::Other {
            let triple = self.module.get_triple();
            return Err(error(format!(
                "`{}` passes a struct by value, which the C ABI of target `{}` does not support yet; pass a raw pointer instead",
                f.name.rsplit('.').next().unwrap_or(&f.name),
                triple.as_str().to_string_lossy()
            )));
        }
        // System V tracks the argument registers left, since a struct
        // either fits entirely or moves to the stack.
        let (mut integers, mut floats) = (6u32, 8u32);
        let ret = if by_value(&f.ret) {
            let pass = self.c_struct_pass(&f.ret, abi, true, &mut integers, &mut floats)?;
            if matches!(pass, Pass::Indirect { .. }) {
                integers -= 1;
            }
            pass
        } else {
            Pass::Direct
        };
        let mut params = Vec::new();
        for p in &f.params {
            params.push(if by_value(&p.ty) {
                self.c_struct_pass(&p.ty, abi, false, &mut integers, &mut floats)?
            } else {
                if abi == Abi::SysV {
                    let (needed_integers, needed_floats) = match &p.ty {
                        Type::Float(_) => (0, 1),
                        Type::Int { bits: 128, .. } => (2, 0),
                        _ => (1, 0),
                    };
                    if integers >= needed_integers && floats >= needed_floats {
                        integers -= needed_integers;
                        floats -= needed_floats;
                    }
                }
                Pass::Direct
            });
        }
        Ok(Some(Lowering { ret, params }))
    }
    fn c_struct_pass(
        &self,
        ty: &Type,
        abi: Abi,
        result: bool,
        integers: &mut u32,
        floats: &mut u32,
    ) -> Result<Pass<'ctx>> {
        let llvm = self.ty(ty)?;
        let size = self.data.get_abi_size(&llvm);
        let align = self.data.get_abi_alignment(&llvm);
        let mut scalars = Vec::new();
        self.c_scalars(ty, 0, &mut scalars)?;
        let i64_type = self.context.i64_type();
        let integer = |bytes: u64| {
            self.context
                .custom_width_int_type(std::num::NonZeroU32::new(bytes as u32 * 8).unwrap())
                .unwrap()
        };
        Ok(match abi {
            Abi::SysV => {
                if size > 16 {
                    return Ok(Pass::Indirect {
                        byval: (!result).then_some(align.max(8)),
                    });
                }
                let mut pieces = Vec::new();
                let (mut needed_integers, mut needed_floats) = (0, 0);
                for eightbyte in 0..size.div_ceil(8) {
                    let (start, end) = (eightbyte * 8, (eightbyte * 8 + 8).min(size));
                    let inside: Vec<_> = scalars
                        .iter()
                        .filter(|(offset, bytes, _)| *offset < end && offset + bytes > start)
                        .collect();
                    if !inside.is_empty() && inside.iter().all(|(_, _, s)| *s != Scalar::Integer) {
                        needed_floats += 1;
                        pieces.push(if end - start > 4 {
                            self.context.f64_type()
                        } else {
                            self.context.f32_type()
                        });
                    } else {
                        needed_integers += 1;
                        pieces.push(i64_type);
                    }
                }
                // Results have two registers of each class; arguments must
                // fit in the registers that remain, or go to the stack.
                if result {
                    Pass::Coerce(pieces)
                } else if needed_integers <= *integers && needed_floats <= *floats {
                    *integers -= needed_integers;
                    *floats -= needed_floats;
                    Pass::Coerce(pieces)
                } else {
                    Pass::Indirect {
                        byval: Some(align.max(8)),
                    }
                }
            }
            Abi::Aapcs64 => {
                let first = scalars[0].2;
                if scalars.len() <= 4
                    && first != Scalar::Integer
                    && scalars.iter().all(|s| s.2 == first)
                {
                    let element = if first == Scalar::Float {
                        self.context.f32_type()
                    } else {
                        self.context.f64_type()
                    };
                    Pass::Coerce(vec![element.array_type(scalars.len() as u32)])
                } else if size > 16 {
                    Pass::Indirect { byval: None }
                } else if align >= 16 {
                    Pass::Coerce(vec![integer(16)])
                } else if size <= 8 {
                    Pass::Coerce(vec![i64_type])
                } else {
                    Pass::Coerce(vec![i64_type.array_type(2)])
                }
            }
            Abi::Win64 => {
                if matches!(size, 1 | 2 | 4 | 8) {
                    Pass::Coerce(vec![integer(size)])
                } else {
                    Pass::Indirect { byval: None }
                }
            }
            Abi::Other => unreachable!("rejected before classification"),
        })
    }
    /// The scalars of a C value at their byte offsets, with their sizes.
    fn c_scalars(&self, ty: &Type, offset: u64, out: &mut Vec<(u64, u64, Scalar)>) -> Result<()> {
        match ty {
            Type::Named(name) => {
                let declaration = self
                    .program
                    .structs
                    .iter()
                    .find(|s| &s.name == name)
                    .ok_or_else(|| error(format!("unresolved type {name}")))?;
                let llvm = self.ty(ty)?;
                for (index, field) in declaration.fields.iter().enumerate() {
                    let at = self
                        .data
                        .offset_of_element(&llvm, index as u32)
                        .ok_or_else(|| error(format!("no field {index} in {name}")))?;
                    self.c_scalars(&field.ty, offset + at, out)?;
                }
            }
            Type::Array(length, element) => {
                let stride = self.data.get_abi_size(&self.ty(element)?);
                for index in 0..*length as u64 {
                    self.c_scalars(element, offset + index * stride, out)?;
                }
            }
            _ => {
                let size = self.data.get_abi_size(&self.ty(ty)?);
                let scalar = match ty {
                    Type::Float(32) => Scalar::Float,
                    Type::Float(_) => Scalar::Double,
                    _ => Scalar::Integer,
                };
                out.push((offset, size, scalar));
            }
        }
        Ok(())
    }
    /// The IR function type of a lowered C signature.
    pub(super) fn c_fn_type(
        &self,
        f: &Function,
        lowering: &Lowering<'ctx>,
    ) -> Result<LlvmType<'ctx>> {
        let pointer = self.context.ptr_type(0);
        let mut params = Vec::new();
        if matches!(lowering.ret, Pass::Indirect { .. }) {
            params.push(pointer);
        }
        for (p, pass) in f.params.iter().zip(&lowering.params) {
            match pass {
                Pass::Direct => params.push(self.ty(&p.ty)?),
                Pass::Coerce(pieces) => params.extend(pieces),
                Pass::Indirect { .. } => params.push(pointer),
            }
        }
        let ret = match &lowering.ret {
            Pass::Direct if f.ret == Type::Void => self.context.void_type(),
            Pass::Direct => self.ty(&f.ret)?,
            Pass::Coerce(pieces) => self.coerced_type(pieces),
            Pass::Indirect { .. } => self.context.void_type(),
        };
        Ok(ret.fn_type(&params, false))
    }
    fn coerced_type(&self, pieces: &[LlvmType<'ctx>]) -> LlvmType<'ctx> {
        if let [single] = pieces {
            *single
        } else {
            self.context.struct_type(pieces, false)
        }
    }
    /// The IR operand index of each Dodo parameter's first operand.
    pub(super) fn c_operand_indices(lowering: &Lowering<'ctx>) -> Vec<u32> {
        let mut next = u32::from(matches!(lowering.ret, Pass::Indirect { .. }));
        lowering
            .params
            .iter()
            .map(|pass| {
                let index = next;
                next += match pass {
                    Pass::Coerce(pieces) => pieces.len() as u32,
                    _ => 1,
                };
                index
            })
            .collect()
    }
    /// `sret`, `byval` and `align` attributes of a lowered C signature.
    pub(super) fn c_lowering_attributes(
        &self,
        f: &Function,
        lowering: &Lowering<'ctx>,
    ) -> Result<Vec<(AttributeLoc, Attribute<'ctx>)>> {
        let mut attributes = Vec::new();
        let kind = Attribute::get_named_enum_kind_id;
        if matches!(lowering.ret, Pass::Indirect { .. }) {
            let ty = self.ty(&f.ret)?;
            let align = self.data.get_abi_alignment(&ty);
            attributes.push((
                AttributeLoc::Param(0),
                self.context.create_type_attribute(kind("sret"), ty),
            ));
            attributes.push((
                AttributeLoc::Param(0),
                self.context
                    .create_enum_attribute(kind("align"), align.into()),
            ));
        }
        let indices = Self::c_operand_indices(lowering);
        for ((p, pass), index) in f.params.iter().zip(&lowering.params).zip(indices) {
            if let Pass::Indirect { byval: Some(align) } = pass {
                attributes.push((
                    AttributeLoc::Param(index),
                    self.context
                        .create_type_attribute(kind("byval"), self.ty(&p.ty)?),
                ));
                attributes.push((
                    AttributeLoc::Param(index),
                    self.context
                        .create_enum_attribute(kind("align"), (*align).into()),
                ));
            }
        }
        Ok(attributes)
    }
    /// Storage that holds both a value of `ty` and the coerced pieces.
    fn coercion_slot(&self, ty: LlvmType<'ctx>, pieces: &[LlvmType<'ctx>]) -> Result<Value<'ctx>> {
        let coerced = self.coerced_type(pieces);
        let size = self
            .data
            .get_abi_size(&ty)
            .max(self.data.get_abi_size(&coerced));
        let align = self
            .data
            .get_abi_alignment(&ty)
            .max(self.data.get_abi_alignment(&coerced));
        let slot = self.alloca(self.context.i8_type().array_type(size as u32), "c.coerce")?;
        slot.set_alignment(align)?;
        Ok(slot)
    }
    /// The IR operands of one by-value argument.
    pub(super) fn c_argument(
        &self,
        value: Value<'ctx>,
        ty: &Type,
        pass: &Pass<'ctx>,
    ) -> Result<Vec<Value<'ctx>>> {
        Ok(match pass {
            Pass::Direct => vec![value],
            Pass::Coerce(pieces) => {
                let slot = self.coercion_slot(self.ty(ty)?, pieces)?;
                self.builder.build_store(slot, value)?;
                let layout = self.coerced_type(pieces);
                let mut operands = Vec::new();
                for (index, piece) in pieces.iter().enumerate() {
                    let at = if pieces.len() == 1 {
                        slot
                    } else {
                        self.builder
                            .build_struct_gep(layout, slot, index as u32, "c.piece")?
                    };
                    operands.push(self.builder.build_load(*piece, at, "c.piece")?);
                }
                operands
            }
            Pass::Indirect { .. } => {
                let copy = self.alloca(self.ty(ty)?, "c.argument")?;
                self.store_value(copy, value)?;
                vec![copy]
            }
        })
    }
    /// The Dodo value of one by-value parameter from its IR operands.
    pub(super) fn c_parameter(
        &self,
        operands: &[Value<'ctx>],
        ty: &Type,
        pass: &Pass<'ctx>,
    ) -> Result<Value<'ctx>> {
        Ok(match pass {
            Pass::Direct => operands[0],
            Pass::Coerce(pieces) => {
                let llvm = self.ty(ty)?;
                let slot = self.coercion_slot(llvm, pieces)?;
                let layout = self.coerced_type(pieces);
                for (index, operand) in operands.iter().enumerate() {
                    let at = if pieces.len() == 1 {
                        slot
                    } else {
                        self.builder
                            .build_struct_gep(layout, slot, index as u32, "c.piece")?
                    };
                    self.builder.build_store(at, *operand)?;
                }
                self.builder.build_load(llvm, slot, "c.parameter")?
            }
            Pass::Indirect { .. } => self.load(operands[0], ty)?,
        })
    }
    /// A coerced C result as its Dodo value.
    pub(super) fn c_result(
        &self,
        value: Value<'ctx>,
        ty: &Type,
        pieces: &[LlvmType<'ctx>],
    ) -> Result<Value<'ctx>> {
        let llvm = self.ty(ty)?;
        let slot = self.coercion_slot(llvm, pieces)?;
        self.builder.build_store(slot, value)?;
        self.builder.build_load(llvm, slot, "c.result")
    }
    /// Return a Dodo value as coerced C result pieces.
    pub(super) fn c_return(&self, value: Value<'ctx>, pieces: &[LlvmType<'ctx>]) -> Result<()> {
        let slot = self.coercion_slot(value.get_type(), pieces)?;
        self.builder.build_store(slot, value)?;
        let result = self
            .builder
            .build_load(self.coerced_type(pieces), slot, "c.result")?;
        self.builder.build_return(Some(&result))?;
        Ok(())
    }
    /// A call to a C function that passes or returns structs by value.
    pub(super) fn c_call(
        &mut self,
        name: &str,
        function: Value<'ctx>,
        lowering: &Lowering<'ctx>,
        args: &[Expr],
        ret: &Type,
    ) -> Result<Value<'ctx>> {
        let storage = match lowering.ret {
            Pass::Indirect { .. } => Some(self.alloca(self.ty(ret)?, "call.result")?),
            _ => None,
        };
        let mut pending = Vec::new();
        let mut operands: Vec<Value<'ctx>> = storage.into_iter().collect();
        for (e, pass) in args.iter().zip(&lowering.params) {
            let value = self.stage(e, &mut pending)?;
            operands.extend(self.c_argument(value, &e.ty, pass)?);
        }
        self.transfer(&pending)?;
        // A @link_name declaration may bind the symbol with its own signature.
        let call = match self.foreign_signatures.get(name) {
            Some(&ty) => self
                .builder
                .build_indirect_call(ty, function, &operands, "call")?,
            None => self.builder.build_call(function, &operands, "call")?,
        };
        let declaration = self
            .program
            .functions
            .iter()
            .find(|f| f.name == name && f.extern_)
            .ok_or_else(|| error(format!("unknown C function {name}")))?;
        for (location, attribute) in self.c_abi_attributes(declaration)? {
            call.add_attribute(location, attribute);
        }
        match (&lowering.ret, storage) {
            (_, Some(storage)) => self.load(storage, ret),
            (Pass::Coerce(pieces), _) => self.c_result(call, ret, pieces),
            _ if *ret == Type::Void => Ok(self.context.i8_type().const_zero()),
            _ => Ok(call),
        }
    }
}
