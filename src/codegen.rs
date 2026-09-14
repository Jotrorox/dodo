//! Native code generation. Checked operations keep their checks at every optimization level.
use crate::ast::*;
use inkwell::attributes::{Attribute, AttributeLoc};
use inkwell::basic_block::BasicBlock;
use inkwell::builder::{Builder, BuilderError};
use inkwell::context::Context;
use inkwell::intrinsics::Intrinsic;
use inkwell::module::{Linkage, Module};
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, InitializationConfig, RelocMode, Target, TargetMachine, TargetTriple,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, StructType};
use inkwell::values::{
    BasicMetadataValueEnum, BasicValue, BasicValueEnum, FunctionValue, IntValue, PointerValue,
};
use inkwell::{
    AddressSpace, AtomicOrdering, AtomicRMWBinOp, FloatPredicate, IntPredicate, OptimizationLevel,
};
use std::collections::HashMap;

mod debug;
mod panic;
use debug::{DebugInfo, SourceMap};

#[derive(Clone, Debug, Default)]
pub enum PanicStrategy {
    /// Report to stderr on hosted targets; trap on freestanding targets.
    #[default]
    Auto,
    Hosted,
    /// Target-dependent LLVM trap; may lower to a C abort call.
    Trap,
    /// Non-returning C ABI:
    /// _Noreturn void hook(const char *check, const char *file, uint32_t line, uint32_t column).
    /// The hook owns termination; returning is undefined behavior.
    Hook(String),
}

#[derive(Debug)]
pub struct CodegenError(pub String);
impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for CodegenError {}
impl From<BuilderError> for CodegenError {
    fn from(e: BuilderError) -> Self {
        Self(e.to_string())
    }
}
impl From<inkwell::support::LLVMString> for CodegenError {
    fn from(e: inkwell::support::LLVMString) -> Self {
        Self(e.to_string())
    }
}
type Result<T> = std::result::Result<T, CodegenError>;
fn error(s: impl std::fmt::Display) -> CodegenError {
    CodegenError(s.to_string())
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub target: Option<String>,
    pub cpu: Option<String>,
    pub features: String,
    pub optimization: u8,
    pub entry: bool,
    /// Emit DWARF independently of the optimization level.
    pub debug: bool,
    /// Original files and package offsets, required for debug information and
    /// used for runtime failure locations even when debug information is off.
    pub sources: Vec<crate::package::Source>,
    pub panic: PanicStrategy,
    /// A hosted test executable dispatches one function per process invocation.
    pub test_functions: Vec<String>,
    /// Source maps are used only by the test failure reporter.
    pub test_sources: Vec<crate::package::Source>,
}
pub struct Generated<'ctx> {
    pub module: Module<'ctx>,
    pub machine: TargetMachine,
}
pub fn target_machine(options: &Options) -> Result<TargetMachine> {
    Target::initialize_all(&InitializationConfig::default());
    let triple = options
        .target
        .as_ref()
        .map_or_else(TargetMachine::get_default_triple, |s| {
            TargetTriple::create(s)
        });
    let target = Target::from_triple(&triple)?;
    let level = match options.optimization {
        0 => OptimizationLevel::None,
        1 => OptimizationLevel::Less,
        2 => OptimizationLevel::Default,
        _ => OptimizationLevel::Aggressive,
    };
    target
        .create_target_machine(
            &triple,
            options.cpu.as_deref().unwrap_or("generic"),
            &options.features,
            level,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| error("LLVM could not create the requested target machine"))
}
pub fn pointer_bits(options: &Options) -> Result<u32> {
    Ok(target_machine(options)?
        .get_target_data()
        .get_pointer_byte_size(None)
        * 8)
}
pub fn generate<'ctx>(
    context: &'ctx Context,
    program: &Program,
    options: &Options,
) -> Result<Generated<'ctx>> {
    let machine = target_machine(options)?;
    let module = context.create_module(&program.package);
    if let Some(source) = options.sources.first() {
        module.set_source_file_name(&source.path.to_string_lossy());
    }
    module.set_triple(&machine.get_triple());
    module.set_data_layout(&machine.get_target_data().get_data_layout());
    let bits = machine.get_target_data().get_pointer_byte_size(None) * 8;
    let sources = SourceMap::new(&options.sources);
    let debug = if options.debug {
        Some(DebugInfo::new(context, &module, options, &sources)?)
    } else {
        None
    };
    let mut cg = Codegen {
        context,
        module,
        builder: context.create_builder(),
        program,
        structs: HashMap::new(),
        functions: HashMap::new(),
        globals: HashMap::new(),
        scopes: vec![],
        loops: vec![],
        yields: vec![],
        function: None,
        return_type: Type::Void,
        bits,
        sources,
        debug,
        span: Span::default(),
        parameter: 0,
        data: machine.get_target_data(),
        panic: options.panic.clone(),
        test_sources: &options.test_sources,
        testing: !options.test_functions.is_empty(),
    };
    cg.declare_types()?;
    cg.declare_functions()?;
    cg.declare_constants()?;
    for f in &program.functions {
        if f.body.is_some() && f.generics.is_empty() {
            cg.function(f)?;
        }
    }
    if options.entry {
        cg.entry(&options.test_functions)?;
    }
    if let Some(debug) = &cg.debug {
        debug.builder.finalize();
    }
    cg.module
        .verify()
        .map_err(|e| error(format!("LLVM verification failed: {e}")))?;
    let passes = format!("default<O{}>", options.optimization.min(3));
    cg.module
        .run_passes(&passes, &machine, PassBuilderOptions::create())?;
    cg.module
        .verify()
        .map_err(|e| error(format!("optimized LLVM verification failed: {e}")))?;
    Ok(Generated {
        module: cg.module,
        machine,
    })
}

