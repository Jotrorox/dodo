//! LLVM debug metadata and the source map shared with runtime diagnostics.
use super::llvm::{DebugBuilder, Metadata};
use super::*;
use crate::package::Source;
use llvm_sys::debuginfo::{LLVMDIFlagPrototyped, LLVMDIFlagPublic, LLVMDIFlagZero};

pub(super) struct SourceMap<'a> {
    pub sources: &'a [Source],
    lines: Vec<Vec<usize>>,
}
impl<'a> SourceMap<'a> {
    pub fn new(sources: &'a [Source]) -> Self {
        let lines = sources
            .iter()
            .map(|source| {
                std::iter::once(0)
                    .chain(
                        source
                            .text
                            .bytes()
                            .enumerate()
                            .filter_map(|(i, b)| (b == b'\n').then_some(i + 1)),
                    )
                    .collect()
            })
            .collect();
        Self { sources, lines }
    }
    pub fn location(&self, span: Span) -> Option<(usize, u32, u32)> {
        let index = self
            .sources
            .iter()
            .position(|s| span.start >= s.start && span.start <= s.start + s.text.len())?;
        let source = &self.sources[index];
        let mut offset = span.start - source.start;
        while !source.text.is_char_boundary(offset) {
            offset -= 1;
        }
        let line = self.lines[index].partition_point(|start| *start <= offset) - 1;
        let column = source.text[self.lines[index][line]..offset].chars().count();
        Some((index, line as u32 + 1, column as u32 + 1))
    }
}

pub(super) struct DebugInfo<'ctx> {
    pub builder: DebugBuilder<'ctx>,
    files: Vec<Metadata<'ctx>>,
    scopes: Vec<Metadata<'ctx>>,
    types: HashMap<Type, Metadata<'ctx>>,
    optimized: bool,
}
impl<'ctx> DebugInfo<'ctx> {
    pub fn new(
        _context: &'ctx Context,
        module: &Module<'ctx>,
        options: &Options,
        sources: &SourceMap<'_>,
    ) -> Result<Self> {
        let source = sources
            .sources
            .first()
            .ok_or_else(|| error("debug information requires source files"))?;
        let directory = source.path.parent().unwrap_or(std::path::Path::new("."));
        // Dodo has no DWARF language code yet. C gives debuggers a familiar
        // expression evaluator for our scalars and explicitly described layouts.
        let builder = DebugBuilder::new(
            module,
            &source
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy(),
            &directory.to_string_lossy(),
            options.optimization != 0,
        );
        let files = sources
            .sources
            .iter()
            .map(|s| {
                builder.create_file(
                    &s.path.file_name().unwrap_or_default().to_string_lossy(),
                    &s.path
                        .parent()
                        .unwrap_or(std::path::Path::new("."))
                        .to_string_lossy(),
                )
            })
            .collect();
        Ok(Self {
            builder,
            files,
            scopes: vec![],
            types: HashMap::new(),
            optimized: options.optimization != 0,
        })
    }
}

impl<'a, 'ctx> Codegen<'a, 'ctx> {
    fn debug_file(&self, span: Span) -> (Metadata<'ctx>, u32, u32) {
        let (index, line, column) = self.sources.location(span).unwrap_or((0, 0, 0));
        (self.debug.as_ref().unwrap().files[index], line, column)
    }
    pub(super) fn location(&mut self, span: Span) {
        self.span = span;
        if let Some(debug) = &self.debug
            && let Some(scope) = debug.scopes.last()
        {
            let (_, line, column) = self.debug_file(span);
            self.builder
                .set_current_debug_location(debug.builder.create_debug_location(
                    self.context,
                    line,
                    column,
                    *scope,
                    None,
                ));
        }
    }
    pub(super) fn push_scope(&mut self) {
        self.scopes.push(vec![]);
        if self.debug.is_some() {
            let (file, line, column) = self.debug_file(self.span);
            let debug = self.debug.as_mut().unwrap();
            let scope = debug.builder.create_lexical_block(
                *debug.scopes.last().unwrap(),
                file,
                line,
                column,
            );
            debug.scopes.push(scope);
            self.location(self.span);
        }
    }
    pub(super) fn pop_scope(&mut self) {
        self.scopes.pop();
        if let Some(debug) = &mut self.debug {
            debug.scopes.pop();
        }
        self.location(self.span);
    }
    pub(super) fn debug_function(&mut self, f: &Function, function: Value<'ctx>) -> Result<()> {
        if self.debug.is_none() {
            return Ok(());
        }
        let ret = if f.ret == Type::Void {
            None
        } else {
            Some(self.debug_type(&f.ret)?)
        };
        let params = f
            .params
            .iter()
            .map(|p| self.debug_type(&p.ty))
            .collect::<Result<Vec<_>>>()?;
        let (file, line, _) = self.debug_file(f.span);
        let debug = self.debug.as_mut().unwrap();
        let signature = debug
            .builder
            .create_subroutine_type(file, ret, &params, LLVMDIFlagZero);
        let subprogram = debug.builder.create_function(
            file,
            &f.name,
            Some(&function.get_name().to_string_lossy()),
            file,
            line,
            signature,
            function.get_linkage() == LLVMInternalLinkage,
            true,
            line,
            LLVMDIFlagPrototyped,
            debug.optimized,
        );
        function.set_subprogram(subprogram);
        debug.scopes = vec![subprogram];
        function.add_attribute(
            AttributeLoc::Function,
            self.context.create_string_attribute("frame-pointer", "all"),
        );
        Ok(())
    }
    pub(super) fn debug_variable(&mut self, name: &str, ty: &Type, ptr: Value<'ctx>) -> Result<()> {
        if self.debug.is_none() || name.starts_with('$') || name == "_" {
            return Ok(());
        }
        let dtype = self.debug_type(ty)?;
        let (file, line, column) = self.debug_file(self.span);
        let align = self.data.get_abi_alignment(&self.ty(ty)?) * 8;
        let debug = self.debug.as_mut().unwrap();
        // A new nested scope also models same-block shadowing: earlier bindings
        // remain visible while evaluating the new binding's initializer.
        if self.parameter == 0 {
            let scope = debug.builder.create_lexical_block(
                *debug.scopes.last().unwrap(),
                file,
                line,
                column,
            );
            *debug.scopes.last_mut().unwrap() = scope;
        }
        let scope = *debug.scopes.last().unwrap();
        let var = if self.parameter != 0 {
            debug.builder.create_parameter_variable(
                scope,
                name,
                self.parameter,
                file,
                line,
                dtype,
                true,
                LLVMDIFlagZero,
            )
        } else {
            debug.builder.create_auto_variable(
                scope,
                name,
                file,
                line,
                dtype,
                true,
                LLVMDIFlagZero,
                align,
            )
        };
        let location = debug
            .builder
            .create_debug_location(self.context, line, column, scope, None);
        debug
            .builder
            .insert_declare(ptr, var, location, self.builder.get_insert_block().unwrap());
        self.location(self.span);
        Ok(())
    }
    fn debug_type(&mut self, ty: &Type) -> Result<Metadata<'ctx>> {
        if let Some(dtype) = self.debug.as_ref().unwrap().types.get(ty) {
            return Ok(*dtype);
        }
        // Temporaries break cycles such as Node -> *Node. Replace every use
        // before finalization and remove the dangling handle from our cache.
        let placeholder = self
            .debug
            .as_ref()
            .unwrap()
            .builder
            .placeholder(self.context);
        self.debug
            .as_mut()
            .unwrap()
            .types
            .insert(ty.clone(), placeholder.metadata());
        let dtype = self.debug_type_inner(ty)?;
        placeholder.replace_with(dtype);
        self.debug.as_mut().unwrap().types.insert(ty.clone(), dtype);
        Ok(dtype)
    }
    fn debug_type_inner(&mut self, ty: &Type) -> Result<Metadata<'ctx>> {
        let llvm = self.storage_ty(ty)?;
        let size = self.data.get_abi_size(&llvm) * 8;
        let align = self.data.get_abi_alignment(&llvm) * 8;
        let name = ty.to_string();
        match ty {
            Type::Void | Type::Bool | Type::Int { .. } | Type::Float(_) => {
                let encoding = match ty {
                    Type::Bool => 0x02,
                    Type::Float(_) => 0x04,
                    Type::Int { signed: true, .. } => 0x05,
                    _ => 0x07,
                };
                Ok(self
                    .debug
                    .as_ref()
                    .unwrap()
                    .builder
                    .create_basic_type(&name, size, encoding, LLVMDIFlagZero)
                    .map_err(error)?)
            }
            Type::Ref(_, inner) | Type::Raw(_, inner) => {
                let inner = self.debug_type(inner)?;
                Ok(self
                    .debug
                    .as_ref()
                    .unwrap()
                    .builder
                    .create_pointer_type(&name, inner, size, align, 0))
            }
            Type::Array(_, inner) | Type::MaybeUninit(inner) => {
                let n = if let Type::Array(n, _) = ty { *n } else { 1 };
                let inner = self.debug_type(inner)?;
                let debug = self.debug.as_ref().unwrap();
                let array = debug.builder.create_array_type(
                    inner,
                    size,
                    align,
                    std::slice::from_ref(&(0..n as i64)),
                );
                let file = debug.files[0];
                Ok(debug
                    .builder
                    .create_typedef(array, &name, file, 0, file, align))
            }
            Type::Result(ok, err) => {
                let span = Span::default();
                let storage = llvm.get_field_type_at_index(1).unwrap();
                let mut alternatives = vec![];
                for (field, ty) in [("value", ok), ("error", err)] {
                    if **ty != Type::Void {
                        alternatives.push((field.into(), self.ty(ty)?, self.debug_type(ty)?, span));
                    }
                }
                let payload = self.debug_aggregate(
                    &format!("{name}.payload"),
                    span,
                    storage,
                    alternatives,
                    true,
                );
                let tag = self.debug_type(&Type::Bool)?;
                Ok(self.debug_struct(
                    &name,
                    span,
                    llvm,
                    vec![
                        ("is_error".into(), self.context.bool_type(), tag, span),
                        ("payload".into(), storage, payload, span),
                    ],
                ))
            }
            _ => {
                let (span, fields): (Span, Vec<(String, Type, Span)>) = match ty {
                    Type::Str => (
                        Span::default(),
                        vec![
                            (
                                "data".into(),
                                Type::Raw(false, Box::new(Type::u8())),
                                Span::default(),
                            ),
                            ("len".into(), Type::usize(), Span::default()),
                        ],
                    ),
                    Type::Slice(m, inner) => (
                        Span::default(),
                        vec![
                            ("data".into(), Type::Raw(*m, inner.clone()), Span::default()),
                            ("len".into(), Type::usize(), Span::default()),
                        ],
                    ),
                    Type::Option(inner) => (
                        Span::default(),
                        vec![
                            ("is_some".into(), Type::Bool, Span::default()),
                            ("value".into(), *inner.clone(), Span::default()),
                        ],
                    ),
                    Type::Named(name) => {
                        if let Some(s) = self.program.structs.iter().find(|s| s.name == *name) {
                            (
                                s.span,
                                s.fields
                                    .iter()
                                    .map(|f| (f.name.clone(), f.ty.clone(), f.span))
                                    .collect(),
                            )
                        } else {
                            return self.debug_enum(name, llvm);
                        }
                    }
                    _ => return Err(error(format!("cannot describe debug type {ty}"))),
                };
                let mut members = vec![];
                for (field, ty, span) in fields {
                    members.push((field, self.storage_ty(&ty)?, self.debug_type(&ty)?, span));
                }
                Ok(self.debug_struct(&name, span, llvm, members))
            }
        }
    }
    fn debug_struct(
        &self,
        name: &str,
        span: Span,
        llvm: LlvmType<'ctx>,
        fields: Vec<(String, LlvmType<'ctx>, Metadata<'ctx>, Span)>,
    ) -> Metadata<'ctx> {
        self.debug_aggregate(name, span, llvm, fields, false)
    }
    fn debug_aggregate(
        &self,
        name: &str,
        span: Span,
        llvm: LlvmType<'ctx>,
        fields: Vec<(String, LlvmType<'ctx>, Metadata<'ctx>, Span)>,
        union: bool,
    ) -> Metadata<'ctx> {
        let debug = self.debug.as_ref().unwrap();
        let (file, line, _) = self.debug_file(span);
        let scope = file;
        let members: Vec<_> = fields
            .into_iter()
            .enumerate()
            .map(|(i, (name, ty, dtype, span))| {
                let (file, line, _) = self.debug_file(span);
                debug.builder.create_member_type(
                    scope,
                    &name,
                    file,
                    line,
                    self.data.get_abi_size(&ty) * 8,
                    self.data.get_abi_alignment(&ty) * 8,
                    if union {
                        0
                    } else {
                        self.data.offset_of_element(&llvm, i as u32).unwrap() * 8
                    },
                    LLVMDIFlagPublic,
                    dtype,
                )
            })
            .collect();
        if union {
            return debug.builder.create_union_type(
                scope,
                name,
                file,
                line,
                self.data.get_abi_size(&llvm) * 8,
                self.data.get_abi_alignment(&llvm) * 8,
                LLVMDIFlagZero,
                &members,
                0,
                "",
            );
        }
        debug.builder.create_struct_type(
            scope,
            name,
            file,
            line,
            self.data.get_abi_size(&llvm) * 8,
            self.data.get_abi_alignment(&llvm) * 8,
            LLVMDIFlagZero,
            None,
            &members,
            0,
            None,
            "",
        )
    }
    fn debug_enum(&mut self, name: &str, llvm: LlvmType<'ctx>) -> Result<Metadata<'ctx>> {
        let decl = self
            .program
            .enums
            .iter()
            .find(|e| e.name == name)
            .ok_or_else(|| error("unknown debug enum"))?
            .clone();
        let tag_type = self.debug_type(&Type::Int {
            signed: false,
            bits: 32,
        })?;
        let (file, line, _) = self.debug_file(decl.span);
        let debug = self.debug.as_ref().unwrap();
        let variants: Vec<_> = decl
            .variants
            .iter()
            .enumerate()
            .map(|(i, v)| debug.builder.create_enumerator(&v.name, i as i64, true))
            .collect();
        let tag = debug.builder.create_enumeration_type(
            file,
            &format!("{name}.tag"),
            file,
            line,
            32,
            32,
            &variants,
            tag_type,
        );
        let mut fields = vec![("tag".into(), self.context.i32_type(), tag, decl.span)];
        let mut alternatives = vec![];
        for variant in &decl.variants {
            let layout = self.variant_ty(&variant.fields)?;
            let mut members = vec![];
            for f in &variant.fields {
                members.push((
                    f.name.clone(),
                    self.ty(&f.ty)?,
                    self.debug_type(&f.ty)?,
                    f.span,
                ));
            }
            let dtype = self.debug_struct(
                &format!("{name}.{}", variant.name),
                variant.span,
                layout,
                members,
            );
            alternatives.push((variant.name.clone(), layout, dtype, variant.span));
        }
        let storage = llvm.get_field_type_at_index(1).unwrap();
        let payload = self.debug_aggregate(
            &format!("{name}.payload"),
            decl.span,
            storage,
            alternatives,
            true,
        );
        fields.push(("payload".into(), storage, payload, decl.span));
        Ok(self.debug_struct(name, decl.span, llvm, fields))
    }
}