#[derive(Clone)]
struct Binding<'ctx> {
    name: String,
    ty: Type,
    ptr: PointerValue<'ctx>,
    live: Option<PointerValue<'ctx>>,
}
#[derive(Clone, Copy)]
struct Loop<'ctx> {
    end: BasicBlock<'ctx>,
    next: BasicBlock<'ctx>,
    depth: usize,
}
struct YieldTarget<'ctx> {
    ptr: PointerValue<'ctx>,
    ty: Type,
    end: BasicBlock<'ctx>,
    depth: usize,
}
struct Codegen<'a, 'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    program: &'a Program,
    structs: HashMap<String, StructType<'ctx>>,
    functions: HashMap<String, FunctionValue<'ctx>>,
    globals: HashMap<String, (PointerValue<'ctx>, Type)>,
    scopes: Vec<Vec<Binding<'ctx>>>,
    loops: Vec<Loop<'ctx>>,
    yields: Vec<YieldTarget<'ctx>>,
    function: Option<FunctionValue<'ctx>>,
    return_type: Type,
    bits: u32,
    sources: SourceMap<'a>,
    debug: Option<DebugInfo<'ctx>>,
    span: Span,
    parameter: u32,
    data: inkwell::targets::TargetData,
    panic: PanicStrategy,
    test_sources: &'a [crate::package::Source],
    testing: bool,
}
impl<'a, 'ctx> Codegen<'a, 'ctx> {
    fn ty(&self, t: &Type) -> Result<BasicTypeEnum<'ctx>> {
        Ok(match t {
            Type::Bool => self.context.bool_type().into(),
            Type::Int { bits, .. } => self
                .context
                .custom_width_int_type(
                    std::num::NonZeroU32::new(if *bits == 0 { self.bits } else { *bits }).unwrap(),
                )
                .unwrap()
                .into(),
            Type::Float(32) => self.context.f32_type().into(),
            Type::Float(64) => self.context.f64_type().into(),
            Type::Ref(..) | Type::Raw(..) => self.context.ptr_type(AddressSpace::default()).into(),
            Type::Slice(..) | Type::Str => self
                .context
                .struct_type(
                    &[
                        self.context.ptr_type(AddressSpace::default()).into(),
                        self.usize_type().into(),
                    ],
                    false,
                )
                .into(),
            // An opaque single-element array preserves T's ABI size/alignment without
            // exposing an initialized T to the language or invoking its destructor.
            Type::MaybeUninit(t) => self.ty(t)?.array_type(1).into(),
            Type::Array(n, t) => self
                .ty(t)?
                .array_type(u32::try_from(*n).map_err(|_| error("array is too large for LLVM"))?)
                .into(),
            Type::Named(n) => self
                .structs
                .get(n)
                .copied()
                .ok_or_else(|| error(format!("unresolved type {n}")))?
                .into(),
            Type::Result(t, e) => self
                .context
                .struct_type(
                    &[
                        self.context.bool_type().into(),
                        self.union_storage(&[self.payload_ty(t)?, self.payload_ty(e)?])?
                            .into(),
                    ],
                    false,
                )
                .into(),
            Type::Option(t) => self
                .context
                .struct_type(
                    &[self.context.bool_type().into(), self.storage_ty(t)?],
                    false,
                )
                .into(),
            _ => return Err(error(format!("cannot lower type {t}"))),
        })
    }
    fn storage_ty(&self, t: &Type) -> Result<BasicTypeEnum<'ctx>> {
        if *t == Type::Void {
            Ok(self.context.i8_type().into())
        } else {
            self.ty(t)
        }
    }
    fn payload_ty(&self, t: &Type) -> Result<BasicTypeEnum<'ctx>> {
        if *t == Type::Void {
            Ok(self.context.struct_type(&[], false).into())
        } else {
            self.ty(t)
        }
    }
    fn variant_ty(&self, fields: &[Field]) -> Result<StructType<'ctx>> {
        let fields = fields
            .iter()
            .map(|f| self.ty(&f.ty))
            .collect::<Result<Vec<_>>>()?;
        Ok(self.context.struct_type(&fields, false))
    }
    /// LLVM has no union type. A zero-length array supplies the strictest
    /// alignment, while bytes preserve every alternative's representation when
    /// the enclosing aggregate is copied or passed by value. Using a real
    /// alternative as storage would lose bytes in its padding or narrow fields.
    fn union_storage(&self, alternatives: &[BasicTypeEnum<'ctx>]) -> Result<StructType<'ctx>> {
        let mut aligner: BasicTypeEnum<'ctx> = self.context.i8_type().into();
        let mut size = 0;
        for ty in alternatives {
            if !ty.is_sized() {
                return Err(error("cannot lay out an unsized payload"));
            }
            size = size.max(self.data.get_abi_size(ty));
            if self.data.get_abi_alignment(ty) > self.data.get_abi_alignment(&aligner) {
                aligner = *ty;
            }
        }
        let alignment = u64::from(self.data.get_abi_alignment(&aligner));
        let size = size.div_ceil(alignment) * alignment;
        let bytes = self.context.i8_type().array_type(
            u32::try_from(size).map_err(|_| error("payload storage is too large for LLVM"))?,
        );
        Ok(self
            .context
            .struct_type(&[aligner.array_type(0).into(), bytes.into()], false))
    }
    fn usize_type(&self) -> inkwell::types::IntType<'ctx> {
        self.context
            .custom_width_int_type(std::num::NonZeroU32::new(self.bits).unwrap())
            .unwrap()
    }
    fn declare_types(&mut self) -> Result<()> {
        for s in &self.program.structs {
            if s.generics.is_empty() {
                self.structs
                    .insert(s.name.clone(), self.context.opaque_struct_type(&s.name));
            }
        }
        for e in &self.program.enums {
            if e.generics.is_empty() {
                self.structs
                    .insert(e.name.clone(), self.context.opaque_struct_type(&e.name));
            }
        }
        for name in self
            .program
            .structs
            .iter()
            .filter(|s| s.generics.is_empty())
            .map(|s| &s.name)
            .chain(
                self.program
                    .enums
                    .iter()
                    .filter(|e| e.generics.is_empty())
                    .map(|e| &e.name),
            )
        {
            self.define_layout(&Type::Named(name.clone()))?;
        }
        Ok(())
    }
    /// Resolve by-value dependencies before querying payload size/alignment.
    /// The checker rejects by-value cycles; pointer/reference cycles need no layout.
    fn define_layout(&self, ty: &Type) -> Result<()> {
        match ty {
            Type::Named(name) if self.structs[name].is_opaque() => {
                let fields = if let Some(s) = self.program.structs.iter().find(|s| s.name == *name)
                {
                    for f in &s.fields {
                        self.define_layout(&f.ty)?;
                    }
                    s.fields
                        .iter()
                        .map(|f| self.ty(&f.ty))
                        .collect::<Result<Vec<_>>>()?
                } else {
                    let e = self.program.enums.iter().find(|e| e.name == *name).unwrap();
                    let mut alternatives = Vec::new();
                    for v in &e.variants {
                        for f in &v.fields {
                            self.define_layout(&f.ty)?;
                        }
                        alternatives.push(self.variant_ty(&v.fields)?.into());
                    }
                    vec![
                        self.context.i32_type().into(),
                        self.union_storage(&alternatives)?.into(),
                    ]
                };
                self.structs[name].set_body(&fields, false);
            }
            Type::Array(_, t) | Type::MaybeUninit(t) | Type::Option(t) => self.define_layout(t)?,
            Type::Result(t, e) => {
                self.define_layout(t)?;
                self.define_layout(e)?;
            }
            _ => {}
        }
        Ok(())
    }
    fn tagged_value(
        &self,
        ty: StructType<'ctx>,
        tag: IntValue<'ctx>,
        payload: Option<BasicValueEnum<'ctx>>,
    ) -> Result<BasicValueEnum<'ctx>> {
        let value = self
            .builder
            .build_insert_value(ty.const_zero(), tag, 0, "tag")?;
        let ptr = self.alloca(ty.into(), "tagged.value")?;
        self.builder.build_store(ptr, value.into_struct_value())?;
        if let Some(payload) = payload {
            let p = self.builder.build_struct_gep(ty, ptr, 1, "payload")?;
            self.builder.build_store(p, payload)?;
        }
        Ok(self.builder.build_load(ty, ptr, "tagged")?)
    }
    fn declare_functions(&mut self) -> Result<()> {
        for f in &self.program.functions {
            if !f.generics.is_empty() {
                continue;
            }
            let params = f
                .params
                .iter()
                .map(|p| self.ty(&p.ty).map(BasicMetadataTypeEnum::from))
                .collect::<Result<Vec<_>>>()?;
            let ty = if f.ret == Type::Void {
                self.context.void_type().fn_type(&params, false)
            } else {
                self.ty(&f.ret)?.fn_type(&params, false)
            };
            let symbol = if f.extern_ {
                f.name.rsplit('.').next().unwrap_or(&f.name).to_owned()
            } else {
                format!("dodo.{}.{}", self.program.package, f.name)
            };
            let linkage = if f.extern_ || f.public || f.name == "main" {
                Linkage::External
            } else {
                Linkage::Internal
            };
            let function = if let Some(existing) = self.module.get_function(&symbol) {
                if !f.extern_ || existing.get_type() != ty {
                    return Err(error(format!(
                        "incompatible declarations of external symbol `{symbol}`"
                    )));
                }
                existing
            } else {
                self.module.add_function(&symbol, ty, Some(linkage))
            };
            if f.extern_ {
                for (location, attribute) in self.c_abi_attributes(f) {
                    function.add_attribute(location, attribute);
                }
            }
            self.functions.insert(f.name.clone(), function);
        }
        Ok(())
    }
    fn c_abi_attributes(&self, function: &Function) -> Vec<(AttributeLoc, Attribute)> {
        let triple = self.module.get_triple();
        let triple = triple.as_str().to_string_lossy();
        // SysV x86 C promotes narrow arguments/results in registers. LLVM needs
        // this promise on both declarations and calls (LangRef parameter attrs).
        let x86 = ["x86_64-", "i386-", "i486-", "i586-", "i686-"]
            .iter()
            .any(|arch| triple.starts_with(arch));
        if !x86 || triple.contains("windows") {
            return Vec::new();
        }
        std::iter::once((AttributeLoc::Return, &function.ret))
            .chain(
                function
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (AttributeLoc::Param(i as u32), &p.ty)),
            )
            .filter_map(|(location, ty)| {
                let name = match ty {
                    Type::Bool => "zeroext",
                    Type::Int {
                        signed,
                        bits: 8 | 16,
                    } => {
                        if *signed {
                            "signext"
                        } else {
                            "zeroext"
                        }
                    }
                    _ => return None,
                };
                Some((
                    location,
                    self.context
                        .create_enum_attribute(Attribute::get_named_enum_kind_id(name), 0),
                ))
            })
            .collect()
    }
    fn declare_constants(&mut self) -> Result<()> {
        for c in &self.program.constants {
            let val = self.constant(&c.value)?;
            let g = self.module.add_global(
                self.ty(&c.ty)?,
                None,
                &format!("dodo.{}.{}", self.program.package, c.name),
            );
            g.set_initializer(&val);
            g.set_constant(!c.mutable);
            g.set_linkage(if c.public {
                Linkage::External
            } else {
                Linkage::Internal
            });
            self.globals
                .insert(c.name.clone(), (g.as_pointer_value(), c.ty.clone()));
        }
        Ok(())
    }
    fn string_literal(&self, bytes: &[u8]) -> BasicValueEnum<'ctx> {
        let data = self.context.const_string(bytes, false);
        let g = self.module.add_global(data.get_type(), None, "string");
        g.set_initializer(&data);
        g.set_constant(true);
        g.set_linkage(Linkage::Private);
        g.set_unnamed_addr(true);
        self.context
            .struct_type(
                &[
                    self.context.ptr_type(AddressSpace::default()).into(),
                    self.usize_type().into(),
                ],
                false,
            )
            .const_named_struct(&[
                g.as_pointer_value().into(),
                self.usize_type()
                    .const_int(bytes.len() as u64, false)
                    .into(),
            ])
            .into()
    }
    fn constant(&self, e: &Expr) -> Result<BasicValueEnum<'ctx>> {
        match &e.kind {
            ExprKind::String(bytes, _) => Ok(self.string_literal(bytes)),
            ExprKind::Name(n) => {
                let c = self
                    .program
                    .constants
                    .iter()
                    .find(|c| c.name == *n)
                    .ok_or_else(|| error("unknown constant"))?;
                self.constant(&c.value)
            }
            ExprKind::Constant(value, _) => self.constant(value),
            ExprKind::Repeat(value, _) => {
                let Type::Array(n, t) = &e.ty else {
                    return Err(error("invalid repeated array constant"));
                };
                let value = self.constant(value)?;
                if match value {
                    BasicValueEnum::IntValue(v) => v.is_null(),
                    BasicValueEnum::FloatValue(v) => v.is_null(),
                    BasicValueEnum::PointerValue(v) => v.is_null(),
                    _ => false,
                } {
                    return Ok(self.ty(&e.ty)?.const_zero());
                }
                if *n > 1_000_000 {
                    return Err(error(
                        "nonzero repeated constant array exceeds the supported size of 1000000 elements",
                    ));
                }
                self.const_array(t, &vec![value; *n])
            }
            ExprKind::Array(_, xs) => {
                let Type::Array(_, t) = &e.ty else {
                    return Err(error("invalid array constant"));
                };
                let vals = xs
                    .iter()
                    .map(|x| self.constant(x))
                    .collect::<Result<Vec<_>>>()?;
                self.const_array(t, &vals)
            }
            ExprKind::Struct(n, fields) => {
                let decl = self
                    .program
                    .structs
                    .iter()
                    .find(|s| s.name == *n)
                    .ok_or_else(|| error("unknown constant struct"))?;
                let values = decl
                    .fields
                    .iter()
                    .map(|f| {
                        let (_, e) = fields
                            .iter()
                            .find(|(n, _)| *n == f.name)
                            .ok_or_else(|| error("missing constant field"))?;
                        self.constant(e)
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(self
                    .ty(&e.ty)?
                    .into_struct_type()
                    .const_named_struct(&values)
                    .into())
            }
            _ => {
                use crate::consteval::Scalar;
                Ok(match crate::consteval::eval(e, self.bits).map_err(error)? {
                    Scalar::Int(v) => self
                        .ty(&e.ty)?
                        .into_int_type()
                        .const_int(v as u64, false)
                        .into(),
                    Scalar::Float(v) => self.ty(&e.ty)?.into_float_type().const_float(v).into(),
                    Scalar::Bool(v) => self.context.bool_type().const_int(v as u64, false).into(),
                })
            }
        }
    }
    fn const_array(&self, t: &Type, vs: &[BasicValueEnum<'ctx>]) -> Result<BasicValueEnum<'ctx>> {
        use inkwell::types::BasicTypeEnum::*;
        Ok(match self.ty(t)? {
            IntType(t) => t
                .const_array(&vs.iter().map(|v| v.into_int_value()).collect::<Vec<_>>())
                .into(),
            FloatType(t) => t
                .const_array(&vs.iter().map(|v| v.into_float_value()).collect::<Vec<_>>())
                .into(),
            StructType(t) => t
                .const_array(&vs.iter().map(|v| v.into_struct_value()).collect::<Vec<_>>())
                .into(),
            ArrayType(t) => t
                .const_array(&vs.iter().map(|v| v.into_array_value()).collect::<Vec<_>>())
                .into(),
            PointerType(t) => t
                .const_array(
                    &vs.iter()
                        .map(|v| v.into_pointer_value())
                        .collect::<Vec<_>>(),
                )
                .into(),
            _ => return Err(error("unsupported constant array element")),
        })
    }
    fn function(&mut self, f: &Function) -> Result<()> {
        let function = self.functions[&f.name];
        self.function = Some(function);
        self.return_type = f.ret.clone();
        self.scopes = vec![vec![]];
        self.loops.clear();
        self.debug_function(f, function)?;
        self.location(f.span);
        self.builder
            .position_at_end(self.context.append_basic_block(function, "entry"));
        for (index, (p, v)) in f.params.iter().zip(function.get_param_iter()).enumerate() {
            self.parameter = index as u32 + 1;
            self.location(p.span);
            self.bind(&p.name, &p.ty, Some(v))?;
        }
        self.parameter = 0;
        self.location(f.span);
        self.block(f.body.as_ref().unwrap())?;
        if !self.terminated() {
            self.cleanup_to(0)?;
            if f.ret == Type::Void {
                self.builder.build_return(None)?;
            } else {
                self.builder.build_unreachable()?;
            }
        }
        self.scopes.clear();
        Ok(())
    }
    fn entry(&mut self, tests: &[String]) -> Result<()> {
        self.builder.unset_current_debug_location();
        if tests.is_empty() {
            let f = self
                .program
                .functions
                .iter()
                .find(|f| f.name == "main" && !f.extern_)
                .ok_or_else(|| error("executable requires fn main() -> i32 or -> void"))?;
            if !f.params.is_empty()
                || !matches!(
                    f.ret,
                    Type::Void
                        | Type::Int {
                            signed: true,
                            bits: 32
                        }
                )
            {
                return Err(error(
                    "main must have signature fn main() -> i32 or fn main() -> void",
                ));
            }
        }
        if self.module.get_function("main").is_some() {
            return Err(error("extern main conflicts with the hosted entry point"));
        }
        let uses_environment = self
            .program
            .imports
            .iter()
            .any(|path| path == "std/env" || path.starts_with("std/env/"));
        let arguments_init = uses_environment
            .then(|| {
                self.program.functions.iter().find_map(|function| {
                    (function.extern_
                        && function.name.rsplit('.').next() == Some("dodo_env_init_args"))
                    .then(|| self.functions[&function.name])
                })
            })
            .flatten();
        let entry_params = if arguments_init.is_some() || !tests.is_empty() {
            vec![
                self.context.i32_type().into(),
                self.context.ptr_type(AddressSpace::default()).into(),
            ]
        } else {
            vec![]
        };
        let entry = self.module.add_function(
            "main",
            self.context.i32_type().fn_type(&entry_params, false),
            None,
        );
        self.builder
            .position_at_end(self.context.append_basic_block(entry, "entry"));
        if let Some(initialize) = arguments_init {
            self.builder.build_call(
                initialize,
                &[
                    entry.get_nth_param(0).unwrap().into(),
                    entry.get_nth_param(1).unwrap().into(),
                ],
                "",
            )?;
        }
        if !tests.is_empty() {
            let select = self.module.add_function(
                "dodo_test_select",
                self.context.i32_type().fn_type(&entry_params, false),
                None,
            );
            let index = self
                .builder
                .build_call(
                    select,
                    &[
                        entry.get_nth_param(0).unwrap().into(),
                        entry.get_nth_param(1).unwrap().into(),
                    ],
                    "test.index",
                )?
                .try_as_basic_value()
                .basic()
                .unwrap()
                .into_int_value();
            let invalid = self.context.append_basic_block(entry, "invalid.test");
            let cases: Vec<_> = tests
                .iter()
                .enumerate()
                .map(|(i, _)| {
                    (
                        self.context.i32_type().const_int(i as u64, false),
                        self.context.append_basic_block(entry, "test"),
                    )
                })
                .collect();
            self.builder.build_switch(index, invalid, &cases)?;
            self.builder.position_at_end(invalid);
            self.builder
                .build_return(Some(&self.context.i32_type().const_int(2, false)))?;
            for (name, (_, block)) in tests.iter().zip(cases) {
                self.builder.position_at_end(block);
                let call = self
                    .builder
                    .build_call(self.functions[name], &[], "test.result")?;
                let value = call
                    .try_as_basic_value()
                    .basic()
                    .unwrap_or(self.context.i32_type().const_zero().into());
                self.builder.build_return(Some(&value))?;
            }
            return Ok(());
        }
        let call = self
            .builder
            .build_call(self.functions["main"], &[], "dodo.main")?;
        let value = call
            .try_as_basic_value()
            .basic()
            .unwrap_or(self.context.i32_type().const_zero().into());
        self.builder.build_return(Some(&value))?;
        Ok(())
    }
    fn terminated(&self) -> bool {
        self.builder
            .get_insert_block()
            .is_some_and(|b| b.get_terminator().is_some())
    }
    fn bb(&self, name: &str) -> BasicBlock<'ctx> {
        self.context
            .append_basic_block(self.function.unwrap(), name)
    }
    fn alloca(&self, t: BasicTypeEnum<'ctx>, name: &str) -> Result<PointerValue<'ctx>> {
        let b = self.context.create_builder();
        let entry = self.function.unwrap().get_first_basic_block().unwrap();
        if let Some(i) = entry.get_first_instruction() {
            b.position_before(&i)
        } else {
            b.position_at_end(entry)
        };
        Ok(b.build_alloca(t, name)?)
    }
    fn bind(
        &mut self,
        name: &str,
        ty: &Type,
        value: Option<BasicValueEnum<'ctx>>,
    ) -> Result<Binding<'ctx>> {
        let ptr = self.alloca(self.ty(ty)?, name)?;
        if let Some(v) = value {
            self.builder.build_store(ptr, v)?;
        }
        let live = if self.needs_drop(ty) {
            let flag = self.alloca(self.context.bool_type().into(), "initialized")?;
            // A temporary may be created only in a short-circuit branch. Its
            // cleanup flag must still be defined along the skipped path.
            let entry_builder = self.context.create_builder();
            let allocation = flag.as_instruction_value().unwrap();
            if let Some(next) = allocation.get_next_instruction() {
                entry_builder.position_before(&next);
            } else {
                entry_builder
                    .position_at_end(self.function.unwrap().get_first_basic_block().unwrap());
            }
            entry_builder.build_store(flag, self.context.bool_type().const_zero())?;
            self.builder.build_store(
                flag,
                self.context
                    .bool_type()
                    .const_int(value.is_some() as u64, false),
            )?;
            Some(flag)
        } else {
            None
        };
        let binding = Binding {
            name: name.into(),
            ty: ty.clone(),
            ptr,
            live,
        };
        self.scopes.last_mut().unwrap().push(binding.clone());
        self.debug_variable(name, ty, ptr)?;
        Ok(binding)
    }
    fn binding(&self, name: &str) -> Option<Binding<'ctx>> {
        self.scopes
            .iter()
            .rev()
            .flat_map(|s| s.iter().rev())
            .find(|b| b.name == name)
            .cloned()
    }
    fn load(&self, ptr: PointerValue<'ctx>, t: &Type) -> Result<BasicValueEnum<'ctx>> {
        Ok(self.builder.build_load(self.ty(t)?, ptr, "value")?)
    }
    fn consume(&self, e: &Expr) -> Result<()> {
        if !e.ty.is_copy()
            && let ExprKind::Name(n) = &e.kind
            && let Some(b) = self.binding(n)
            && let Some(p) = b.live
        {
            self.builder
                .build_store(p, self.context.bool_type().const_zero())?;
        }
        Ok(())
    }
    fn cleanup_to(&mut self, depth: usize) -> Result<()> {
        let bindings = self.scopes[depth..]
            .iter()
            .rev()
            .flat_map(|s| s.iter().rev().cloned())
            .collect::<Vec<_>>();
        for b in bindings {
            self.drop_binding(&b)?;
        }
        Ok(())
    }
    fn drop_binding(&mut self, b: &Binding<'ctx>) -> Result<()> {
        if let Some(flag) = b.live {
            let live = self
                .builder
                .build_load(self.context.bool_type(), flag, "live")?
                .into_int_value();
            let yes = self.bb("drop");
            let done = self.bb("drop.done");
            self.builder.build_conditional_branch(live, yes, done)?;
            self.builder.position_at_end(yes);
            self.drop_ptr(b.ptr, &b.ty)?;
            self.builder
                .build_store(flag, self.context.bool_type().const_zero())?;
            self.builder.build_unconditional_branch(done)?;
            self.builder.position_at_end(done);
        }
        Ok(())
    }
    fn needs_drop(&self, ty: &Type) -> bool {
        match ty {
            Type::Named(n) => {
                self.functions.contains_key(&format!("{n}.drop"))
                    || self
                        .program
                        .structs
                        .iter()
                        .find(|s| s.name == *n)
                        .is_some_and(|s| s.fields.iter().any(|f| self.needs_drop(&f.ty)))
                    || self
                        .program
                        .enums
                        .iter()
                        .find(|e| e.name == *n)
                        .is_some_and(|e| {
                            e.variants
                                .iter()
                                .any(|v| v.fields.iter().any(|f| self.needs_drop(&f.ty)))
                        })
            }
            Type::Array(_, t) | Type::Option(t) => self.needs_drop(t),
            Type::Result(t, e) => self.needs_drop(t) || self.needs_drop(e),
            _ => false,
        }
    }
    fn drop_ptr(&mut self, ptr: PointerValue<'ctx>, ty: &Type) -> Result<()> {
        if !self.needs_drop(ty) {
            return Ok(());
        }
        match ty {
            Type::Named(n) => {
                if let Some(s) = self.program.structs.iter().find(|s| s.name == *n).cloned() {
                    if let Some(f) = self.functions.get(&format!("{n}.drop")).copied() {
                        self.builder.build_call(f, &[ptr.into()], "")?;
                    }
                    for (i, field) in s.fields.iter().enumerate().rev() {
                        let p = self.builder.build_struct_gep(
                            self.ty(ty)?.into_struct_type(),
                            ptr,
                            i as u32,
                            "drop.field",
                        )?;
                        self.drop_ptr(p, &field.ty)?;
                    }
                } else if let Some(e) = self.program.enums.iter().find(|e| e.name == *n).cloned() {
                    let st = self.ty(ty)?.into_struct_type();
                    let tagptr = self.builder.build_struct_gep(st, ptr, 0, "tag")?;
                    let tag = self
                        .builder
                        .build_load(self.context.i32_type(), tagptr, "tag")?
                        .into_int_value();
                    let end = self.bb("drop.enum.end");
                    let cases = e
                        .variants
                        .iter()
                        .enumerate()
                        .map(|(i, _)| {
                            (
                                self.context.i32_type().const_int(i as u64, false),
                                self.bb("drop.variant"),
                            )
                        })
                        .collect::<Vec<_>>();
                    self.builder.build_switch(tag, end, &cases)?;
                    for ((_, block), v) in cases.iter().zip(&e.variants) {
                        self.builder.position_at_end(*block);
                        let p = self.builder.build_struct_gep(st, ptr, 1, "payload")?;
                        let pt = self.variant_ty(&v.fields)?;
                        for (j, f) in v.fields.iter().enumerate().rev() {
                            let fp = self.builder.build_struct_gep(pt, p, j as u32, "field")?;
                            self.drop_ptr(fp, &f.ty)?;
                        }
                        self.builder.build_unconditional_branch(end)?;
                    }
                    self.builder.position_at_end(end);
                }
            }
            Type::Array(n, t) => {
                for i in (0..*n).rev() {
                    let p =
                        self.array_gep(ptr, ty, self.usize_type().const_int(i as u64, false))?;
                    self.drop_ptr(p, t)?;
                }
            }
            Type::Result(t, e) => {
                let st = self.ty(ty)?.into_struct_type();
                let tagptr = self.builder.build_struct_gep(st, ptr, 0, "tag")?;
                let tag = self
                    .builder
                    .build_load(self.context.bool_type(), tagptr, "tag")?
                    .into_int_value();
                let ok = self.bb("drop.ok");
                let err = self.bb("drop.err");
                let end = self.bb("drop.result.end");
                self.builder.build_conditional_branch(tag, err, ok)?;
                for (bb, t) in [(ok, t), (err, e)] {
                    self.builder.position_at_end(bb);
                    if **t != Type::Void {
                        let p = self.builder.build_struct_gep(st, ptr, 1, "payload")?;
                        self.drop_ptr(p, t)?;
                    }
                    self.builder.build_unconditional_branch(end)?;
                }
                self.builder.position_at_end(end);
            }
            Type::Option(t) => {
                let st = self.ty(ty)?.into_struct_type();
                let tp = self.builder.build_struct_gep(st, ptr, 0, "tag")?;
                let tag = self
                    .builder
                    .build_load(self.context.bool_type(), tp, "tag")?
                    .into_int_value();
                let some = self.bb("drop.some");
                let end = self.bb("drop.option.end");
                self.builder.build_conditional_branch(tag, some, end)?;
                self.builder.position_at_end(some);
                let p = self.builder.build_struct_gep(st, ptr, 1, "payload")?;
                self.drop_ptr(p, t)?;
                self.builder.build_unconditional_branch(end)?;
                self.builder.position_at_end(end);
            }
            _ => {}
        }
        Ok(())
    }
    fn block(&mut self, block: &Block) -> Result<()> {
        self.push_scope();
        for s in block {
            if self.terminated() {
                break;
            }
            self.stmt(s)?;
        }
        if !self.terminated() {
            self.cleanup_to(self.scopes.len() - 1)?;
        }
        self.pop_scope();
        Ok(())
    }
    fn stmt(&mut self, s: &Stmt) -> Result<()> {
        let previous = self.span;
        self.location(s.span);
        let result = self.stmt_inner(s);
        self.location(previous);
        result
    }
    fn stmt_inner(&mut self, s: &Stmt) -> Result<()> {
        match &s.kind {
            StmtKind::Let {
                name, ty, value, ..
            } => {
                if let Some(initializer) = value
                    && let ExprKind::Repeat(element, _) = &initializer.kind
                {
                    // Evaluate before introducing the new name: a shadowing
                    // initializer still refers to the previous binding. This
                    // also preserves evaluation and propagation for length 0.
                    let element = self.expr(element)?;
                    let binding = self.bind(name, ty, None)?;
                    self.initialize_repeat(binding.ptr, ty, element)?;
                    if let Some(flag) = binding.live {
                        self.builder
                            .build_store(flag, self.context.bool_type().const_int(1, false))?;
                    }
                } else {
                    let v = value.as_ref().map(|v| self.expr(v)).transpose()?;
                    self.bind(name, ty, v)?;
                }
            }
            StmtKind::Assign { target, op, value } => {
                if matches!(&target.kind,ExprKind::Name(n) if n=="_") {
                    let v = self.expr(value)?;
                    if !value.ty.is_copy() {
                        let p = self.alloca(self.ty(&value.ty)?, "discard")?;
                        self.builder.build_store(p, v)?;
                        self.drop_ptr(p, &value.ty)?;
                    }
                    return Ok(());
                }
                let ptr = self.place(target)?;
                let old = if op.is_some() {
                    Some(self.load(ptr, &target.ty)?)
                } else {
                    None
                };
                let rhs = self.expr(value)?;
                let v = if let Some(op) = op {
                    self.binary(*op, old.unwrap(), rhs, &target.ty)?
                } else {
                    rhs
                };
                if op.is_none() && !target.ty.is_copy() {
                    if let ExprKind::Name(n) = &target.kind {
                        if let Some(b) = self.binding(n) {
                            self.drop_binding(&b)?;
                        }
                    } else {
                        self.drop_ptr(ptr, &target.ty)?;
                    }
                }
                self.builder.build_store(ptr, v)?;
                if let ExprKind::Name(n) = &target.kind
                    && let Some(b) = self.binding(n)
                    && let Some(f) = b.live
                {
                    self.builder
                        .build_store(f, self.context.bool_type().const_int(1, false))?;
                }
            }
            StmtKind::Expr(e) => {
                let v = self.expr(e)?;
                if !e.ty.is_copy() {
                    let p = self.alloca(self.ty(&e.ty)?, "temporary")?;
                    self.builder.build_store(p, v)?;
                    self.drop_ptr(p, &e.ty)?;
                }
            }
            StmtKind::Yield(e) => {
                let value = self.expr(e)?;
                let target = self
                    .yields
                    .last()
                    .ok_or_else(|| error("value exit outside a value block"))?;
                let (ptr, end, depth) = (target.ptr, target.end, target.depth);
                if target.ty != Type::Void {
                    self.builder.build_store(ptr, value)?;
                }
                self.cleanup_to(depth)?;
                self.builder.build_unconditional_branch(end)?;
            }
            StmtKind::Return(e) => {
                let v = e.as_ref().map(|v| self.expr(v)).transpose()?;
                self.cleanup_to(0)?;
                if self.return_type == Type::Void {
                    self.builder.build_return(None)?;
                } else if let Some(v) = v {
                    self.builder.build_return(Some(&v))?;
                } else {
                    return Err(error("non-void return is missing its value"));
                }
            }
            StmtKind::Block(b) | StmtKind::Unsafe(b) => self.block(b)?,
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                let c = self.expr(condition)?.into_int_value();
                let yes = self.bb("if.then");
                let no = self.bb("if.else");
                let end = self.bb("if.end");
                self.builder.build_conditional_branch(c, yes, no)?;
                self.builder.position_at_end(yes);
                self.block(then_block)?;
                let yes_ends = self.terminated();
                if !yes_ends {
                    self.builder.build_unconditional_branch(end)?;
                }
                self.builder.position_at_end(no);
                self.block(else_block)?;
                let no_ends = self.terminated();
                if !no_ends {
                    self.builder.build_unconditional_branch(end)?;
                }
                self.builder.position_at_end(end);
                if yes_ends && no_ends {
                    self.builder.build_unreachable()?;
                }
            }
            StmtKind::IfLet {
                pattern,
                value,
                then_block,
                else_block,
            } => {
                self.if_let_stmt(pattern, value, then_block, else_block)?;
            }
            StmtKind::LetPattern {
                pattern,
                value,
                else_block,
                ..
            } => {
                self.let_pattern_stmt(pattern, value, else_block.as_ref())?;
            }
            StmtKind::For {
                init,
                condition,
                step,
                body,
            } => {
                self.push_scope();
                if let Some(s) = init {
                    self.stmt(s)?;
                }
                let head = self.bb("for.condition");
                let body_bb = self.bb("for.body");
                let step_bb = self.bb("for.step");
                let end = self.bb("for.end");
                self.builder.build_unconditional_branch(head)?;
                self.builder.position_at_end(head);
                if let Some(c) = condition {
                    let v = self.expr(c)?.into_int_value();
                    self.builder.build_conditional_branch(v, body_bb, end)?;
                } else {
                    self.builder.build_unconditional_branch(body_bb)?;
                }
                self.loops.push(Loop {
                    end,
                    next: step_bb,
                    depth: self.scopes.len(),
                });
                self.builder.position_at_end(body_bb);
                self.block(body)?;
                if !self.terminated() {
                    self.builder.build_unconditional_branch(step_bb)?;
                }
                self.builder.position_at_end(step_bb);
                if let Some(s) = step {
                    self.stmt(s)?;
                }
                if !self.terminated() {
                    self.builder.build_unconditional_branch(head)?;
                }
                self.loops.pop();
                self.builder.position_at_end(end);
                self.cleanup_to(self.scopes.len() - 1)?;
                self.pop_scope();
            }
            StmtKind::ForEach {
                index,
                name,
                copy,
                iterable,
                body,
            } => self.foreach(index.as_deref(), name, *copy, iterable, body)?,
            StmtKind::Break | StmtKind::Continue => {
                let l = *self
                    .loops
                    .last()
                    .ok_or_else(|| error("loop control outside loop"))?;
                self.cleanup_to(l.depth)?;
                self.builder
                    .build_unconditional_branch(if matches!(s.kind, StmtKind::Break) {
                        l.end
                    } else {
                        l.next
                    })?;
            }
            StmtKind::Match { value, arms } => self.match_stmt(value, arms)?,
        }
        Ok(())
    }
    fn stage(
        &mut self,
        expression: &Expr,
        pending: &mut Vec<PointerValue<'ctx>>,
    ) -> Result<BasicValueEnum<'ctx>> {
        let value = self.expr(expression)?;
        if self.needs_drop(&expression.ty) {
            let binding = self.bind("$pending", &expression.ty, Some(value))?;
            if let Some(flag) = binding.live {
                pending.push(flag);
            }
        }
        Ok(value)
    }
    fn transfer(&self, pending: &[PointerValue<'ctx>]) -> Result<()> {
        for flag in pending {
            self.builder
                .build_store(*flag, self.context.bool_type().const_zero())?;
        }
        Ok(())
    }
    // Fill existing array storage without materializing the complete array as
    // an SSA aggregate. In particular, large caller-owned byte buffers must not
    // become hundreds of thousands of scalar loads/stores during lowering.
    // The checked language requires a copyable element and evaluates it once.
    fn initialize_repeat(
        &mut self,
        storage: PointerValue<'ctx>,
        ty: &Type,
        value: BasicValueEnum<'ctx>,
    ) -> Result<()> {
        let Type::Array(n, element) = ty else {
            return Err(error("invalid repeated array"));
        };
        let counter = self.alloca(self.usize_type().into(), "repeat.index")?;
        self.builder
            .build_store(counter, self.usize_type().const_zero())?;
        let head = self.bb("repeat.condition");
        let body = self.bb("repeat.body");
        let end = self.bb("repeat.end");
        self.builder.build_unconditional_branch(head)?;
        self.builder.position_at_end(head);
        let index = self
            .builder
            .build_load(self.usize_type(), counter, "repeat.index")?
            .into_int_value();
        let more = self.builder.build_int_compare(
            IntPredicate::ULT,
            index,
            self.usize_type().const_int(*n as u64, false),
            "repeat.more",
        )?;
        self.builder.build_conditional_branch(more, body, end)?;
        self.builder.position_at_end(body);
        let ptr = unsafe {
            self.builder
                .build_gep(self.ty(element)?, storage, &[index], "repeat.element")?
        };
        self.builder.build_store(ptr, value)?;
        let next = self.builder.build_int_add(
            index,
            self.usize_type().const_int(1, false),
            "repeat.next",
        )?;
        self.builder.build_store(counter, next)?;
        self.builder.build_unconditional_branch(head)?;
        self.builder.position_at_end(end);
        Ok(())
    }
    fn expr(&mut self, e: &Expr) -> Result<BasicValueEnum<'ctx>> {
        let previous = self.span;
        self.location(e.span);
        let result = self.expr_inner(e);
        self.location(previous);
        result
    }
    fn expr_inner(&mut self, e: &Expr) -> Result<BasicValueEnum<'ctx>> {
        let value = match &e.kind {
            ExprKind::Int(v, _) => self.ty(&e.ty)?.into_int_type().const_int(*v, false).into(),
            ExprKind::Float(v, _) => self.ty(&e.ty)?.into_float_type().const_float(*v).into(),
            ExprKind::Bool(v) => self.context.bool_type().const_int(*v as u64, false).into(),
            ExprKind::String(bytes, _) => self.string_literal(bytes),
            ExprKind::Name(n) => {
                if n == "none" {
                    self.ty(&e.ty)?.const_zero()
                } else if let Some(b) = self.binding(n) {
                    self.load(b.ptr, &b.ty)?
                } else if let Some((p, t)) = self.globals.get(n).cloned() {
                    self.load(p, &t)?
                } else {
                    return Err(error(format!("unknown value {n}")));
                }
            }
            ExprKind::Constant(value, _) => self.expr(value)?,
            ExprKind::Repeat(value, _) => {
                let value = self.expr(value)?;
                let storage = self.alloca(self.ty(&e.ty)?, "repeat.array")?;
                self.initialize_repeat(storage, &e.ty, value)?;
                self.load(storage, &e.ty)?
            }
            ExprKind::ValueBlock(body) => {
                let ptr = self.alloca(self.storage_ty(&e.ty)?, "block.value")?;
                let end = self.bb("block.end");
                self.yields.push(YieldTarget {
                    ptr,
                    ty: e.ty.clone(),
                    end,
                    depth: self.scopes.len(),
                });
                self.block(body)?;
                self.yields.pop();
                self.builder.position_at_end(end);
                if e.ty == Type::Void {
                    self.context.i8_type().const_zero().into()
                } else {
                    self.load(ptr, &e.ty)?
                }
            }
            ExprKind::Slice {
                base, start, end, ..
            } => {
                let (ptr, length, element) = self.collection(base)?;
                let start = if let Some(e) = start {
                    let v = self.expr(e)?;
                    self.cast(v, &e.ty, &Type::usize())?.into_int_value()
                } else {
                    self.usize_type().const_zero()
                };
                let end = if let Some(e) = end {
                    let v = self.expr(e)?;
                    self.cast(v, &e.ty, &Type::usize())?.into_int_value()
                } else {
                    length
                };
                let ordered = self.builder.build_int_compare(
                    IntPredicate::ULE,
                    start,
                    end,
                    "slice.ordered",
                )?;
                let inside = self.builder.build_int_compare(
                    IntPredicate::ULE,
                    end,
                    length,
                    "slice.inside",
                )?;
                self.guard(
                    self.builder.build_and(ordered, inside, "slice.valid")?,
                    "slice bounds",
                    "slice bounds out of range",
                )?;
                let data = unsafe {
                    self.builder
                        .build_gep(self.ty(&element)?, ptr, &[start], "slice.data")?
                };
                let length = self.builder.build_int_sub(end, start, "slice.length")?;
                let slice = self.ty(&e.ty)?.into_struct_type().const_zero();
                let slice = self
                    .builder
                    .build_insert_value(slice, data, 0, "slice.ptr")?
                    .into_struct_value();
                self.builder
                    .build_insert_value(slice, length, 1, "slice.len")?
                    .into_struct_value()
                    .into()
            }
            ExprKind::Range(..) => return Err(error("unresolved range reached code generation")),
            ExprKind::Array(_, xs) => {
                let mut array = self.ty(&e.ty)?.into_array_type().const_zero();
                let mut pending = Vec::new();
                for (i, x) in xs.iter().enumerate() {
                    let v = self.stage(x, &mut pending)?;
                    array = self
                        .builder
                        .build_insert_value(array, v, i as u32, "array")?
                        .into_array_value();
                }
                self.transfer(&pending)?;
                array.into()
            }
            ExprKind::Struct(n, fields) => {
                let st = self
                    .structs
                    .get(n)
                    .copied()
                    .ok_or_else(|| error(format!("unknown struct {n}")))?;
                let decl = self
                    .program
                    .structs
                    .iter()
                    .find(|s| s.name == *n)
                    .unwrap()
                    .clone();
                let mut v = st.const_zero();
                let mut pending = Vec::new();
                for (name, e) in fields {
                    let i = decl
                        .fields
                        .iter()
                        .position(|f| f.name == *name)
                        .ok_or_else(|| error("unknown field"))?;
                    let x = self.stage(e, &mut pending)?;
                    v = self
                        .builder
                        .build_insert_value(v, x, i as u32, "struct")?
                        .into_struct_value();
                }
                self.transfer(&pending)?;
                v.into()
            }
            ExprKind::Unary(op, x) => match op {
                UnaryOp::Borrow | UnaryOp::BorrowMut => {
                    let ptr = self.place(x)?;
                    if let Type::Slice(_, _) = &e.ty {
                        match &x.ty {
                            Type::Array(n, _) => {
                                let mut v = self.ty(&e.ty)?.into_struct_type().const_zero();
                                v = self
                                    .builder
                                    .build_insert_value(v, ptr, 0, "slice.ptr")?
                                    .into_struct_value();
                                self.builder
                                    .build_insert_value(
                                        v,
                                        self.usize_type().const_int(*n as u64, false),
                                        1,
                                        "slice.len",
                                    )?
                                    .into_struct_value()
                                    .into()
                            }
                            _ => self.load(ptr, &e.ty)?,
                        }
                    } else {
                        ptr.into()
                    }
                }
                UnaryOp::Deref => {
                    let p = self.expr(x)?.into_pointer_value();
                    self.load(p, &e.ty)?
                }
                UnaryOp::Neg => {
                    if let ExprKind::Int(v, _) = x.kind {
                        self.ty(&e.ty)?
                            .into_int_type()
                            .const_int(v, false)
                            .const_neg()
                            .into()
                    } else {
                        let v = self.expr(x)?;
                        if e.ty.is_integer() {
                            self.binary(BinaryOp::Sub, self.ty(&e.ty)?.const_zero(), v, &e.ty)?
                        } else {
                            self.builder
                                .build_float_neg(v.into_float_value(), "neg")?
                                .into()
                        }
                    }
                }
                UnaryOp::Not | UnaryOp::BitNot => {
                    let v = self.expr(x)?.into_int_value();
                    self.builder.build_not(v, "not")?.into()
                }
            },
            ExprKind::Binary(op, l, r) => {
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    self.short_circuit(*op, l, r)?
                } else {
                    let a = self.expr(l)?;
                    let b = self.expr(r)?;
                    self.binary(*op, a, b, &l.ty)?
                }
            }
            ExprKind::Call {
                name,
                type_args,
                args,
            } => self.call(name, type_args, args, &e.ty)?,
            ExprKind::MethodCall { .. } => {
                return Err(error("unresolved method call reached code generation"));
            }
            ExprKind::Field(x, n) => {
                if let ExprKind::Name(en) = &x.kind
                    && let Some(decl) = self
                        .program
                        .enums
                        .iter()
                        .find(|en_decl| en_decl.name == *en)
                {
                    let idx = decl
                        .variants
                        .iter()
                        .position(|v| v.name == *n)
                        .ok_or_else(|| error("unknown enum variant"))?;
                    let v = self.ty(&e.ty)?.into_struct_type().const_zero();
                    return Ok(self
                        .builder
                        .build_insert_value(
                            v,
                            self.context.i32_type().const_int(idx as u64, false),
                            0,
                            "enum.tag",
                        )?
                        .into_struct_value()
                        .into());
                }
                if n == "len" {
                    self.collection(x)?.1.into()
                } else {
                    let p = self.place(e)?;
                    self.load(p, &e.ty)?
                }
            }
            ExprKind::Index(..) => {
                let p = self.place(e)?;
                self.load(p, &e.ty)?
            }
            ExprKind::Cast(x, t) => {
                let v = self.expr(x)?;
                self.cast(v, &x.ty, t)?
            }
            ExprKind::Try(x) => {
                let v = self.expr(x)?.into_struct_value();
                let Type::Result(success, failure) = &x.ty else {
                    return Err(error("propagation requires a Result"));
                };
                let st = v.get_type();
                let ptr = self.alloca(st.into(), "propagate.value")?;
                self.builder.build_store(ptr, v)?;
                let p = self.builder.build_struct_gep(st, ptr, 1, "payload")?;
                let tag = self
                    .builder
                    .build_extract_value(v, 0, "is.error")?
                    .into_int_value();
                let err = self.bb("propagate.error");
                let ok = self.bb("propagate.ok");
                self.builder.build_conditional_branch(tag, err, ok)?;
                self.builder.position_at_end(err);
                let payload = if **failure == Type::Void {
                    None
                } else {
                    Some(self.load(p, failure)?)
                };
                let ret = self.tagged_value(
                    self.ty(&self.return_type)?.into_struct_type(),
                    self.context.bool_type().const_int(1, false),
                    payload,
                )?;
                self.cleanup_to(0)?;
                self.builder.build_return(Some(&ret))?;
                self.builder.position_at_end(ok);
                if **success == Type::Void {
                    self.context.i8_type().const_zero().into()
                } else {
                    self.load(p, success)?
                }
            }
        };
        self.consume(e)?;
        Ok(value)
    }
    fn place(&mut self, e: &Expr) -> Result<PointerValue<'ctx>> {
        let previous = self.span;
        self.location(e.span);
        let result = self.place_inner(e);
        self.location(previous);
        result
    }
    fn place_inner(&mut self, e: &Expr) -> Result<PointerValue<'ctx>> {
        match &e.kind {
            ExprKind::Name(n) => self
                .binding(n)
                .map(|b| b.ptr)
                .or_else(|| self.globals.get(n).map(|g| g.0))
                .ok_or_else(|| error(format!("{n} has no address"))),
            ExprKind::Unary(UnaryOp::Deref, x) => Ok(self.expr(x)?.into_pointer_value()),
            ExprKind::Field(x, n) => {
                let (p, t) = self.autoderef_place(x)?;
                let Type::Named(name) = &t else {
                    return Err(error("field access on non-struct"));
                };
                let decl = self
                    .program
                    .structs
                    .iter()
                    .find(|s| s.name == *name)
                    .ok_or_else(|| error("unknown struct"))?;
                let i = decl
                    .fields
                    .iter()
                    .position(|f| f.name == *n)
                    .ok_or_else(|| error(format!("unknown field {n}")))?;
                Ok(self.builder.build_struct_gep(
                    self.ty(&t)?.into_struct_type(),
                    p,
                    i as u32,
                    "field.ptr",
                )?)
            }
            ExprKind::Index(x, i) => {
                let (ptr, len, t) = self.collection(x)?;
                let idx = self.expr(i)?;
                let wide = self.cast(idx, &i.ty, &Type::usize())?.into_int_value();
                let in_bounds =
                    self.builder
                        .build_int_compare(IntPredicate::ULT, wide, len, "in.bounds")?;
                self.guard(in_bounds, "index bounds", "index out of bounds")?;
                // The bounds check dominates this GEP; no inbounds assumption is needed.
                Ok(unsafe {
                    self.builder
                        .build_gep(self.ty(&t)?, ptr, &[wide], "element.ptr")?
                })
            }
            _ => {
                let v = self.expr(e)?;
                Ok(self.bind("$temporary", &e.ty, Some(v))?.ptr)
            }
        }
    }
    fn autoderef_place(&mut self, e: &Expr) -> Result<(PointerValue<'ctx>, Type)> {
        let mut ty = e.ty.clone();
        let mut p = self.place(e)?;
        while let Type::Ref(_, t) = ty {
            p = self
                .load(p, &Type::Ref(false, t.clone()))?
                .into_pointer_value();
            ty = *t;
        }
        Ok((p, ty))
    }
    fn collection(&mut self, e: &Expr) -> Result<(PointerValue<'ctx>, IntValue<'ctx>, Type)> {
        match &e.ty {
            Type::Array(n, t) => Ok((
                self.place(e)?,
                self.usize_type().const_int(*n as u64, false),
                *t.clone(),
            )),
            Type::Ref(_, t) => {
                let p = self.expr(e)?.into_pointer_value();
                match &**t {
                    Type::Array(n, t) => {
                        Ok((p, self.usize_type().const_int(*n as u64, false), *t.clone()))
                    }
                    _ => Err(error("reference is not a collection")),
                }
            }
            Type::Slice(_, t) => {
                let v = self.expr(e)?.into_struct_value();
                Ok((
                    self.builder
                        .build_extract_value(v, 0, "data")?
                        .into_pointer_value(),
                    self.builder
                        .build_extract_value(v, 1, "len")?
                        .into_int_value(),
                    *t.clone(),
                ))
            }
            Type::Str => {
                let v = self.expr(e)?.into_struct_value();
                Ok((
                    self.builder
                        .build_extract_value(v, 0, "data")?
                        .into_pointer_value(),
                    self.builder
                        .build_extract_value(v, 1, "len")?
                        .into_int_value(),
                    Type::u8(),
                ))
            }
            _ => Err(error(format!("{} is not an array or slice", e.ty))),
        }
    }
    fn array_gep(
        &self,
        p: PointerValue<'ctx>,
        t: &Type,
        i: IntValue<'ctx>,
    ) -> Result<PointerValue<'ctx>> {
        // Callers use a statically in-range index into an array of this exact type.
        Ok(unsafe {
            self.builder.build_gep(
                self.ty(t)?,
                p,
                &[self.usize_type().const_zero(), i],
                "array.element",
            )?
        })
    }
    fn guard(&mut self, valid: IntValue<'ctx>, check: &str, reason: &str) -> Result<()> {
        let ok = self.bb("checked");
        let fail = self.bb("trap");
        self.builder.build_conditional_branch(valid, ok, fail)?;
        self.builder.position_at_end(fail);
        if self.testing {
            self.report_failure(&format!("runtime check failed: {reason}"))?;
            self.trap()?;
        } else {
            self.panic(check)?;
        }
        self.builder.position_at_end(ok);
        Ok(())
    }
    fn trap(&mut self) -> Result<()> {
        let trap = Intrinsic::find("llvm.trap")
            .and_then(|i| i.get_declaration(&self.module, &[]))
            .ok_or_else(|| error("LLVM trap intrinsic is unavailable"))?;
        self.builder.build_call(trap, &[], "")?;
        self.builder.build_unreachable()?;
        Ok(())
    }
    fn test_runtime_call(&self, name: &str, args: &[BasicValueEnum<'ctx>]) -> Result<()> {
        let types: Vec<_> = args.iter().map(|v| v.get_type().into()).collect();
        let ty = self.context.void_type().fn_type(&types, false);
        let function = self
            .module
            .get_function(name)
            .unwrap_or_else(|| self.module.add_function(name, ty, None));
        if function.get_type() != ty {
            return Err(error(format!("{name} is reserved for the test runtime")));
        }
        let args: Vec<_> = args.iter().copied().map(Into::into).collect();
        self.builder.build_call(function, &args, "")?;
        Ok(())
    }
    fn report_failure(&self, message: &str) -> Result<()> {
        if !self.testing {
            return Ok(());
        }
        let paths: Vec<_> = self
            .test_sources
            .iter()
            .map(|s| s.path.display().to_string())
            .collect();
        let sources: Vec<_> = self
            .test_sources
            .iter()
            .zip(&paths)
            .map(|(s, p)| (p.as_str(), s.text.as_str(), s.start))
            .collect();
        let report =
            crate::diagnostic::Diagnostic::new(self.span, message).render_with_sources(&sources);
        let text = self
            .builder
            .build_global_string_ptr(&report, "test.diagnostic")?;
        self.test_runtime_call(
            "dodo_test_failure",
            &[
                text.as_pointer_value().into(),
                self.usize_type()
                    .const_int(report.len() as u64, false)
                    .into(),
            ],
        )
    }
    fn report_value(&self, label: u64, value: BasicValueEnum<'ctx>, ty: &Type) -> Result<()> {
        if !self.testing {
            return Ok(());
        }
        let label = self.context.i32_type().const_int(label, false).into();
        match ty {
            Type::Bool => {
                let value = self.builder.build_int_z_extend(
                    value.into_int_value(),
                    self.context.i32_type(),
                    "test.bool",
                )?;
                self.test_runtime_call("dodo_test_bool", &[label, value.into()])
            }
            Type::Str => {
                let value = value.into_struct_value();
                let ptr = self.builder.build_extract_value(value, 0, "text.ptr")?;
                let len = self.builder.build_extract_value(value, 1, "text.len")?;
                self.test_runtime_call("dodo_test_text", &[label, ptr, len])
            }
            Type::Float(_) => {
                let value = self.builder.build_float_cast(
                    value.into_float_value(),
                    self.context.f64_type(),
                    "test.float",
                )?;
                self.test_runtime_call("dodo_test_float", &[label, value.into()])
            }
            _ => {
                let signed = matches!(ty, Type::Int { signed: true, .. });
                let value = if value.is_pointer_value() {
                    self.builder.build_ptr_to_int(
                        value.into_pointer_value(),
                        self.context.i64_type(),
                        "test.address",
                    )?
                } else if value.is_struct_value() {
                    self.builder
                        .build_extract_value(value.into_struct_value(), 0, "test.tag")?
                        .into_int_value()
                } else {
                    value.into_int_value()
                };
                let value = self.builder.build_int_cast_sign_flag(
                    value,
                    self.context.i64_type(),
                    signed,
                    "test.integer",
                )?;
                self.test_runtime_call(
                    if signed {
                        "dodo_test_signed"
                    } else {
                        "dodo_test_unsigned"
                    },
                    &[label, value.into()],
                )
            }
        }
    }
    /// String assertions compare bytes, including embedded NULs, without adding
    /// a libc dependency to ordinary freestanding assertion users.
    fn strings_equal(
        &self,
        a: BasicValueEnum<'ctx>,
        b: BasicValueEnum<'ctx>,
    ) -> Result<IntValue<'ctx>> {
        let a = a.into_struct_value();
        let b = b.into_struct_value();
        let ap = self
            .builder
            .build_extract_value(a, 0, "left.ptr")?
            .into_pointer_value();
        let bp = self
            .builder
            .build_extract_value(b, 0, "right.ptr")?
            .into_pointer_value();
        let al = self
            .builder
            .build_extract_value(a, 1, "left.len")?
            .into_int_value();
        let bl = self
            .builder
            .build_extract_value(b, 1, "right.len")?
            .into_int_value();
        let result = self.alloca(self.context.bool_type().into(), "strings.equal")?;
        let index = self.alloca(self.usize_type().into(), "strings.index")?;
        self.builder
            .build_store(result, self.context.bool_type().const_zero())?;
        self.builder
            .build_store(index, self.usize_type().const_zero())?;
        let head = self.bb("strings.loop");
        let body = self.bb("strings.byte");
        let next = self.bb("strings.next");
        let equal = self.bb("strings.match");
        let end = self.bb("strings.end");
        let same_length =
            self.builder
                .build_int_compare(IntPredicate::EQ, al, bl, "same.length")?;
        self.builder
            .build_conditional_branch(same_length, head, end)?;
        self.builder.position_at_end(head);
        let i = self
            .builder
            .build_load(self.usize_type(), index, "index")?
            .into_int_value();
        let done = self
            .builder
            .build_int_compare(IntPredicate::EQ, i, al, "done")?;
        self.builder.build_conditional_branch(done, equal, body)?;
        self.builder.position_at_end(body);
        let (ap, bp) = unsafe {
            (
                self.builder
                    .build_gep(self.context.i8_type(), ap, &[i], "left.byte")?,
                self.builder
                    .build_gep(self.context.i8_type(), bp, &[i], "right.byte")?,
            )
        };
        let av = self
            .builder
            .build_load(self.context.i8_type(), ap, "left")?
            .into_int_value();
        let bv = self
            .builder
            .build_load(self.context.i8_type(), bp, "right")?
            .into_int_value();
        let same = self
            .builder
            .build_int_compare(IntPredicate::EQ, av, bv, "same.byte")?;
        self.builder.build_conditional_branch(same, next, end)?;
        self.builder.position_at_end(next);
        let increment =
            self.builder
                .build_int_add(i, self.usize_type().const_int(1, false), "next")?;
        self.builder.build_store(index, increment)?;
        self.builder.build_unconditional_branch(head)?;
        self.builder.position_at_end(equal);
        self.builder
            .build_store(result, self.context.bool_type().const_int(1, false))?;
        self.builder.build_unconditional_branch(end)?;
        self.builder.position_at_end(end);
        Ok(self
            .builder
            .build_load(self.context.bool_type(), result, "equal")?
            .into_int_value())
    }
    fn short_circuit(&mut self, op: BinaryOp, l: &Expr, r: &Expr) -> Result<BasicValueEnum<'ctx>> {
        let a = self.expr(l)?.into_int_value();
        let left = self.builder.get_insert_block().unwrap();
        let rhs = self.bb("logic.rhs");
        let end = self.bb("logic.end");
        if op == BinaryOp::And {
            self.builder.build_conditional_branch(a, rhs, end)?;
        } else {
            self.builder.build_conditional_branch(a, end, rhs)?;
        }
        self.builder.position_at_end(rhs);
        let b = self.expr(r)?.into_int_value();
        let right = self.builder.get_insert_block().unwrap();
        self.builder.build_unconditional_branch(end)?;
        self.builder.position_at_end(end);
        let phi = self.builder.build_phi(self.context.bool_type(), "logic")?;
        phi.add_incoming(&[(&a, left), (&b, right)]);
        Ok(phi.as_basic_value())
    }
    fn binary(
        &mut self,
        op: BinaryOp,
        a: BasicValueEnum<'ctx>,
        b: BasicValueEnum<'ctx>,
        t: &Type,
    ) -> Result<BasicValueEnum<'ctx>> {
        use BinaryOp::*;
        if matches!(t, Type::Float(_)) {
            let a = a.into_float_value();
            let b = b.into_float_value();
            return Ok(match op {
                Add => self.builder.build_float_add(a, b, "add")?.into(),
                Sub => self.builder.build_float_sub(a, b, "sub")?.into(),
                Mul => self.builder.build_float_mul(a, b, "mul")?.into(),
                Div => self.builder.build_float_div(a, b, "div")?.into(),
                Rem => self.builder.build_float_rem(a, b, "rem")?.into(),
                Eq | Ne | Lt | Le | Gt | Ge => self
                    .builder
                    .build_float_compare(
                        match op {
                            Eq => FloatPredicate::OEQ,
                            Ne => FloatPredicate::UNE,
                            Lt => FloatPredicate::OLT,
                            Le => FloatPredicate::OLE,
                            Gt => FloatPredicate::OGT,
                            _ => FloatPredicate::OGE,
                        },
                        a,
                        b,
                        "compare",
                    )?
                    .into(),
                _ => return Err(error("invalid floating point operation")),
            });
        }
        let (a, b) = if a.is_struct_value() {
            (
                self.builder
                    .build_extract_value(a.into_struct_value(), 0, "tag")?
                    .into_int_value(),
                self.builder
                    .build_extract_value(b.into_struct_value(), 0, "tag")?
                    .into_int_value(),
            )
        } else if a.is_pointer_value() {
            let ai = self.builder.build_ptr_to_int(
                a.into_pointer_value(),
                self.usize_type(),
                "address",
            )?;
            let bi = self.builder.build_ptr_to_int(
                b.into_pointer_value(),
                self.usize_type(),
                "address",
            )?;
            (ai, bi)
        } else {
            (a.into_int_value(), b.into_int_value())
        };
        let signed = matches!(t, Type::Int { signed: true, .. });
        let ity = a.get_type();
        Ok(match op {
            Add | Sub | Mul => {
                let name = format!(
                    "llvm.{}{}.with.overflow",
                    if signed { "s" } else { "u" },
                    match op {
                        Add => "add",
                        Sub => "sub",
                        _ => "mul",
                    }
                );
                let intrinsic = Intrinsic::find(&name)
                    .and_then(|i| i.get_declaration(&self.module, &[ity.into()]))
                    .ok_or_else(|| error("overflow intrinsic unavailable"))?;
                let out = self
                    .builder
                    .build_call(intrinsic, &[a.into(), b.into()], "checked.arithmetic")?
                    .try_as_basic_value()
                    .basic()
                    .unwrap()
                    .into_struct_value();
                let overflow = self
                    .builder
                    .build_extract_value(out, 1, "overflow")?
                    .into_int_value();
                let valid = self.builder.build_not(overflow, "no.overflow")?;
                self.guard(valid, "arithmetic overflow", "integer overflow")?;
                self.builder.build_extract_value(out, 0, "result")?
            }
            Div | Rem => {
                let nz = self.builder.build_int_compare(
                    IntPredicate::NE,
                    b,
                    ity.const_zero(),
                    "nonzero",
                )?;
                self.guard(nz, "division by zero", "division or remainder by zero")?;
                if signed {
                    let min = ity.const_int(1u64 << (ity.get_bit_width() - 1), false);
                    let a_min =
                        self.builder
                            .build_int_compare(IntPredicate::EQ, a, min, "is.min")?;
                    let b_neg = self.builder.build_int_compare(
                        IntPredicate::EQ,
                        b,
                        ity.const_all_ones(),
                        "is.neg.one",
                    )?;
                    let bad = self.builder.build_and(a_min, b_neg, "division.overflow")?;
                    let valid = self.builder.build_not(bad, "division.valid")?;
                    self.guard(valid, "division overflow", "signed division overflow")?;
                }
                match (op, signed) {
                    (Div, true) => self.builder.build_int_signed_div(a, b, "div")?.into(),
                    (Div, false) => self.builder.build_int_unsigned_div(a, b, "div")?.into(),
                    (_, true) => self.builder.build_int_signed_rem(a, b, "rem")?.into(),
                    _ => self.builder.build_int_unsigned_rem(a, b, "rem")?.into(),
                }
            }
            Eq | Ne | Lt | Le | Gt | Ge => self
                .builder
                .build_int_compare(
                    match op {
                        Eq => IntPredicate::EQ,
                        Ne => IntPredicate::NE,
                        Lt => {
                            if signed {
                                IntPredicate::SLT
                            } else {
                                IntPredicate::ULT
                            }
                        }
                        Le => {
                            if signed {
                                IntPredicate::SLE
                            } else {
                                IntPredicate::ULE
                            }
                        }
                        Gt => {
                            if signed {
                                IntPredicate::SGT
                            } else {
                                IntPredicate::UGT
                            }
                        }
                        _ => {
                            if signed {
                                IntPredicate::SGE
                            } else {
                                IntPredicate::UGE
                            }
                        }
                    },
                    a,
                    b,
                    "compare",
                )?
                .into(),
            BitAnd => self.builder.build_and(a, b, "and")?.into(),
            BitOr => self.builder.build_or(a, b, "or")?.into(),
            BitXor => self.builder.build_xor(a, b, "xor")?.into(),
            Shl | Shr => {
                let valid = self.builder.build_int_compare(
                    IntPredicate::ULT,
                    b,
                    ity.const_int(ity.get_bit_width() as u64, false),
                    "shift.valid",
                )?;
                self.guard(valid, "shift amount", "shift count out of range")?;
                if op == Shl {
                    let v = self.builder.build_left_shift(a, b, "shl")?;
                    let back = self.builder.build_right_shift(v, b, signed, "shift.back")?;
                    let fits =
                        self.builder
                            .build_int_compare(IntPredicate::EQ, back, a, "shift.fits")?;
                    self.guard(fits, "shift overflow", "left shift overflow")?;
                    v.into()
                } else {
                    self.builder.build_right_shift(a, b, signed, "shr")?.into()
                }
            }
            And | Or => return Err(error("logical operator must use short-circuit lowering")),
        })
    }
    fn cast(
        &mut self,
        v: BasicValueEnum<'ctx>,
        from: &Type,
        to: &Type,
    ) -> Result<BasicValueEnum<'ctx>> {
        match (from, to) {
            (Type::Int { signed: fs, .. }, Type::Int { signed: ts, .. }) => {
                let v = v.into_int_value();
                let out = self.ty(to)?.into_int_type();
                let source = v.get_type();
                if *fs && !*ts {
                    let nonnegative = self.builder.build_int_compare(
                        IntPredicate::SGE,
                        v,
                        source.const_zero(),
                        "cast.nonnegative",
                    )?;
                    self.guard(
                        nonnegative,
                        "numeric conversion",
                        "negative value converted to an unsigned integer",
                    )?;
                }
                let converted = self.builder.build_int_cast_sign_flag(v, out, *fs, "cast")?;
                if out.get_bit_width() < source.get_bit_width() {
                    let back = self.builder.build_int_cast_sign_flag(
                        converted,
                        source,
                        *ts,
                        "cast.back",
                    )?;
                    let fits =
                        self.builder
                            .build_int_compare(IntPredicate::EQ, back, v, "cast.fits")?;
                    self.guard(
                        fits,
                        "numeric conversion",
                        "integer conversion out of range",
                    )?;
                }
                if !*fs && *ts && out.get_bit_width() <= source.get_bit_width() {
                    let max = (1u64 << (out.get_bit_width() - 1)) - 1;
                    let fits = self.builder.build_int_compare(
                        IntPredicate::ULE,
                        v,
                        source.const_int(max, false),
                        "cast.fits",
                    )?;
                    self.guard(
                        fits,
                        "numeric conversion",
                        "integer conversion out of range",
                    )?;
                }
                Ok(converted.into())
            }
            (Type::Int { signed, .. }, Type::Float(_)) => {
                let t = self.ty(to)?.into_float_type();
                Ok(if *signed {
                    self.builder
                        .build_signed_int_to_float(v.into_int_value(), t, "cast")?
                        .into()
                } else {
                    self.builder
                        .build_unsigned_int_to_float(v.into_int_value(), t, "cast")?
                        .into()
                })
            }
            (Type::Float(_), Type::Int { signed, .. }) => {
                let v = v.into_float_value();
                let t = self.ty(to)?.into_int_type();
                let bits = t.get_bit_width();
                let low = if *signed {
                    -(2f64).powi(bits as i32 - 1)
                } else {
                    0.
                };
                let high = (2f64).powi(bits as i32 - if *signed { 1 } else { 0 });
                let lo = self.builder.build_float_compare(
                    FloatPredicate::OGE,
                    v,
                    v.get_type().const_float(low),
                    "cast.lower",
                )?;
                let hi = self.builder.build_float_compare(
                    FloatPredicate::OLT,
                    v,
                    v.get_type().const_float(high),
                    "cast.upper",
                )?;
                let valid = self.builder.build_and(lo, hi, "cast.valid")?;
                self.guard(
                    valid,
                    "numeric conversion",
                    "float-to-integer conversion out of range",
                )?;
                Ok(if *signed {
                    self.builder.build_float_to_signed_int(v, t, "cast")?.into()
                } else {
                    self.builder
                        .build_float_to_unsigned_int(v, t, "cast")?
                        .into()
                })
            }
            (Type::Float(_), Type::Float(_)) => {
                let source = v.into_float_value();
                let target = self.ty(to)?.into_float_type();
                let out = self.builder.build_float_cast(source, target, "cast")?;
                if target.get_bit_width() < source.get_type().get_bit_width() {
                    let max = source.get_type().const_float(f32::MAX as f64);
                    let min = source.get_type().const_float(-(f32::MAX as f64));
                    let hi = self.builder.build_float_compare(
                        FloatPredicate::OLE,
                        source,
                        max,
                        "cast.upper",
                    )?;
                    let lo = self.builder.build_float_compare(
                        FloatPredicate::OGE,
                        source,
                        min,
                        "cast.lower",
                    )?;
                    let valid = self.builder.build_and(hi, lo, "cast.fits")?;
                    self.guard(
                        valid,
                        "numeric conversion",
                        "floating-point conversion out of range",
                    )?;
                }
                Ok(out.into())
            }
            (Type::Raw(..) | Type::Ref(..), Type::Raw(..) | Type::Ref(..)) => Ok(v),
            (Type::Int { .. }, Type::Raw(..)) => Ok(self
                .builder
                .build_int_to_ptr(
                    v.into_int_value(),
                    self.context.ptr_type(AddressSpace::default()),
                    "pointer",
                )?
                .into()),
            (Type::Raw(..), Type::Int { .. }) => Ok(self
                .builder
                .build_ptr_to_int(
                    v.into_pointer_value(),
                    self.ty(to)?.into_int_type(),
                    "address",
                )?
                .into()),
            _ if from == to => Ok(v),
            _ => Err(error(format!(
                "cannot lower conversion from {from} to {to}"
            ))),
        }
    }
    fn call(
        &mut self,
        name: &str,
        type_args: &[Type],
        args: &[Expr],
        ret: &Type,
    ) -> Result<BasicValueEnum<'ctx>> {
        let unit = self.context.i8_type().const_zero().into();
        if matches!(
            name,
            "core.wrapping_add" | "core.wrapping_sub" | "core.wrapping_mul"
        ) {
            let a = self.expr(&args[0])?.into_int_value();
            let b = self.expr(&args[1])?.into_int_value();
            // Plain LLVM integer operations wrap modulo 2^N. Do not attach
            // no-wrap flags or route these through checked arithmetic.
            return Ok(match name {
                "core.wrapping_add" => self.builder.build_int_add(a, b, "wrapping.add")?,
                "core.wrapping_sub" => self.builder.build_int_sub(a, b, "wrapping.sub")?,
                "core.wrapping_mul" => self.builder.build_int_mul(a, b, "wrapping.mul")?,
                _ => unreachable!(),
            }
            .into());
        }
        if matches!(name, "core.assert" | "core.assert_eq" | "core.assert_ne") {
            let count = if name == "core.assert" { 1 } else { 2 };
            let values = args
                .iter()
                .map(|e| self.expr(e))
                .collect::<Result<Vec<_>>>()?;
            let valid = if count == 1 {
                values[0].into_int_value()
            } else {
                let equal = if args[0].ty == Type::Str {
                    self.strings_equal(values[0], values[1])?
                } else {
                    self.binary(BinaryOp::Eq, values[0], values[1], &args[0].ty)?
                        .into_int_value()
                };
                if name == "core.assert_ne" {
                    self.builder.build_not(equal, "not.equal")?
                } else {
                    equal
                }
            };
            let ok = self.bb("assert.passed");
            let fail = self.bb("assert.failed");
            self.builder.build_conditional_branch(valid, ok, fail)?;
            self.builder.position_at_end(fail);
            self.report_failure(&format!("{} failed", name.trim_start_matches("core.")))?;
            if count == 2 {
                self.report_value(0, values[0], &args[0].ty)?;
                self.report_value(1, values[1], &args[1].ty)?;
            }
            if let Some(message) = values.get(count) {
                self.report_value(2, *message, &Type::Str)?;
            }
            if self.testing {
                self.trap()?;
            } else {
                self.panic(name.trim_start_matches("core."))?;
            }
            self.builder.position_at_end(ok);
            return Ok(unit);
        }
        if name == "ok" || name == "err" || name == "some" || name == "none" {
            if matches!(ret, Type::Result(..)) {
                let payload = args.first().map(|e| self.expr(e)).transpose()?;
                return self.tagged_value(
                    self.ty(ret)?.into_struct_type(),
                    self.context
                        .bool_type()
                        .const_int((name == "err") as u64, false),
                    payload,
                );
            }
            let mut v = self.ty(ret)?.into_struct_type().const_zero();
            let tag = name == "err" || name == "some";
            v = self
                .builder
                .build_insert_value(
                    v,
                    self.context.bool_type().const_int(tag as u64, false),
                    0,
                    "tag",
                )?
                .into_struct_value();
            if let Some(e) = args.first() {
                let x = self.expr(e)?;
                v = self
                    .builder
                    .build_insert_value(v, x, 1, "payload")?
                    .into_struct_value();
            }
            return Ok(v.into());
        }
        if name == "core.drop" {
            let e = &args[0];
            let v = self.expr(e)?;
            let ptr = self.alloca(self.ty(&e.ty)?, "drop.value")?;
            self.builder.build_store(ptr, v)?;
            self.drop_ptr(ptr, &e.ty)?;
            return Ok(unit);
        }
        if matches!(
            name,
            "mem.size_of"
                | "core.mem.size_of"
                | "mem.align_of"
                | "core.mem.align_of"
                | "mem.offset_of"
                | "core.mem.offset_of"
        ) {
            let argument = type_args
                .first()
                .ok_or_else(|| error("memory query requires a type argument"))?;
            if *argument == Type::Void {
                return Ok(self
                    .usize_type()
                    .const_int(u64::from(name.ends_with("align_of")), false)
                    .into());
            }
            let t = self.ty(argument)?;
            let data = inkwell::targets::TargetData::create(
                self.module
                    .get_data_layout()
                    .as_str()
                    .to_str()
                    .map_err(|_| error("invalid LLVM data layout"))?,
            );
            let value = if name.ends_with("size_of") {
                data.get_abi_size(&t)
            } else if name.ends_with("offset_of") {
                let Type::Named(owner) = argument else {
                    return Err(error("field offset requires a struct type"));
                };
                let ExprKind::String(bytes, false) = &args[0].kind else {
                    return Err(error("field offset requires a literal field name"));
                };
                let index = self
                    .program
                    .structs
                    .iter()
                    .find(|s| s.name == *owner)
                    .and_then(|s| s.fields.iter().position(|f| f.name.as_bytes() == bytes))
                    .ok_or_else(|| error("unknown field in field offset query"))?;
                data.offset_of_element(&t.into_struct_type(), index as u32)
                    .ok_or_else(|| error("field offset is unavailable for this struct"))?
            } else {
                data.get_abi_alignment(&t) as u64
            };
            return Ok(self.usize_type().const_int(value, false).into());
        }
        if let Some(op) = name.strip_prefix("core.mem.") {
            if let Some(operation) = op.strip_prefix("atomic_") {
                let triple = self
                    .module
                    .get_triple()
                    .as_str()
                    .to_string_lossy()
                    .into_owned();
                if !["x86_64-", "aarch64-"]
                    .iter()
                    .any(|arch| triple.starts_with(arch))
                {
                    return Err(error(format!(
                        "atomic integer primitives are not supported for target `{triple}`; no OS or libatomic fallback is inserted"
                    )));
                }
                let pointer = self.expr(&args[0])?.into_pointer_value();
                let order = |index: usize| -> Result<AtomicOrdering> {
                    let ExprKind::Int(value, _) = args[index].kind else {
                        return Err(error("atomic ordering was not checked"));
                    };
                    Ok(match value {
                        0 => AtomicOrdering::Monotonic,
                        1 => AtomicOrdering::Acquire,
                        2 => AtomicOrdering::Release,
                        3 => AtomicOrdering::AcquireRelease,
                        4 => AtomicOrdering::SequentiallyConsistent,
                        _ => return Err(error("invalid atomic ordering")),
                    })
                };
                match operation {
                    "load" => {
                        let value =
                            self.builder
                                .build_load(self.ty(ret)?, pointer, "atomic.load")?;
                        let instruction = value.as_instruction_value().unwrap();
                        instruction.set_atomic_ordering(order(1)?).map_err(error)?;
                        instruction
                            .set_alignment(self.ty(ret)?.into_int_type().get_bit_width() / 8)
                            .map_err(error)?;
                        return Ok(value);
                    }
                    "store" => {
                        let value = self.expr(&args[1])?;
                        let instruction = self.builder.build_store(pointer, value)?;
                        instruction.set_atomic_ordering(order(2)?).map_err(error)?;
                        instruction
                            .set_alignment(value.into_int_value().get_type().get_bit_width() / 8)
                            .map_err(error)?;
                        return Ok(unit);
                    }
                    "exchange" | "fetch_add" => {
                        let value = self.expr(&args[1])?.into_int_value();
                        return Ok(self
                            .builder
                            .build_atomicrmw(
                                if operation == "exchange" {
                                    AtomicRMWBinOp::Xchg
                                } else {
                                    AtomicRMWBinOp::Add
                                },
                                pointer,
                                value,
                                order(2)?,
                            )?
                            .into());
                    }
                    "compare_exchange" => {
                        let expected = self.expr(&args[1])?.into_int_value();
                        let replacement = self.expr(&args[2])?.into_int_value();
                        let result = self.builder.build_cmpxchg(
                            pointer,
                            expected,
                            replacement,
                            order(3)?,
                            order(4)?,
                        )?;
                        return Ok(self.builder.build_extract_value(
                            result,
                            0,
                            "atomic.observed",
                        )?);
                    }
                    _ => return Err(error("unknown atomic intrinsic")),
                }
            }
            match op {
                "assert_send" | "assert_sync" => return Ok(unit),
                "callback" => {
                    let ExprKind::Name(target) = &args[0].kind else {
                        return Err(error("callback target was not checked"));
                    };
                    return Ok(self
                        .functions
                        .get(target)
                        .ok_or_else(|| error("callback function is unavailable"))?
                        .as_global_value()
                        .as_pointer_value()
                        .into());
                }
                "uninit" => {
                    // The bytes are deliberately opaque. Zero initialization is an
                    // implementation choice, never a claim that T is initialized.
                    return Ok(self.ty(ret)?.const_zero());
                }
                "init" => {
                    let value = self.expr(&args[0])?;
                    return Ok(self
                        .builder
                        .build_insert_value(
                            self.ty(ret)?.into_array_type().const_zero(),
                            value,
                            0,
                            "storage.init",
                        )?
                        .into_array_value()
                        .into());
                }
                "assume_init" => {
                    let storage = self.expr(&args[0])?.into_array_value();
                    return Ok(self.builder.build_extract_value(
                        storage,
                        0,
                        "storage.assume_init",
                    )?);
                }
                "uninit_as_ptr" | "uninit_as_mut_ptr" | "str_bytes" | "str_from_utf8" => {
                    return self.expr(&args[0]);
                }
                "replace" | "swap" => {
                    let pointer = self.expr(&args[0])?.into_pointer_value();
                    let Type::Ref(_, element) = &args[0].ty else {
                        return Err(error("memory exchange requires a reference"));
                    };
                    // Evaluate both operands before changing storage: a propagated
                    // failure in the replacement must leave the old value intact.
                    let replacement = self.expr(&args[1])?;
                    let old = self
                        .builder
                        .build_load(self.ty(element)?, pointer, "mem.old")?;
                    if op == "replace" {
                        self.builder.build_store(pointer, replacement)?;
                        return Ok(old);
                    }
                    let other = replacement.into_pointer_value();
                    let second = self
                        .builder
                        .build_load(self.ty(element)?, other, "mem.other")?;
                    self.builder.build_store(pointer, second)?;
                    self.builder.build_store(other, old)?;
                    return Ok(unit);
                }
                _ => (),
            }
        }
        let mmio = name
            .strip_prefix("mmio.")
            .or_else(|| name.strip_prefix("core.mmio."));
        if let Some(op) = mmio {
            let address = self.expr(&args[0])?.into_int_value();
            let pointer = self.builder.build_int_to_ptr(
                address,
                self.context.ptr_type(AddressSpace::default()),
                "mmio.address",
            )?;
            if op.starts_with("read") {
                let value = self
                    .builder
                    .build_load(self.ty(ret)?, pointer, "mmio.read")?;
                value
                    .as_instruction_value()
                    .unwrap()
                    .set_volatile(true)
                    .map_err(error)?;
                return Ok(value);
            }
            if op.starts_with("write") {
                let value = self.expr(&args[1])?;
                self.builder
                    .build_store(pointer, value)?
                    .set_volatile(true)
                    .map_err(error)?;
                return Ok(unit);
            }
        }
        if let Some(op) = name
            .strip_prefix("ptr.")
            .or_else(|| name.strip_prefix("core.ptr."))
        {
            let input = self.expr(&args[0])?;
            if matches!(op, "as_ptr" | "as_mut_ptr") && matches!(args[0].ty, Type::Slice(..)) {
                return Ok(self.builder.build_extract_value(
                    input.into_struct_value(),
                    0,
                    "slice.pointer",
                )?);
            }
            let pointer = input.into_pointer_value();
            match op {
                "borrow" | "borrow_mut" => {
                    self.expr(&args[1])?;
                    return Ok(pointer.into());
                }
                "borrow_slice" | "borrow_slice_mut" => {
                    let length = self.expr(&args[1])?;
                    self.expr(&args[2])?;
                    let value = self
                        .builder
                        .build_insert_value(
                            self.ty(ret)?.into_struct_type().const_zero(),
                            pointer,
                            0,
                            "owned.slice.pointer",
                        )?
                        .into_struct_value();
                    return Ok(self
                        .builder
                        .build_insert_value(value, length, 1, "owned.slice.length")?
                        .into_struct_value()
                        .into());
                }
                "from_ref" | "from_mut" | "as_ptr" | "as_mut_ptr" => return Ok(pointer.into()),
                "is_null" => return Ok(self.builder.build_is_null(pointer, "ptr.is_null")?.into()),
                "copy" | "copy_nonoverlapping" | "write_bytes" => {
                    let second = self.expr(&args[1])?;
                    let count = self.expr(&args[2])?.into_int_value();
                    let Type::Raw(_, element) = &args[0].ty else {
                        return Err(error("memory operation requires a raw pointer"));
                    };
                    let data = inkwell::targets::TargetData::create(
                        self.module
                            .get_data_layout()
                            .as_str()
                            .to_str()
                            .map_err(|_| error("invalid LLVM data layout"))?,
                    );
                    let size = self
                        .usize_type()
                        .const_int(data.get_abi_size(&self.ty(element)?), false);
                    // The unsafe caller guarantees the complete byte extent fits.
                    let bytes = self.builder.build_int_mul(count, size, "ptr.byte_count")?;
                    match op {
                        "copy" => {
                            self.builder.build_memmove(
                                second.into_pointer_value(),
                                1,
                                pointer,
                                1,
                                bytes,
                            )?;
                        }
                        "copy_nonoverlapping" => {
                            self.builder.build_memcpy(
                                second.into_pointer_value(),
                                1,
                                pointer,
                                1,
                                bytes,
                            )?;
                        }
                        _ => {
                            self.builder.build_memset(
                                pointer,
                                1,
                                second.into_int_value(),
                                bytes,
                            )?;
                        }
                    }
                    return Ok(unit);
                }
                "drop_in_place" => {
                    let Type::Raw(_, element) = &args[0].ty else {
                        return Err(error("dropping in place requires a raw pointer"));
                    };
                    self.drop_ptr(pointer, element)?;
                    return Ok(unit);
                }
                "read" | "read_unaligned" | "read_volatile" => {
                    let value = self
                        .builder
                        .build_load(self.ty(ret)?, pointer, "ptr.read")?;
                    let i = value.as_instruction_value().unwrap();
                    if op == "read_unaligned" {
                        i.set_alignment(1).map_err(error)?;
                    }
                    if op == "read_volatile" {
                        i.set_volatile(true).map_err(error)?;
                    }
                    return Ok(value);
                }
                "write" | "write_unaligned" | "write_volatile" => {
                    let value = self.expr(&args[1])?;
                    let i = self.builder.build_store(pointer, value)?;
                    if op == "write_unaligned" {
                        i.set_alignment(1).map_err(error)?;
                    }
                    if op == "write_volatile" {
                        i.set_volatile(true).map_err(error)?;
                    }
                    return Ok(unit);
                }
                "offset" => {
                    let offset = self.expr(&args[1])?.into_int_value();
                    let Type::Raw(_, t) = ret else {
                        return Err(error("pointer offset requires raw pointer"));
                    };
                    return Ok(unsafe {
                        self.builder
                            .build_gep(self.ty(t)?, pointer, &[offset], "ptr.offset")?
                    }
                    .into());
                }
                _ => {}
            }
        }
        if let Some((en, variant)) = name.rsplit_once('.')
            && let Some(decl) = self.program.enums.iter().find(|e| e.name == en)
        {
            let i = decl
                .variants
                .iter()
                .position(|v| v.name == variant)
                .ok_or_else(|| error("unknown variant"))?;
            let st = self.ty(ret)?.into_struct_type();
            let mut payload = self.variant_ty(&decl.variants[i].fields)?.const_zero();
            let mut pending = Vec::new();
            for (j, arg) in args.iter().enumerate() {
                let x = self.stage(arg, &mut pending)?;
                payload = self
                    .builder
                    .build_insert_value(payload, x, j as u32, "payload")?
                    .into_struct_value();
            }
            let v = self.tagged_value(
                st,
                self.context.i32_type().const_int(i as u64, false),
                Some(payload.into()),
            )?;
            self.transfer(&pending)?;
            return Ok(v);
        }
        let f = self
            .functions
            .get(name)
            .copied()
            .ok_or_else(|| error(format!("unknown function {name}")))?;
        let mut pending = Vec::new();
        let vs = args
            .iter()
            .map(|e| {
                self.stage(e, &mut pending)
                    .map(BasicMetadataValueEnum::from)
            })
            .collect::<Result<Vec<_>>>()?;
        self.transfer(&pending)?;
        let call = self
            .builder
            .build_call(f, &vs, if *ret == Type::Void { "" } else { "call" })?;
        if let Some(function) = self
            .program
            .functions
            .iter()
            .find(|f| f.name == name && f.extern_)
        {
            for (location, attribute) in self.c_abi_attributes(function) {
                call.add_attribute(location, attribute);
            }
        }
        Ok(call.try_as_basic_value().basic().unwrap_or(unit))
    }
    fn foreach(
        &mut self,
        index: Option<&str>,
        name: &str,
        copy: bool,
        iterable: &Expr,
        body: &Block,
    ) -> Result<()> {
        let (ptr, len, element) = self.collection(iterable)?;
        let mutable = matches!(iterable.ty, Type::Slice(true, _) | Type::Ref(true, _))
            || matches!(iterable.kind, ExprKind::Unary(UnaryOp::BorrowMut, _));
        let counter = self.alloca(self.usize_type().into(), "foreach.index")?;
        self.builder
            .build_store(counter, self.usize_type().const_zero())?;
        let head = self.bb("foreach.condition");
        let bb = self.bb("foreach.body");
        let step = self.bb("foreach.step");
        let end = self.bb("foreach.end");
        self.builder.build_unconditional_branch(head)?;
        self.builder.position_at_end(head);
        let i = self
            .builder
            .build_load(self.usize_type(), counter, "index")?
            .into_int_value();
        let test = self
            .builder
            .build_int_compare(IntPredicate::ULT, i, len, "foreach.more")?;
        self.builder.build_conditional_branch(test, bb, end)?;
        self.builder.position_at_end(bb);
        self.loops.push(Loop {
            end,
            next: step,
            depth: self.scopes.len(),
        });
        self.push_scope();
        if let Some(name) = index {
            self.bind(name, &Type::usize(), Some(i.into()))?;
        }
        // The loop condition proves the element index is below the captured length.
        let p = unsafe {
            self.builder
                .build_gep(self.ty(&element)?, ptr, &[i], "foreach.element")?
        };
        if copy {
            if name != "_" {
                let value = self.load(p, &element)?;
                self.bind(name, &element, Some(value))?;
            }
        } else {
            self.bind(name, &Type::Ref(mutable, Box::new(element)), Some(p.into()))?;
        }
        self.block(body)?;
        if !self.terminated() {
            self.cleanup_to(self.scopes.len() - 1)?;
            self.builder.build_unconditional_branch(step)?;
        }
        self.pop_scope();
        self.loops.pop();
        self.builder.position_at_end(step);
        let next = self
            .builder
            .build_int_add(i, self.usize_type().const_int(1, false), "next")?;
        self.builder.build_store(counter, next)?;
        self.builder.build_unconditional_branch(head)?;
        self.builder.position_at_end(end);
        Ok(())
    }
    /// Expand nested alternatives in source order. Each alternative gets its own
    /// guard evaluation, including when several alternatives match the value.
    fn pattern_alternatives(pattern: &Pattern) -> Vec<Pattern> {
        match pattern {
            Pattern::Or(patterns) => patterns
                .iter()
                .flat_map(Self::pattern_alternatives)
                .collect(),
            Pattern::Variant(name, fields) => {
                let mut combinations = vec![vec![]];
                for field in fields {
                    let alternatives = Self::pattern_alternatives(field);
                    combinations = combinations
                        .into_iter()
                        .flat_map(|prefix| {
                            alternatives.iter().map(move |alternative| {
                                let mut fields = prefix.clone();
                                fields.push(alternative.clone());
                                fields
                            })
                        })
                        .collect();
                }
                combinations
                    .into_iter()
                    .map(|fields| Pattern::Variant(name.clone(), fields))
                    .collect()
            }
            Pattern::Struct(name, fields, rest) => {
                let mut combinations = vec![vec![]];
                for (field, pattern) in fields {
                    let alternatives = Self::pattern_alternatives(pattern);
                    combinations = combinations
                        .into_iter()
                        .flat_map(|prefix| {
                            alternatives.iter().map(move |alternative| {
                                let mut fields = prefix.clone();
                                fields.push((field.clone(), alternative.clone()));
                                fields
                            })
                        })
                        .collect();
                }
                combinations
                    .into_iter()
                    .map(|fields| Pattern::Struct(name.clone(), fields, *rest))
                    .collect()
            }
            _ => vec![pattern.clone()],
        }
    }
    fn pattern_value(
        &mut self,
        value: &Expr,
    ) -> Result<(
        PointerValue<'ctx>,
        Type,
        Option<bool>,
        Option<Binding<'ctx>>,
    )> {
        if let Type::Ref(mutable, inner) = &value.ty {
            Ok((
                self.expr(value)?.into_pointer_value(),
                *inner.clone(),
                Some(*mutable),
                None,
            ))
        } else {
            let value_ir = self.expr(value)?;
            let owner = self.bind("$pattern", &value.ty, Some(value_ir))?;
            Ok((owner.ptr, value.ty.clone(), None, Some(owner)))
        }
    }
    /// Reference fields are inspected through their pointees for a structural
    /// pattern. A binding still binds the reference itself.
    fn pattern_place(
        &self,
        pattern: &Pattern,
        mut ptr: PointerValue<'ctx>,
        ty: &Type,
        mut borrowed: Option<bool>,
    ) -> Result<(PointerValue<'ctx>, Type, Option<bool>)> {
        let mut ty = ty.clone();
        if !matches!(pattern, Pattern::Binding(_) | Pattern::Wildcard) {
            while let Type::Ref(mutable, inner) = &ty {
                ptr = self.load(ptr, &ty)?.into_pointer_value();
                borrowed = Some(borrowed.is_none_or(|outer| outer) && *mutable);
                ty = *inner.clone();
            }
        }
        Ok((ptr, ty, borrowed))
    }
    fn struct_pattern_fields(
        &self,
        ptr: PointerValue<'ctx>,
        ty: &Type,
    ) -> Result<Vec<(String, PointerValue<'ctx>, Type)>> {
        let Type::Named(name) = ty else {
            return Err(error("struct pattern on non-struct value"));
        };
        let declaration = self
            .program
            .structs
            .iter()
            .find(|s| s.name == *name)
            .ok_or_else(|| error("unknown struct pattern"))?;
        let st = self.ty(ty)?.into_struct_type();
        declaration
            .fields
            .iter()
            .enumerate()
            .map(|(i, field)| {
                Ok((
                    field.name.clone(),
                    self.builder
                        .build_struct_gep(st, ptr, i as u32, "pattern.field")?,
                    field.ty.clone(),
                ))
            })
            .collect()
    }
    /// Emit short-circuit tests so inactive enum payloads are never loaded.
    fn pattern_test(
        &mut self,
        pattern: &Pattern,
        ptr: PointerValue<'ctx>,
        ty: &Type,
        yes: BasicBlock<'ctx>,
        no: BasicBlock<'ctx>,
    ) -> Result<()> {
        let (ptr, ty, _) = self.pattern_place(pattern, ptr, ty, None)?;
        match pattern {
            Pattern::Binding(_) | Pattern::Wildcard => {
                self.builder.build_unconditional_branch(yes)?;
            }
            Pattern::Int(value) | Pattern::Range(value, _, _) => {
                let value_ir = self.load(ptr, &ty)?.into_int_value();
                let signed = matches!(ty, Type::Int { signed: true, .. });
                let lower = value_ir.get_type().const_int(*value, false);
                let test = if let Pattern::Range(_, upper, inclusive) = pattern {
                    let lower_test = self.builder.build_int_compare(
                        if signed {
                            IntPredicate::SGE
                        } else {
                            IntPredicate::UGE
                        },
                        value_ir,
                        lower,
                        "pattern.lower",
                    )?;
                    let upper_test = self.builder.build_int_compare(
                        match (signed, inclusive) {
                            (true, true) => IntPredicate::SLE,
                            (true, false) => IntPredicate::SLT,
                            (false, true) => IntPredicate::ULE,
                            (false, false) => IntPredicate::ULT,
                        },
                        value_ir,
                        value_ir.get_type().const_int(*upper, false),
                        "pattern.upper",
                    )?;
                    self.builder
                        .build_and(lower_test, upper_test, "pattern.range")?
                } else {
                    self.builder.build_int_compare(
                        IntPredicate::EQ,
                        value_ir,
                        lower,
                        "pattern.equal",
                    )?
                };
                self.builder.build_conditional_branch(test, yes, no)?;
            }
            Pattern::Bool(value) => {
                let value_ir = self.load(ptr, &ty)?.into_int_value();
                let test = self.builder.build_int_compare(
                    IntPredicate::EQ,
                    value_ir,
                    self.context.bool_type().const_int(*value as u64, false),
                    "pattern.bool",
                )?;
                self.builder.build_conditional_branch(test, yes, no)?;
            }
            Pattern::Variant(name, fields) => {
                let tagptr = self.builder.build_struct_gep(
                    self.ty(&ty)?.into_struct_type(),
                    ptr,
                    0,
                    "pattern.tag",
                )?;
                let tag_type = if matches!(ty, Type::Named(_)) {
                    self.context.i32_type()
                } else {
                    self.context.bool_type()
                };
                let tag = self
                    .builder
                    .build_load(tag_type, tagptr, "tag")?
                    .into_int_value();
                let test = self.builder.build_int_compare(
                    IntPredicate::EQ,
                    tag,
                    tag_type.const_int(self.variant_tag(&ty, name)?, false),
                    "pattern.variant",
                )?;
                if fields.is_empty() {
                    self.builder.build_conditional_branch(test, yes, no)?;
                } else {
                    let payload = self.bb("pattern.payload");
                    self.builder.build_conditional_branch(test, payload, no)?;
                    self.builder.position_at_end(payload);
                    let payloads = self.pattern_payloads(ptr, &ty, name)?;
                    for (i, (field, (ptr, ty))) in fields.iter().zip(payloads).enumerate() {
                        let next = if i + 1 == fields.len() {
                            yes
                        } else {
                            self.bb("pattern.next")
                        };
                        self.pattern_test(field, ptr, &ty, next, no)?;
                        if i + 1 != fields.len() {
                            self.builder.position_at_end(next);
                        }
                    }
                }
            }
            Pattern::Struct(_, fields, _) => {
                let all_fields = self.struct_pattern_fields(ptr, &ty)?;
                if fields.is_empty() {
                    self.builder.build_unconditional_branch(yes)?;
                }
                for (i, (name, field)) in fields.iter().enumerate() {
                    let (_, ptr, ty) = all_fields
                        .iter()
                        .find(|(n, _, _)| n == name)
                        .ok_or_else(|| error("unknown pattern field"))?;
                    let next = if i + 1 == fields.len() {
                        yes
                    } else {
                        self.bb("pattern.next")
                    };
                    self.pattern_test(field, *ptr, ty, next, no)?;
                    if i + 1 != fields.len() {
                        self.builder.position_at_end(next);
                    }
                }
            }
            Pattern::Or(_) => return Err(error("unexpanded pattern alternative")),
        }
        Ok(())
    }
    /// Preview bindings only alias the scrutinee for guard evaluation. Commit
    /// transfers ownership and drops the fields omitted by the chosen pattern.
    fn pattern_bind(
        &mut self,
        pattern: &Pattern,
        ptr: PointerValue<'ctx>,
        ty: &Type,
        borrowed: Option<bool>,
        preview: bool,
        destinations: Option<&HashMap<String, Binding<'ctx>>>,
    ) -> Result<()> {
        let (ptr, ty, borrowed) = self.pattern_place(pattern, ptr, ty, borrowed)?;
        match pattern {
            Pattern::Binding(name) => {
                let (binding_ty, value) = if let Some(mutable) = borrowed {
                    (Type::Ref(mutable, Box::new(ty)), ptr.into())
                } else {
                    let value = self.load(ptr, &ty)?;
                    (ty, value)
                };
                if let Some(destinations) = destinations {
                    let binding = &destinations[name];
                    self.builder.build_store(binding.ptr, value)?;
                    if let Some(flag) = binding.live {
                        self.builder
                            .build_store(flag, self.context.bool_type().const_int(1, false))?;
                    }
                } else {
                    let binding = self.bind(name, &binding_ty, Some(value))?;
                    if preview && let Some(flag) = binding.live {
                        self.builder
                            .build_store(flag, self.context.bool_type().const_zero())?;
                    }
                }
            }
            Pattern::Variant(name, fields) => {
                for (field, (ptr, ty)) in fields.iter().zip(self.pattern_payloads(ptr, &ty, name)?)
                {
                    self.pattern_bind(field, ptr, &ty, borrowed, preview, destinations)?;
                }
            }
            Pattern::Struct(_, fields, _) => {
                let all_fields = self.struct_pattern_fields(ptr, &ty)?;
                for (name, pattern) in fields {
                    let (_, ptr, ty) = all_fields
                        .iter()
                        .find(|(n, _, _)| n == name)
                        .ok_or_else(|| error("unknown pattern field"))?;
                    self.pattern_bind(pattern, *ptr, ty, borrowed, preview, destinations)?;
                }
                for (name, ptr, ty) in all_fields.into_iter().rev() {
                    if !fields.iter().any(|(n, _)| *n == name) {
                        self.pattern_bind(
                            &Pattern::Wildcard,
                            ptr,
                            &ty,
                            borrowed,
                            preview,
                            destinations,
                        )?;
                    }
                }
            }
            Pattern::Wildcard if !preview && borrowed.is_none() => self.drop_ptr(ptr, &ty)?,
            Pattern::Or(_) => return Err(error("unexpanded pattern alternative")),
            _ => {}
        }
        Ok(())
    }
    fn pattern_commit(&self, owner: &Option<Binding<'ctx>>) -> Result<()> {
        if let Some(owner) = owner
            && let Some(flag) = owner.live
        {
            self.builder
                .build_store(flag, self.context.bool_type().const_zero())?;
        }
        Ok(())
    }
    fn order_pattern_bindings(&mut self, names: &[String]) {
        // Alternatives introduce one lexical set of bindings, whose drop
        // order is defined by the first alternative even when another wins.
        self.scopes
            .last_mut()
            .unwrap()
            .sort_by_key(|binding| names.iter().position(|name| *name == binding.name));
    }
    fn match_stmt(&mut self, value: &Expr, arms: &[MatchArm]) -> Result<()> {
        self.push_scope();
        let (ptr, actual, borrowed, owner) = self.pattern_value(value)?;
        let end = self.bb("match.end");
        let mut reaches_end = false;
        for arm in arms {
            let binding_order = arm.pattern.bindings();
            for pattern in Self::pattern_alternatives(&arm.pattern) {
                let yes = self.bb("match.pattern");
                let no = self.bb("match.next");
                self.pattern_test(&pattern, ptr, &actual, yes, no)?;
                self.builder.position_at_end(yes);
                if let Some(guard) = &arm.guard {
                    self.push_scope();
                    self.pattern_bind(&pattern, ptr, &actual, borrowed, true, None)?;
                    self.order_pattern_bindings(&binding_order);
                    let condition = self.expr(guard)?.into_int_value();
                    self.cleanup_to(self.scopes.len() - 1)?;
                    self.pop_scope();
                    let accepted = self.bb("match.guarded");
                    self.builder
                        .build_conditional_branch(condition, accepted, no)?;
                    self.builder.position_at_end(accepted);
                }
                self.push_scope();
                self.pattern_commit(&owner)?;
                self.pattern_bind(&pattern, ptr, &actual, borrowed, false, None)?;
                self.order_pattern_bindings(&binding_order);
                self.block(&arm.body)?;
                if !self.terminated() {
                    self.cleanup_to(self.scopes.len() - 1)?;
                    self.builder.build_unconditional_branch(end)?;
                    reaches_end = true;
                }
                self.pop_scope();
                self.builder.position_at_end(no);
            }
        }
        self.builder.build_unreachable()?;
        self.builder.position_at_end(end);
        if !reaches_end {
            self.builder.build_unreachable()?;
        }
        self.pop_scope();
        Ok(())
    }
    fn if_let_stmt(
        &mut self,
        pattern: &Pattern,
        value: &Expr,
        then_block: &Block,
        else_block: &Block,
    ) -> Result<()> {
        self.push_scope();
        let (ptr, actual, borrowed, owner) = self.pattern_value(value)?;
        let end = self.bb("if.let.end");
        let mut reaches_end = false;
        let binding_order = pattern.bindings();
        for pattern in Self::pattern_alternatives(pattern) {
            let yes = self.bb("if.let.then");
            let no = self.bb("if.let.next");
            self.pattern_test(&pattern, ptr, &actual, yes, no)?;
            self.builder.position_at_end(yes);
            self.push_scope();
            self.pattern_commit(&owner)?;
            self.pattern_bind(&pattern, ptr, &actual, borrowed, false, None)?;
            self.order_pattern_bindings(&binding_order);
            self.block(then_block)?;
            if !self.terminated() {
                self.cleanup_to(self.scopes.len() - 1)?;
                self.builder.build_unconditional_branch(end)?;
                reaches_end = true;
            }
            self.pop_scope();
            self.builder.position_at_end(no);
        }
        if let Some(owner) = &owner {
            self.drop_binding(owner)?;
        }
        self.block(else_block)?;
        if !self.terminated() {
            self.builder.build_unconditional_branch(end)?;
            reaches_end = true;
        }
        self.builder.position_at_end(end);
        if !reaches_end {
            self.builder.build_unreachable()?;
        }
        self.pop_scope();
        Ok(())
    }
    fn pattern_binding_types(
        &self,
        pattern: &Pattern,
        ty: &Type,
        mut borrowed: Option<bool>,
        bindings: &mut Vec<(String, Type)>,
    ) -> Result<()> {
        let mut ty = ty;
        if !matches!(pattern, Pattern::Binding(_) | Pattern::Wildcard) {
            while let Type::Ref(mutable, inner) = ty {
                borrowed = Some(borrowed.is_none_or(|outer| outer) && *mutable);
                ty = inner;
            }
        }
        match pattern {
            Pattern::Binding(name) => {
                bindings.push((
                    name.clone(),
                    borrowed.map_or_else(
                        || ty.clone(),
                        |mutable| Type::Ref(mutable, Box::new(ty.clone())),
                    ),
                ));
            }
            Pattern::Variant(name, fields) => {
                let tag = self.variant_tag(ty, name)?;
                let types = match ty {
                    Type::Result(ok, err) => {
                        let ty = if tag == 0 { ok } else { err };
                        if **ty == Type::Void {
                            vec![]
                        } else {
                            vec![ty.as_ref()]
                        }
                    }
                    Type::Option(inner) => {
                        if tag == 0 {
                            vec![]
                        } else {
                            vec![inner.as_ref()]
                        }
                    }
                    Type::Named(name) => self
                        .program
                        .enums
                        .iter()
                        .find(|e| e.name == *name)
                        .ok_or_else(|| error("unknown enum pattern"))?
                        .variants[tag as usize]
                        .fields
                        .iter()
                        .map(|f| &f.ty)
                        .collect(),
                    _ => return Err(error("invalid payload pattern")),
                };
                for (pattern, ty) in fields.iter().zip(types) {
                    self.pattern_binding_types(pattern, ty, borrowed, bindings)?;
                }
            }
            Pattern::Struct(_, fields, _) => {
                let Type::Named(name) = ty else {
                    return Err(error("invalid struct pattern"));
                };
                let declaration = self
                    .program
                    .structs
                    .iter()
                    .find(|s| s.name == *name)
                    .ok_or_else(|| error("unknown struct pattern"))?;
                for (name, pattern) in fields {
                    let field = declaration
                        .fields
                        .iter()
                        .find(|f| f.name == *name)
                        .ok_or_else(|| error("unknown pattern field"))?;
                    self.pattern_binding_types(pattern, &field.ty, borrowed, bindings)?;
                }
            }
            Pattern::Or(_) => return Err(error("unexpanded pattern alternative")),
            _ => {}
        }
        Ok(())
    }
    fn let_pattern_stmt(
        &mut self,
        pattern: &Pattern,
        value: &Expr,
        else_block: Option<&Block>,
    ) -> Result<()> {
        let (ptr, actual, borrowed, owner) = self.pattern_value(value)?;
        let alternatives = Self::pattern_alternatives(pattern);
        // Allocate the names once so every successful alternative initializes
        // the same bindings visible to the following statements.
        let mut binding_types = vec![];
        self.pattern_binding_types(&alternatives[0], &actual, borrowed, &mut binding_types)?;
        self.scopes.push(vec![]);
        for (name, ty) in binding_types {
            self.bind(&name, &ty, None)?;
        }
        let destinations = self
            .scopes
            .pop()
            .unwrap()
            .into_iter()
            .map(|binding| (binding.name.clone(), binding))
            .collect::<HashMap<_, _>>();
        let end = self.bb("let.pattern.end");
        for pattern in alternatives {
            let yes = self.bb("let.pattern.bound");
            let no = self.bb("let.pattern.next");
            self.pattern_test(&pattern, ptr, &actual, yes, no)?;
            self.builder.position_at_end(yes);
            self.pattern_commit(&owner)?;
            self.pattern_bind(&pattern, ptr, &actual, borrowed, false, Some(&destinations))?;
            self.builder.build_unconditional_branch(end)?;
            self.builder.position_at_end(no);
        }
        if let Some(owner) = &owner {
            self.drop_binding(owner)?;
        }
        if let Some(block) = else_block {
            self.block(block)?;
        }
        if !self.terminated() {
            self.builder.build_unreachable()?;
        }
        self.builder.position_at_end(end);
        // Preserve source binding order for deterministic reverse-order drops.
        for name in pattern.bindings() {
            self.scopes
                .last_mut()
                .unwrap()
                .push(destinations[&name].clone());
        }
        Ok(())
    }
    fn variant_tag(&self, t: &Type, name: &str) -> Result<u64> {
        let short = name.rsplit('.').next().unwrap_or(name);
        match t {
            Type::Result(..) => match short {
                "ok" => Ok(0),
                "err" => Ok(1),
                _ => Err(error("invalid Result pattern")),
            },
            Type::Option(_) => match short {
                "none" => Ok(0),
                "some" => Ok(1),
                _ => Err(error("invalid Option pattern")),
            },
            Type::Named(n) => self
                .program
                .enums
                .iter()
                .find(|e| e.name == *n)
                .and_then(|e| e.variants.iter().position(|v| v.name == short))
                .map(|n| n as u64)
                .ok_or_else(|| error("invalid enum pattern")),
            _ => Err(error("variant pattern on non-enum value")),
        }
    }
    fn pattern_payloads(
        &self,
        ptr: PointerValue<'ctx>,
        t: &Type,
        name: &str,
    ) -> Result<Vec<(PointerValue<'ctx>, Type)>> {
        let tag = self.variant_tag(t, name)?;
        let st = self.ty(t)?.into_struct_type();
        match t {
            Type::Result(ok, err) => {
                let t = if tag == 0 { ok } else { err };
                if **t == Type::Void {
                    return Ok(vec![]);
                }
                Ok(vec![(
                    self.builder.build_struct_gep(st, ptr, 1, "payload")?,
                    *t.clone(),
                )])
            }
            Type::Option(t) => {
                if tag == 0 {
                    Ok(vec![])
                } else {
                    Ok(vec![(
                        self.builder.build_struct_gep(st, ptr, 1, "payload")?,
                        *t.clone(),
                    )])
                }
            }
            Type::Named(n) => {
                let decl = self.program.enums.iter().find(|e| e.name == *n).unwrap();
                let fields = &decl.variants[tag as usize].fields;
                let pt = self.variant_ty(fields)?;
                let p = self.builder.build_struct_gep(st, ptr, 1, "payload")?;
                fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| {
                        Ok((
                            self.builder.build_struct_gep(pt, p, i as u32, "binding")?,
                            f.ty.clone(),
                        ))
                    })
                    .collect()
            }
            _ => Err(error("invalid payload pattern")),
        }
    }
}
