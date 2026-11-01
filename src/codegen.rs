//! Native code generation. Checked operations keep their checks at every optimization level.
use crate::ast::*;
use llvm::{Attribute, AttributeLoc, BasicBlock, Builder, LlvmType, Module, TargetData, Value};
pub use llvm::{Context, FileType, TargetMachine};
use llvm_sys::LLVMLinkage::{
    LLVMAppendingLinkage, LLVMExternalLinkage, LLVMExternalWeakLinkage, LLVMInternalLinkage,
    LLVMPrivateLinkage,
};
use llvm_sys::{
    LLVMAtomicOrdering as AtomicOrdering, LLVMAtomicRMWBinOp as AtomicRMWBinOp,
    LLVMIntPredicate as IntPredicate, LLVMRealPredicate as FloatPredicate,
};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// A `match` with at least this many integer-literal cases in its leading
/// arms is dispatched through one LLVM `switch`.
const MIN_SWITCH_CASES: usize = 3;
/// Integer range patterns spanning fewer values than this are expanded into
/// switch cases; a wider range ends the switched prefix of a match.
const MAX_SWITCH_RANGE: i128 = 256;
/// An interpreter-style dispatch loop needs this many cases for its match to
/// branch through a table of block addresses (see `threaded_dispatch`).
const MIN_DISPATCH_CASES: usize = 16;

mod c_abi;
mod closures;
mod debug;
mod llvm;
mod panic;
use debug::{DebugInfo, SourceMap};

/// Compile LLVM IR text, such as a board runtime, to an object file for the
/// target in `options`. Only code generation runs, never the IR optimization
/// pipeline, so hand-written runtime helpers are not rewritten into calls to
/// the library functions they implement.
pub fn compile_ir_object(options: &Options, name: &str, text: &str, path: &Path) -> Result<()> {
    let context = Context::create();
    let module = Module::parse_ir(&context, name, text)?;
    let machine = target_machine(options)?;
    module.set_triple(&machine.get_triple());
    module.set_data_layout(&machine.get_target_data().get_data_layout());
    module.verify()?;
    function_sections(&module);
    machine.write_to_file(&module, FileType::Object, path)
}

/// One section per defined function, and on ELF per defined global
/// variable, so the linker can drop unused ones.
fn function_sections(module: &Module<'_>) {
    let triple = TargetMachine::normalize_triple(&module.get_triple());
    let triple = triple.as_str().to_string_lossy();
    // Mach-O and Wasm already allow dead stripping individual functions.
    // Their section naming rules (and XCOFF's csects) differ from ELF/COFF.
    if triple.starts_with("wasm")
        || triple.split('-').any(|part| {
            ["darwin", "macos", "ios", "tvos", "watchos", "xros", "aix"]
                .iter()
                .any(|os| part.starts_with(os))
                || part == "macho"
        })
    {
        return;
    }
    let coff = triple.contains("windows") || triple.contains("uefi") || triple.ends_with("-coff");
    let prefix = if coff { ".text$" } else { ".text." };
    // Assign after optimization so generated helpers get their own sections
    // as well, and an unused exported API can be discarded by the linker.
    for function in module.functions() {
        // `@section` placement is the program's choice.
        if function.count_basic_blocks() != 0 && !function.has_section() {
            let name = function.get_name().to_string_lossy();
            function.set_section(Some(&format!("{prefix}{name}")));
            if coff {
                // COFF dead stripping requires COMDAT sections. NoDuplicates
                // preserves the duplicate-definition error for strong exports.
                module.set_noduplicate_comdat(function, &name);
            }
        }
    }
    if coff {
        return;
    }
    let hosted_elf = triple.contains("-linux");
    // Otherwise every constant shares one .rodata section, which a single
    // reference keeps whole: firmware would carry every package's public
    // constants.
    for global in module.globals() {
        let Some(initializer) = global.get_initializer() else {
            continue;
        };
        let name = global.get_name().to_string_lossy();
        // LLVM's own globals, such as llvm.global_ctors, are not data.
        if global.has_section() || global.is_thread_local() || name.starts_with("llvm.") {
            continue;
        }
        let kind = if global.is_global_constant() {
            // Code is position independent, so a constant holding addresses
            // needs load-time relocations and LLVM marks its section
            // writable. GNU ld would merge a writable `.rodata.*` into the
            // text segment and make that writable and executable; hosted
            // programs keep such constants in the RELRO segment instead.
            // Firmware linker scripts place `.rodata.*` in flash.
            if hosted_elf && global.value_type_has_pointer() {
                ".data.rel.ro."
            } else {
                ".rodata."
            }
        } else if initializer.is_null() {
            ".bss."
        } else {
            ".data."
        };
        global.set_section(Some(&format!("{kind}{name}")));
    }
}

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
type Result<T> = std::result::Result<T, CodegenError>;
// Interface method signatures name `Self` only in the receiver, which slots
// always take as an address.
fn mentions_dyn_self(ty: &Type) -> bool {
    matches!(ty, Type::Named(name) if name == "Self")
}

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
    TargetMachine::create(options)
}

pub fn pointer_bits(options: &Options) -> Result<u32> {
    Ok(target_machine(options)?
        .get_target_data()
        .get_pointer_byte_size()
        * 8)
}
/// Declared for reachable `core/cpu` calls the target cannot lower.
const UNSUPPORTED_CPU: &str = "dodo.unsupported.cpu.";
/// Declared for reachable `mem.atomic_*` calls the target cannot lower,
/// suffixed with the operation and its LLVM integer type.
const UNSUPPORTED_ATOMIC: &str = "dodo.unsupported.atomic.";

/// What `mem.atomic_*` lowers to native instructions on a target triple.
#[derive(Clone, Copy, PartialEq)]
enum AtomicSupport {
    /// Loads, stores, and read-modify-write operations.
    Full,
    /// Single-copy atomic loads and stores only (ARMv6-M).
    LoadStore,
    None,
}

fn atomic_support(triple: &str) -> AtomicSupport {
    let arch = triple.split('-').next().unwrap_or_default();
    // LLVM spells Apple's AArch64 as `arm64` (and `arm64e`). ARMv7-M,
    // ARMv7E-M, and ARMv8-M (Cortex-M3, M4, M7, M23, M33, M55, M85) have
    // exclusive loads and stores up to 32 bits; the checker limits widths to
    // the pointer width.
    if ["x86_64", "aarch64", "arm64", "arm64e"].contains(&arch)
        || ["thumbv7m", "thumbv7em", "thumbv8m.", "thumbv8.1m."]
            .iter()
            .any(|prefix| arch.starts_with(prefix))
    {
        AtomicSupport::Full
    } else if arch.starts_with("thumbv6m") {
        AtomicSupport::LoadStore
    } else {
        AtomicSupport::None
    }
}

fn unsupported_atomic(op: &str, triple: &str) -> String {
    if atomic_support(triple) == AtomicSupport::LoadStore {
        format!(
            "atomic `{op}` is not supported for target `{triple}`: ARMv6-M (Cortex-M0 and M0+) has no atomic read-modify-write instructions, only atomic loads and stores; update the value inside a critical section (`hal.Critical`) instead"
        )
    } else {
        format!(
            "atomic integer primitives are not supported for target `{triple}`; no OS or libatomic fallback is inserted"
        )
    }
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
    let bits = machine.get_target_data().get_pointer_byte_size() * 8;
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
        dispatch_loop: None,
        rebind: None,
        yields: vec![],
        function: None,
        return_type: Type::Void,
        return_storage: None,
        indirect: HashMap::new(),
        c_abi: HashMap::new(),
        c_result: None,
        bits,
        sources,
        debug,
        span: Span::default(),
        parameter: 0,
        data: machine.get_target_data(),
        panic: options.panic.clone(),
        test_sources: &options.test_sources,
        testing: !options.test_functions.is_empty(),
        interfaces: crate::sema::flattened_interfaces(program).map_err(|e| error(e.message))?,
        vtables: HashMap::new(),
        fn_pointers: HashMap::new(),
        foreign_signatures: HashMap::new(),
        link_named: HashSet::new(),
        c_definitions: HashSet::new(),
        trampolines: HashMap::new(),
        used: vec![],
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
    cg.emit_used();
    cg.mark_nounwind();
    if let Some(debug) = &cg.debug {
        debug.builder.finalize();
    }
    cg.module.lower_large_aggregates(&cg.data, 1024);
    cg.module
        .verify()
        .map_err(|e| error(format!("LLVM verification failed: {e}")))?;
    // Work from lowered references, including implicit drops, callback addresses,
    // and the hosted/test entry point. Run at O0 too, and again after optimizations
    // that can remove the last reference to an internal function.
    // At O0, fold aggregate load/store pairs before instruction selection.
    // Optimized pipelines already schedule memcpyopt after scalar replacement;
    // running it early there would materialize inactive tagged payload bytes.
    let copies = if options.optimization == 0 {
        "function(memcpyopt),"
    } else {
        ""
    };
    let passes = format!(
        "globaldce,{copies}default<O{}>,globaldce",
        options.optimization.min(3)
    );
    cg.module.run_passes(&passes, &machine)?;
    if let Some(op) = cg.module.functions().find_map(|f| {
        f.get_name()
            .to_str()
            .ok()?
            .strip_prefix(UNSUPPORTED_CPU)
            .map(str::to_owned)
    }) {
        let triple = cg.module.get_triple();
        return Err(error(format!(
            "`cpu.{op}` is not supported for target `{}`; interrupt control is available for bare-metal Cortex-M and RISC-V targets and is a no-op on hosted OS targets",
            triple.as_str().to_string_lossy()
        )));
    }
    if let Some(op) = cg.module.functions().find_map(|f| {
        let name = f
            .get_name()
            .to_str()
            .ok()?
            .strip_prefix(UNSUPPORTED_ATOMIC)?;
        Some(name.split('.').next().unwrap_or(name).to_owned())
    }) {
        let triple = cg.module.get_triple();
        return Err(error(unsupported_atomic(
            &op,
            &triple.as_str().to_string_lossy(),
        )));
    }
    function_sections(&cg.module);
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
    ptr: Value<'ctx>,
    live: Option<Value<'ctx>>,
}
/// An unconditional `for` loop whose body ends with `match value { ... }`.
struct DispatchLoop {
    // Identifies the match; never dereferenced.
    value: *const Expr,
    // The statements of the body before the match.
    prefix: Vec<Stmt>,
    // Index in `scopes` of the loop body's scope.
    body_scope: usize,
}
/// A dispatch loop's match whose arms each end with the next iteration's
/// statements and their own dispatch (see `threaded_dispatch`).
struct ThreadedDispatch<'ctx> {
    prefix: Vec<Stmt>,
    rebind: HashMap<String, Binding<'ctx>>,
}
#[derive(Clone, Copy)]
struct Loop<'ctx> {
    end: BasicBlock<'ctx>,
    next: BasicBlock<'ctx>,
    depth: usize,
}
struct YieldTarget<'ctx> {
    ptr: Value<'ctx>,
    ty: Type,
    end: BasicBlock<'ctx>,
    depth: usize,
}
struct Codegen<'a, 'ctx> {
    context: &'ctx Context,
    // Drop debug metadata and builders before their owning module on errors.
    debug: Option<DebugInfo<'ctx>>,
    builder: Builder<'ctx>,
    module: Module<'ctx>,
    program: &'a Program,
    structs: HashMap<String, LlvmType<'ctx>>,
    functions: HashMap<String, Value<'ctx>>,
    globals: HashMap<String, (Value<'ctx>, Type)>,
    scopes: Vec<Vec<Binding<'ctx>>>,
    loops: Vec<Loop<'ctx>>,
    // The innermost unconditional loop whose body ends with a match.
    dispatch_loop: Option<DispatchLoop>,
    // While a dispatch loop's statements run again at the end of a match arm:
    // the scope depth of their top-level `let`s and the loop-body bindings
    // those store into.
    rebind: Option<(usize, HashMap<String, Binding<'ctx>>)>,
    yields: Vec<YieldTarget<'ctx>>,
    function: Option<Value<'ctx>>,
    return_type: Type,
    return_storage: Option<Value<'ctx>>,
    // Internal calls pass large owned values through caller-owned storage.
    indirect: HashMap<String, (bool, Vec<bool>)>,
    // C signatures that pass or return structs by value, and the coerced
    // result pieces of the C definition being generated.
    c_abi: HashMap<String, c_abi::Lowering<'ctx>>,
    c_result: Option<Vec<LlvmType<'ctx>>>,
    bits: u32,
    sources: SourceMap<'a>,
    span: Span,
    parameter: u32,
    data: TargetData,
    panic: PanicStrategy,
    test_sources: &'a [crate::package::Source],
    testing: bool,
    // Interface methods in vtable order, and the tables built so far.
    interfaces: HashMap<String, Vec<Function>>,
    vtables: HashMap<(String, String), Value<'ctx>>,
    // Function-pointer entry points of named functions and closure bodies,
    // and per-signature adapters from function pointers to closure references.
    fn_pointers: HashMap<String, Value<'ctx>>,
    /// Extern declarations that share a symbol under a different signature
    /// than the module's declaration of it: calls use this function type.
    foreign_signatures: HashMap<String, LlvmType<'ctx>>,
    /// Symbols some declaration named with `@link_name`.
    link_named: HashSet<String>,
    /// Extern symbols this module defines.
    c_definitions: HashSet<String>,
    trampolines: HashMap<String, Value<'ctx>>,
    // Definitions with `@section`, kept through `llvm.used`.
    used: Vec<Value<'ctx>>,
}
impl<'a, 'ctx> Codegen<'a, 'ctx> {
    fn ty(&self, t: &Type) -> Result<LlvmType<'ctx>> {
        Ok(match t {
            Type::Bool => self.context.bool_type(),
            Type::Int { bits, .. } => self
                .context
                .custom_width_int_type(
                    std::num::NonZeroU32::new(if *bits == 0 { self.bits } else { *bits }).unwrap(),
                )
                .unwrap(),
            Type::Float(32) => self.context.f32_type(),
            Type::Float(64) => self.context.f64_type(),
            // A `dyn` reference pairs the value's address with its method table;
            // a closure reference pairs its code with its captured environment.
            Type::Ref(_, t) if matches!(t.as_ref(), Type::Dyn(_) | Type::DynFn(..)) => self
                .context
                .struct_type(&[self.context.ptr_type(0), self.context.ptr_type(0)], false),
            Type::Ref(..) | Type::Raw(..) | Type::Fn(..) | Type::ExternFn(..) => {
                self.context.ptr_type(0)
            }
            Type::Slice(..) | Type::Str => self
                .context
                .struct_type(&[self.context.ptr_type(0), self.usize_type()], false),
            // An opaque single-element array preserves T's ABI size/alignment without
            // exposing an initialized T to the language or invoking its destructor.
            Type::MaybeUninit(t) => self.ty(t)?.array_type(1),
            Type::Array(n, t) => self
                .ty(t)?
                .array_type(u32::try_from(*n).map_err(|_| error("array is too large for LLVM"))?),
            Type::Named(n) => self
                .structs
                .get(n)
                .copied()
                .ok_or_else(|| error(format!("unresolved type {n}")))?,
            Type::Result(t, e) => self.context.struct_type(
                &[
                    self.context.bool_type(),
                    self.union_storage(&[self.payload_ty(t)?, self.payload_ty(e)?])?,
                ],
                false,
            ),
            Type::Option(t) => self
                .context
                .struct_type(&[self.context.bool_type(), self.storage_ty(t)?], false),
            _ => return Err(error(format!("cannot lower type {t}"))),
        })
    }
    fn storage_ty(&self, t: &Type) -> Result<LlvmType<'ctx>> {
        if *t == Type::Void {
            Ok(self.context.i8_type())
        } else {
            self.ty(t)
        }
    }
    fn payload_ty(&self, t: &Type) -> Result<LlvmType<'ctx>> {
        if *t == Type::Void {
            Ok(self.context.struct_type(&[], false))
        } else {
            self.ty(t)
        }
    }
    fn variant_ty(&self, fields: &[Field]) -> Result<LlvmType<'ctx>> {
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
    fn union_storage(&self, alternatives: &[LlvmType<'ctx>]) -> Result<LlvmType<'ctx>> {
        let mut aligner: LlvmType<'ctx> = self.context.i8_type();
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
            .struct_type(&[aligner.array_type(0), bytes], false))
    }
    fn usize_type(&self) -> LlvmType<'ctx> {
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
                        alternatives.push(self.variant_ty(&v.fields)?);
                    }
                    vec![self.context.i32_type(), self.union_storage(&alternatives)?]
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
        ty: LlvmType<'ctx>,
        tag: Value<'ctx>,
        payload: Option<Value<'ctx>>,
    ) -> Result<Value<'ctx>> {
        // Only the tag and active payload are initialized. Inactive bytes are
        // unspecified, so clearing the whole union would add unnecessary stores.
        // Use undef, not poison: aggregate copies may combine inactive bytes with
        // active ones. Keep union_storage's byte representation to preserve every
        // initialized byte, including those in another alternative's padding.
        let value = self
            .builder
            .build_insert_value(ty.get_undef(), tag, 0, "tag")?;
        let Some(payload) = payload else {
            return Ok(value);
        };
        let ptr = self.alloca(ty, "tagged.value")?;
        self.builder.build_store(ptr, value)?;
        let p = self.builder.build_struct_gep(ty, ptr, 1, "payload")?;
        self.store_value(p, payload)?;
        self.builder.build_load(ty, ptr, "tagged")
    }
    fn declare_functions(&mut self) -> Result<()> {
        for f in &self.program.functions {
            if !f.generics.is_empty() {
                continue;
            }
            if let Some(captures) = f.environment {
                let params: Vec<Type> = f.params[captures..].iter().map(|p| p.ty.clone()).collect();
                let convention = self.value_convention(&params, &f.ret)?;
                let ty = self.value_fn_type(&params, &f.ret, true)?;
                let function = self.module.add_function(
                    &format!("dodo.{}.{}", self.program.package, f.name),
                    ty,
                    Some(LLVMInternalLinkage),
                );
                // The environment pointer follows the optional result slot.
                let first = u32::from(convention.0) + 1;
                self.mark_exclusive_params(function, &params, first, &convention.1);
                self.indirect.insert(f.name.clone(), convention);
                self.functions.insert(f.name.clone(), function);
                continue;
            }
            // Keep exported and C signatures stable. Internal aggregate values
            // otherwise flatten into thousands of ABI operands at -O0, making
            // ordinary owned-buffer APIs prohibitively expensive to compile.
            // Interrupt handlers are C functions the vector table names.
            // Root-package public functions use the same Dodo convention:
            // only their linkage differs, and returning a large record by
            // value costs gigabytes of LLVM memory.
            let exported = f.extern_ || f.interrupt.is_some();
            let dodo_abi = !exported && f.name != "main";
            let large = |ty: &Type| -> Result<bool> {
                Ok(dodo_abi && *ty != Type::Void && self.data.get_abi_size(&self.ty(ty)?) > 1024)
            };
            let indirect_return = large(&f.ret)?;
            let indirect_params = f
                .params
                .iter()
                .map(|p| large(&p.ty))
                .collect::<Result<Vec<_>>>()?;
            let mut params = Vec::new();
            if indirect_return {
                params.push(self.context.ptr_type(0));
            }
            for (p, indirect) in f.params.iter().zip(&indirect_params) {
                params.push(if *indirect {
                    self.context.ptr_type(0)
                } else {
                    self.ty(&p.ty)?
                });
            }
            let types: Vec<Type> = f.params.iter().map(|p| p.ty.clone()).collect();
            let lowering = if f.extern_ {
                let short = f.name.rsplit('.').next().unwrap_or(&f.name);
                self.c_lowering(&format!("`{short}`"), &types, &f.ret)?
            } else {
                None
            };
            let ty = if let Some(lowering) = &lowering {
                self.c_fn_type(&types, &f.ret, lowering)?
            } else if f.ret == Type::Void || indirect_return {
                self.context.void_type().fn_type(&params, false)
            } else {
                self.ty(&f.ret)?.fn_type(&params, false)
            };
            self.indirect
                .insert(f.name.clone(), (indirect_return, indirect_params));
            let symbol = if let Some(vector) = &f.interrupt {
                vector.clone()
            } else if f.extern_ {
                f.foreign_symbol().to_owned()
            } else {
                format!("dodo.{}.{}", self.program.package, f.name)
            };
            // `pub` grants source visibility to importers; only the root package
            // exports a Dodo API from this compilation unit. Keep C definitions
            // externally visible even when they originate in an imported package.
            let linkage =
                if exported || f.name == "main" || (!f.imported && f.public && !f.generic_instance)
                {
                    LLVMExternalLinkage
                } else {
                    LLVMInternalLinkage
                };
            let function = if let Some(existing) = self.module.lookup_function(&symbol) {
                // Declarations naming their symbol with @link_name may bind
                // it with their own signature, the way Objective-C casts
                // `objc_msgSend` per method; calls then use that signature.
                // An implicit clash stays an error, since two packages
                // disagreeing about one C function is almost always a bug.
                let aliased = f.link_name.is_some() || self.link_named.contains(&symbol);
                let foreign = f.body.is_none() && !self.c_definitions.contains(&symbol);
                if !f.extern_ || (existing.get_type() != ty && !(aliased && foreign)) {
                    return Err(error(format!(
                        "incompatible declarations of external symbol `{symbol}`"
                    )));
                }
                if existing.get_type() != ty {
                    self.foreign_signatures.insert(f.name.clone(), ty);
                }
                existing
            } else {
                self.module.add_function(&symbol, ty, Some(linkage))
            };
            if let Some(lowering) = lowering {
                self.c_abi.insert(f.name.clone(), lowering);
            }
            if f.link_name.is_some() {
                self.link_named.insert(symbol.clone());
            }
            if f.extern_ && f.body.is_some() {
                self.c_definitions.insert(symbol.clone());
            }
            if exported && !self.foreign_signatures.contains_key(&f.name) {
                for (location, attribute) in self.c_abi_attributes(f)? {
                    function.add_attribute(location, attribute);
                }
            }
            if dodo_abi {
                let params: Vec<Type> = f.params.iter().map(|p| p.ty.clone()).collect();
                let first = u32::from(indirect_return);
                let indirect = &self.indirect[&f.name].1;
                self.mark_exclusive_params(function, &params, first, indirect);
            }
            if f.interrupt.is_some() && self.riscv_bare_metal() {
                // Save every register the handler uses and return with `mret`.
                function.add_attribute(
                    AttributeLoc::Function,
                    self.context.create_string_attribute("interrupt", "machine"),
                );
            }
            self.apply_placement(function, &f.placement, f.body.is_some(), 1)?;
            let inlining: &[&str] = match f.inlining {
                Inlining::Auto => &[],
                Inlining::Always => &["alwaysinline"],
                Inlining::Never => &["noinline"],
                Inlining::Cold => &["cold", "noinline"],
            };
            for name in inlining {
                function.add_attribute(
                    AttributeLoc::Function,
                    self.context
                        .create_enum_attribute(Attribute::get_named_enum_kind_id(name), 0),
                );
            }
            self.functions.insert(f.name.clone(), function);
        }
        Ok(())
    }
    /// Apply `@section` and `@align`. Placed definitions are kept even when
    /// nothing in the program refers to them, like a header the linker script
    /// positions: `llvm.used` survives LLVM's dead-code removal and marks the
    /// section retained against the linker's garbage collection.
    fn apply_placement(
        &mut self,
        value: Value<'ctx>,
        placement: &Placement,
        defined: bool,
        natural: u32,
    ) -> Result<()> {
        if let Some(section) = &placement.section {
            let triple = self.module.get_triple();
            let triple = triple.as_str().to_string_lossy();
            if (triple.contains("apple") || triple.contains("darwin")) && !section.contains(',') {
                return Err(error(format!(
                    "section \"{section}\" needs a Mach-O segment for target `{triple}`: write \"segment,section\", such as \"__DATA,{}\"",
                    section.trim_start_matches('.')
                )));
            }
            value.set_section(Some(section));
            if defined {
                self.used.push(value);
            }
        }
        if let Some(align) = placement.align {
            value.set_alignment(align.max(natural))?;
        }
        Ok(())
    }
    fn riscv_bare_metal(&self) -> bool {
        let triple = self.module.get_triple();
        let triple = triple.as_str().to_string_lossy();
        triple.starts_with("riscv") && (triple.contains("-none") || triple.contains("-unknown-elf"))
    }
    /// Emit `llvm.used` for the definitions `place` retained.
    fn emit_used(&self) {
        if self.used.is_empty() {
            return;
        }
        let pointer = self.context.ptr_type(0);
        let array = pointer.array_type(self.used.len() as u32);
        let global = self.module.add_global(array, None, "llvm.used");
        global.set_initializer(&pointer.const_array(&self.used));
        global.set_linkage(LLVMAppendingLinkage);
        global.set_section(Some("llvm.metadata"));
    }
    fn mark_nounwind(&self) {
        // Dodo never unwinds (traps abort), so neither its functions nor the
        // C, panic-hook, and runtime declarations it calls can throw. Without
        // this, Arm EHABI targets reference `__aeabi_unwind_cpp_pr0`.
        let nounwind = self
            .context
            .create_enum_attribute(Attribute::get_named_enum_kind_id("nounwind"), 0);
        // Keep asynchronous unwind tables for debuggers, profilers, and crash
        // backtraces, except on 32-bit Arm, where EHABI tables for nounwind
        // functions would reintroduce the personality routine reference.
        let triple = self.module.get_triple();
        let triple = triple.as_str().to_string_lossy();
        let ehabi = (triple.starts_with("arm") && !triple.starts_with("arm64"))
            || triple.starts_with("thumb");
        let uwtable = (!ehabi).then(|| {
            self.context
                .create_enum_attribute(Attribute::get_named_enum_kind_id("uwtable"), 2)
        });
        for function in self.module.functions() {
            if !function.get_name().to_bytes().starts_with(b"llvm.") {
                function.add_attribute(AttributeLoc::Function, nounwind);
                if let Some(uwtable) = uwtable.filter(|_| function.count_basic_blocks() != 0) {
                    function.add_attribute(AttributeLoc::Function, uwtable);
                }
            }
        }
    }
    /// Mark `&mut T` parameters `noalias`, as Rust does for `&mut`. For the
    /// duration of the call nothing else accesses the referenced storage:
    /// the flow checker rejects any overlapping loan while an exclusive one
    /// is live, mutable statics and raw-pointer reborrows are unsafe, and
    /// PTR-ALIAS forbids unsafe code from reaching exclusively borrowed
    /// storage by an independent route. Without this, LLVM must assume every
    /// store through a pointer loaded from `self` (such as the elements of a
    /// slice field) may overwrite `*self`, reloading the slice and repeating
    /// its bounds check on each iteration of the loop.
    ///
    /// Only Dodo-convention functions qualify: a C caller makes no such
    /// promise. `&mut dyn` pairs are aggregates LLVM cannot annotate, and
    /// mutable slices pass as `{ptr, len}` values, whose element pointer is
    /// not reloaded from memory in the first place.
    fn mark_exclusive_params(
        &self,
        function: Value<'ctx>,
        params: &[Type],
        first: u32,
        indirect: &[bool],
    ) {
        let noalias = self
            .context
            .create_enum_attribute(Attribute::get_named_enum_kind_id("noalias"), 0);
        for (index, (ty, indirect)) in params.iter().zip(indirect).enumerate() {
            let exclusive = matches!(ty, Type::Ref(true, inner)
                if !matches!(inner.as_ref(), Type::Dyn(_) | Type::DynFn(..)));
            if exclusive && !indirect {
                function.add_attribute(AttributeLoc::Param(first + index as u32), noalias);
            }
        }
    }
    fn c_abi_attributes(
        &self,
        function: &Function,
    ) -> Result<Vec<(AttributeLoc, Attribute<'ctx>)>> {
        let params: Vec<Type> = function.params.iter().map(|p| p.ty.clone()).collect();
        self.c_signature_attributes(&params, &function.ret, self.c_abi.get(&function.name))
    }
    /// Struct-passing and narrow-integer extension attributes of a C
    /// signature, at the IR operand positions of its lowering.
    fn c_signature_attributes(
        &self,
        params: &[Type],
        ret: &Type,
        lowering: Option<&c_abi::Lowering<'ctx>>,
    ) -> Result<Vec<(AttributeLoc, Attribute<'ctx>)>> {
        let mut attributes = match lowering {
            Some(lowering) => self.c_lowering_attributes(params, ret, lowering)?,
            None => Vec::new(),
        };
        let indices = match lowering {
            Some(lowering) => Self::c_operand_indices(lowering),
            None => (0..params.len() as u32).collect(),
        };
        let triple = self.module.get_triple();
        let triple = triple.as_str().to_string_lossy();
        // SysV x86 C promotes narrow arguments/results in registers, and so
        // does Apple's arm64 ABI (unlike AAPCS64, where the callee extends).
        // LLVM needs this promise on both declarations and calls (LangRef
        // parameter attrs).
        let x86 = ["x86_64-", "i386-", "i486-", "i586-", "i686-"]
            .iter()
            .any(|arch| triple.starts_with(arch));
        let apple_arm64 = (triple.starts_with("aarch64-") || triple.starts_with("arm64"))
            && triple.contains("-apple-");
        if !(x86 || apple_arm64) || triple.contains("windows") {
            return Ok(attributes);
        }
        let extensions = std::iter::once((AttributeLoc::Return, ret))
            .chain(
                params
                    .iter()
                    .zip(indices)
                    .map(|(ty, i)| (AttributeLoc::Param(i), ty)),
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
            });
        attributes.extend(extensions);
        Ok(attributes)
    }
    fn declare_constants(&mut self) -> Result<()> {
        let program = self.program;
        for c in &program.constants {
            let val = self.constant(&c.value)?;
            let g = self.module.add_global(
                self.ty(&c.ty)?,
                None,
                &format!("dodo.{}.{}", self.program.package, c.name),
            );
            g.set_initializer(&val);
            // An atomic changes through shared references, so a static that
            // holds one needs writable memory even when it is not `mut`.
            g.set_constant(!c.mutable && !program.holds_interior_mutable(&c.ty));
            let natural = self.data.get_abi_alignment(&self.ty(&c.ty)?);
            self.apply_placement(g, &c.placement, true, natural)?;
            g.set_linkage(if c.public {
                LLVMExternalLinkage
            } else {
                LLVMInternalLinkage
            });
            self.globals.insert(c.name.clone(), (g, c.ty.clone()));
        }
        Ok(())
    }
    fn string_literal(&self, bytes: &[u8]) -> Value<'ctx> {
        let data = self.context.const_string(bytes, false);
        let g = self.module.add_global(data.get_type(), None, "string");
        g.set_initializer(&data);
        g.set_constant(true);
        g.set_linkage(LLVMPrivateLinkage);
        g.set_unnamed_addr(true);
        self.context
            .struct_type(&[self.context.ptr_type(0), self.usize_type()], false)
            .const_named_struct(&[g, self.usize_type().const_int(bytes.len() as u64, false)])
    }
    fn constant(&mut self, e: &Expr) -> Result<Value<'ctx>> {
        match &e.kind {
            ExprKind::String(bytes, _) => Ok(self.string_literal(bytes)),
            ExprKind::Name(n)
                if matches!(e.ty, Type::Fn(..) | Type::ExternFn(..))
                    && self.functions.contains_key(n) =>
            {
                self.fn_pointer(n)
            }
            ExprKind::Name(n) if n == "none" => Ok(self.ty(&e.ty)?.const_zero()),
            ExprKind::Closure(closure) if closure.captures.is_empty() => {
                self.fn_pointer(&closure.function)
            }
            ExprKind::Cast(x, t) if t.closure_signature().is_some() => {
                let pointer = self.constant(x)?;
                let (params, ret) = t.callable_signature().unwrap();
                let trampoline = self.trampoline(params, ret)?;
                Ok(self.ty(t)?.const_named_struct(&[trampoline, pointer]))
            }
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
                if value.is_null() {
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
                Ok(self.ty(&e.ty)?.const_named_struct(&values))
            }
            _ => {
                use crate::consteval::Scalar;
                Ok(match crate::consteval::eval(e, self.bits).map_err(error)? {
                    Scalar::Int(v) => self.ty(&e.ty)?.const_int(v as u64, false),
                    Scalar::Float(v) => self.ty(&e.ty)?.const_float(v),
                    Scalar::Bool(v) => self.context.bool_type().const_int(v as u64, false),
                })
            }
        }
    }
    fn const_array(&self, t: &Type, vs: &[Value<'ctx>]) -> Result<Value<'ctx>> {
        Ok(self.ty(t)?.const_array(vs))
    }

    fn function(&mut self, f: &Function) -> Result<()> {
        let function = self.functions[&f.name];
        self.function = Some(function);
        self.return_type = f.ret.clone();
        let lowering = self.c_abi.get(&f.name).cloned();
        let (mut indirect_return, indirect_params) = self.indirect[&f.name].clone();
        self.c_result = None;
        if let Some(lowering) = &lowering {
            match &lowering.ret {
                c_abi::Pass::Indirect { .. } => indirect_return = true,
                c_abi::Pass::Coerce(pieces) => self.c_result = Some(pieces.clone()),
                c_abi::Pass::Direct => {}
            }
        }
        self.return_storage = if indirect_return {
            function.get_nth_param(0)
        } else {
            None
        };
        self.scopes = vec![vec![]];
        self.loops.clear();
        self.debug_function(f, function)?;
        self.location(f.span);
        self.builder
            .position_at_end(self.context.append_basic_block(function, "entry"));
        let mut incoming = function.get_param_iter().skip(usize::from(indirect_return));
        let captures = f.environment.unwrap_or(0);
        if f.environment.is_some() {
            // A closure body reads its captured bindings' addresses from its environment.
            let environment = incoming.next().unwrap();
            let pointer = self.context.ptr_type(0);
            let layout = self.context.struct_type(&vec![pointer; captures], false);
            for (index, p) in f.params[..captures].iter().enumerate() {
                self.location(p.span);
                let slot = self.builder.build_struct_gep(
                    layout,
                    environment,
                    index as u32,
                    "capture.slot",
                )?;
                let address = self.builder.build_load(pointer, slot, "capture")?;
                self.bind(&p.name, &p.ty, Some(address))?;
            }
        }
        if let Some(lowering) = &lowering {
            let operands: Vec<_> = function.get_param_iter().collect();
            let indices = Self::c_operand_indices(lowering);
            for (index, (p, pass)) in f.params.iter().zip(&lowering.params).enumerate() {
                self.parameter = index as u32 + 1;
                self.location(p.span);
                let first = indices[index] as usize;
                let count = match pass {
                    c_abi::Pass::Coerce(pieces) => pieces.len(),
                    _ => 1,
                };
                let value = self.c_parameter(&operands[first..first + count], &p.ty, pass)?;
                self.bind(&p.name, &p.ty, Some(value))?;
            }
        }
        let incoming = incoming.take(if lowering.is_some() { 0 } else { usize::MAX });
        for (index, (p, v)) in f.params[captures..].iter().zip(incoming).enumerate() {
            self.parameter = index as u32 + 1;
            self.location(p.span);
            let value = if indirect_params[index] {
                self.load(v, &p.ty)?
            } else {
                v
            };
            self.bind(&p.name, &p.ty, Some(value))?;
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
    // Keep large record copies in memory. Letting SROA first split their
    // aggregate loads can build thousands of byte insertions even at O3.
    // Tagged unions are deliberately excluded: copying their inactive payload
    // would defeat the compact constructors used on freestanding targets.
    fn store_value(&self, destination: Value<'ctx>, value: Value<'ctx>) -> Result<()> {
        // A function used as a value is a pointer, not a sized aggregate.
        if value.is_pointer_value() {
            self.builder.build_store(destination, value)?;
            return Ok(());
        }
        let ty = value.get_type();
        let size = self.data.get_abi_size(&ty);
        if size > 1024
            && self
                .program
                .structs
                .iter()
                .any(|s| self.structs.get(&s.name) == Some(&ty))
            && let Some(source) = value.loaded_pointer(&self.builder)
        {
            self.builder.build_memmove(
                destination,
                1,
                source,
                1,
                self.usize_type().const_int(size, false),
            )?;
        } else {
            self.builder.build_store(destination, value)?;
        }
        Ok(())
    }
    fn return_value(&self, value: Value<'ctx>) -> Result<()> {
        if let Some(pieces) = &self.c_result {
            self.c_return(value, pieces)?;
        } else if let Some(storage) = self.return_storage {
            self.store_value(storage, value)?;
            self.builder.build_return(None)?;
        } else {
            self.builder.build_return(Some(&value))?;
        }
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
        if self.module.lookup_function("main").is_some() {
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
            vec![self.context.i32_type(), self.context.ptr_type(0)]
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
                    entry.get_nth_param(0).unwrap(),
                    entry.get_nth_param(1).unwrap(),
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
                        entry.get_nth_param(0).unwrap(),
                        entry.get_nth_param(1).unwrap(),
                    ],
                    "test.index",
                )?
                .basic()
                .unwrap();
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
                let value = call.basic().unwrap_or(self.context.i32_type().const_zero());
                self.builder.build_return(Some(&value))?;
            }
            return Ok(());
        }
        let call = self
            .builder
            .build_call(self.functions["main"], &[], "dodo.main")?;
        let value = call.basic().unwrap_or(self.context.i32_type().const_zero());
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
    fn alloca(&self, t: LlvmType<'ctx>, name: &str) -> Result<Value<'ctx>> {
        let b = self.context.create_builder();
        let entry = self.function.unwrap().get_first_basic_block().unwrap();
        if let Some(i) = entry.get_first_instruction() {
            b.position_before(&i)
        } else {
            b.position_at_end(entry)
        };
        b.build_alloca(t, name)
    }
    fn bind(&mut self, name: &str, ty: &Type, value: Option<Value<'ctx>>) -> Result<Binding<'ctx>> {
        if let Some((depth, bindings)) = &self.rebind
            && *depth == self.scopes.len()
            && let Some(binding) = bindings.get(name)
            && binding.ty == *ty
        {
            // A dispatch loop's `let` running again for the next iteration.
            let binding = binding.clone();
            if let Some(v) = value {
                self.store_value(binding.ptr, v)?;
            }
            return Ok(binding);
        }
        let ptr = self.alloca(self.ty(ty)?, name)?;
        if let Some(v) = value {
            self.store_value(ptr, v)?;
        }
        let live = if self.needs_drop(ty) {
            let flag = self.alloca(self.context.bool_type(), "initialized")?;
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
    fn load(&self, ptr: Value<'ctx>, t: &Type) -> Result<Value<'ctx>> {
        self.builder.build_load(self.ty(t)?, ptr, "value")
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
                .build_load(self.context.bool_type(), flag, "live")?;
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
    fn drop_ptr(&mut self, ptr: Value<'ctx>, ty: &Type) -> Result<()> {
        if !self.needs_drop(ty) {
            return Ok(());
        }
        match ty {
            Type::Named(n) => {
                if let Some(s) = self.program.structs.iter().find(|s| s.name == *n).cloned() {
                    if let Some(f) = self.functions.get(&format!("{n}.drop")).copied() {
                        self.builder.build_call(f, &[ptr], "")?;
                    }
                    for (i, field) in s.fields.iter().enumerate().rev() {
                        let p = self.builder.build_struct_gep(
                            self.ty(ty)?,
                            ptr,
                            i as u32,
                            "drop.field",
                        )?;
                        self.drop_ptr(p, &field.ty)?;
                    }
                } else if let Some(e) = self.program.enums.iter().find(|e| e.name == *n).cloned() {
                    let st = self.ty(ty)?;
                    let tagptr = self.builder.build_struct_gep(st, ptr, 0, "tag")?;
                    let tag = self
                        .builder
                        .build_load(self.context.i32_type(), tagptr, "tag")?;
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
                let st = self.ty(ty)?;
                let tagptr = self.builder.build_struct_gep(st, ptr, 0, "tag")?;
                let tag = self
                    .builder
                    .build_load(self.context.bool_type(), tagptr, "tag")?;
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
                let st = self.ty(ty)?;
                let tp = self.builder.build_struct_gep(st, ptr, 0, "tag")?;
                let tag = self
                    .builder
                    .build_load(self.context.bool_type(), tp, "tag")?;
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
                if self.needs_drop(&e.ty) {
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
                    self.return_value(v)?;
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
                let c = self.expr(condition)?;
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
                    let v = self.expr(c)?;
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
                let dispatch = match body.last() {
                    Some(Stmt {
                        kind: StmtKind::Match { value, .. },
                        ..
                    }) if init.is_none() && condition.is_none() && step.is_none() => {
                        Some(DispatchLoop {
                            value: &raw const *value,
                            prefix: body[..body.len() - 1].to_vec(),
                            body_scope: self.scopes.len(),
                        })
                    }
                    _ => None,
                };
                let outer = std::mem::replace(&mut self.dispatch_loop, dispatch);
                let result = self.block(body);
                self.dispatch_loop = outer;
                result?;
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
    fn stage(&mut self, expression: &Expr, pending: &mut Vec<Value<'ctx>>) -> Result<Value<'ctx>> {
        let value = self.expr(expression)?;
        if self.needs_drop(&expression.ty) {
            let binding = self.bind("$pending", &expression.ty, Some(value))?;
            if let Some(flag) = binding.live {
                pending.push(flag);
            }
        }
        Ok(value)
    }
    fn transfer(&self, pending: &[Value<'ctx>]) -> Result<()> {
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
        storage: Value<'ctx>,
        ty: &Type,
        value: Value<'ctx>,
    ) -> Result<()> {
        let Type::Array(n, element) = ty else {
            return Err(error("invalid repeated array"));
        };
        let counter = self.alloca(self.usize_type(), "repeat.index")?;
        self.builder
            .build_store(counter, self.usize_type().const_zero())?;
        let head = self.bb("repeat.condition");
        let body = self.bb("repeat.body");
        let end = self.bb("repeat.end");
        self.builder.build_unconditional_branch(head)?;
        self.builder.position_at_end(head);
        let index = self
            .builder
            .build_load(self.usize_type(), counter, "repeat.index")?;
        let more = self.builder.build_int_compare(
            IntPredicate::LLVMIntULT,
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
    fn expr(&mut self, e: &Expr) -> Result<Value<'ctx>> {
        let previous = self.span;
        self.location(e.span);
        let result = self.expr_inner(e);
        self.location(previous);
        result
    }
    fn expr_inner(&mut self, e: &Expr) -> Result<Value<'ctx>> {
        let value = match &e.kind {
            ExprKind::Int(v, _) => self.ty(&e.ty)?.const_int(*v, false),
            ExprKind::Float(v, _) => self.ty(&e.ty)?.const_float(*v),
            ExprKind::Bool(v) => self.context.bool_type().const_int(*v as u64, false),
            ExprKind::String(bytes, _) => self.string_literal(bytes),
            ExprKind::Name(n) => {
                if n == "none" {
                    self.ty(&e.ty)?.const_zero()
                } else if let Some(b) = self.binding(n) {
                    self.load(b.ptr, &b.ty)?
                } else if let Some((p, t)) = self.globals.get(n).cloned() {
                    self.load(p, &t)?
                } else if self.functions.contains_key(n) {
                    self.fn_pointer(n)?
                } else {
                    return Err(error(format!("unknown value {n}")));
                }
            }
            ExprKind::Constant(value, _) => self.expr(value)?,
            ExprKind::Repeat(value, _) => {
                let value = self.expr(value)?;
                if value.is_null() {
                    // Returning a zero-filled array (also inside a struct)
                    // should start as one aggregate zero. A fill loop followed
                    // by an aggregate load makes GVN build a fresh array
                    // constant for each element, retaining quadratic memory.
                    self.ty(&e.ty)?.const_zero()
                } else {
                    let storage = self.alloca(self.ty(&e.ty)?, "repeat.array")?;
                    self.initialize_repeat(storage, &e.ty, value)?;
                    self.load(storage, &e.ty)?
                }
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
                    self.context.i8_type().const_zero()
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
                    self.cast(v, &e.ty, &Type::usize())?
                } else {
                    self.usize_type().const_zero()
                };
                let end = if let Some(e) = end {
                    let v = self.expr(e)?;
                    self.cast(v, &e.ty, &Type::usize())?
                } else {
                    length
                };
                let ordered = self.builder.build_int_compare(
                    IntPredicate::LLVMIntULE,
                    start,
                    end,
                    "slice.ordered",
                )?;
                let inside = self.builder.build_int_compare(
                    IntPredicate::LLVMIntULE,
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
                let slice = self.ty(&e.ty)?.const_zero();
                let slice = self
                    .builder
                    .build_insert_value(slice, data, 0, "slice.ptr")?;
                self.builder
                    .build_insert_value(slice, length, 1, "slice.len")?
            }
            ExprKind::Range(..) => return Err(error("unresolved range reached code generation")),
            ExprKind::Array(_, xs) => {
                let mut array = self.ty(&e.ty)?.const_zero();
                let mut pending = Vec::new();
                for (i, x) in xs.iter().enumerate() {
                    let v = self.stage(x, &mut pending)?;
                    array = self
                        .builder
                        .build_insert_value(array, v, i as u32, "array")?;
                }
                self.transfer(&pending)?;
                array
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
                // Initialize large records field by field. Building one SSA
                // aggregate with dynamic metadata and large zero buffers makes
                // instruction selection expand the buffers into scalar stores.
                let storage = if self.data.get_abi_size(&st) > 256 {
                    Some(self.alloca(st, "struct.storage")?)
                } else {
                    None
                };
                let mut v = st.const_zero();
                let mut pending = Vec::new();
                for (name, e) in fields {
                    let i = decl
                        .fields
                        .iter()
                        .position(|f| f.name == *name)
                        .ok_or_else(|| error("unknown field"))?;
                    let x = self.stage(e, &mut pending)?;
                    if let Some(storage) = storage {
                        let field =
                            self.builder
                                .build_struct_gep(st, storage, i as u32, "struct.field")?;
                        self.store_value(field, x)?;
                    } else {
                        v = self.builder.build_insert_value(v, x, i as u32, "struct")?;
                    }
                }
                self.transfer(&pending)?;
                if let Some(storage) = storage {
                    self.builder.build_load(st, storage, "struct.value")?
                } else {
                    v
                }
            }
            ExprKind::Unary(op, x) => match op {
                // `&*r` and `&mut *r` reborrow a `dyn` reference: the same
                // address and method table.
                UnaryOp::Borrow | UnaryOp::BorrowMut if matches!(&e.ty, Type::Ref(_, t) if matches!(t.as_ref(), Type::Dyn(_))) =>
                {
                    let ExprKind::Unary(UnaryOp::Deref, reference) = &x.kind else {
                        return Err(error("a `dyn` value can only be reborrowed"));
                    };
                    self.expr(reference)?
                }
                UnaryOp::Borrow | UnaryOp::BorrowMut => {
                    let ptr = self.place(x)?;
                    if let Type::Slice(_, _) = &e.ty {
                        match &x.ty {
                            Type::Array(n, _) => {
                                let mut v = self.ty(&e.ty)?.const_zero();
                                v = self.builder.build_insert_value(v, ptr, 0, "slice.ptr")?;
                                self.builder.build_insert_value(
                                    v,
                                    self.usize_type().const_int(*n as u64, false),
                                    1,
                                    "slice.len",
                                )?
                            }
                            _ => self.load(ptr, &e.ty)?,
                        }
                    } else {
                        ptr
                    }
                }
                UnaryOp::Deref => {
                    let p = self.expr(x)?;
                    self.load(p, &e.ty)?
                }
                UnaryOp::Neg => {
                    if let ExprKind::Int(v, _) = x.kind {
                        self.ty(&e.ty)?.const_int(v, false).const_neg()
                    } else {
                        let v = self.expr(x)?;
                        if e.ty.is_integer() {
                            self.binary(BinaryOp::Sub, self.ty(&e.ty)?.const_zero(), v, &e.ty)?
                        } else {
                            self.builder.build_float_neg(v, "neg")?
                        }
                    }
                }
                UnaryOp::Not | UnaryOp::BitNot => {
                    let v = self.expr(x)?;
                    self.builder.build_not(v, "not")?
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
                    return self.tagged_value(
                        self.ty(&e.ty)?,
                        self.context.i32_type().const_int(idx as u64, false),
                        None,
                    );
                }
                if n == "len" {
                    self.collection(x)?.1
                } else {
                    let p = self.place(e)?;
                    self.load(p, &e.ty)?
                }
            }
            ExprKind::Index(..) => {
                let p = self.place(e)?;
                self.load(p, &e.ty)?
            }
            ExprKind::Closure(closure) => self.closure(closure, &e.ty)?,
            ExprKind::Invoke { callee, args } => self.invoke(callee, args, &e.ty)?,
            ExprKind::Asm(asm) => self.inline_asm(asm, &e.ty)?,
            ExprKind::Cast(x, t) if t.closure_signature().is_some() => match &x.kind {
                // A closure without captures needs no environment.
                ExprKind::Closure(closure) if closure.captures.is_empty() => {
                    let code = self.functions[&closure.function];
                    self.closure_value(code, self.context.ptr_type(0).const_zero())?
                }
                _ if matches!(x.ty, Type::Fn(..)) => {
                    let pointer = self.expr(x)?;
                    let (params, ret) = t.callable_signature().unwrap();
                    let trampoline = self.trampoline(params, ret)?;
                    self.closure_value(trampoline, pointer)?
                }
                _ => self.expr(x)?,
            },
            ExprKind::Cast(x, t)
                if let Type::Ref(_, object) = t
                    && let Type::Dyn(interface) = object.as_ref() =>
            {
                let v = self.expr(x)?;
                match &x.ty {
                    Type::Ref(_, inner) if matches!(inner.as_ref(), Type::Dyn(_)) => v,
                    Type::Ref(_, inner) => {
                        let table = self.vtable(inner, interface)?;
                        let fat = self.ty(t)?.get_undef();
                        let fat = self.builder.build_insert_value(fat, v, 0, "dyn.data")?;
                        self.builder
                            .build_insert_value(fat, table, 1, "dyn.table")?
                    }
                    other => return Err(error(format!("cannot convert `{other}` to `{t}`"))),
                }
            }
            ExprKind::Cast(x, t) => {
                let v = self.expr(x)?;
                self.cast(v, &x.ty, t)?
            }
            ExprKind::Try(x) | ExprKind::Unwrap(x) => {
                let propagate = matches!(e.kind, ExprKind::Try(_));
                let operation = if propagate { "propagate" } else { "unwrap" };
                let v = self.expr(x)?;
                let Type::Result(success, failure) = &x.ty else {
                    return Err(error(format!("{operation} requires a Result")));
                };
                let st = v.get_type();
                let ptr = self.alloca(st, &format!("{operation}.value"))?;
                self.builder.build_store(ptr, v)?;
                let p = self.builder.build_struct_gep(st, ptr, 1, "payload")?;
                let tag = self.builder.build_extract_value(v, 0, "is.error")?;
                let err = self.bb(&format!("{operation}.error"));
                let ok = self.bb(&format!("{operation}.ok"));
                self.builder.build_conditional_branch(tag, err, ok)?;
                self.builder.position_at_end(err);
                if propagate {
                    let mut payload = if **failure == Type::Void {
                        None
                    } else {
                        Some(self.load(p, failure)?)
                    };
                    if let Type::Result(_, target) = &self.return_type
                        && **target != **failure
                    {
                        payload = Some(self.wrap_error(target, failure, payload)?);
                    }
                    let ret = self.tagged_value(
                        self.ty(&self.return_type)?,
                        self.context.bool_type().const_int(1, false),
                        payload,
                    )?;
                    self.cleanup_to(0)?;
                    self.return_value(ret)?;
                } else {
                    self.panic("result unwrap")?;
                }
                self.builder.position_at_end(ok);
                if **success == Type::Void {
                    self.context.i8_type().const_zero()
                } else {
                    self.load(p, success)?
                }
            }
        };
        self.consume(e)?;
        Ok(value)
    }
    fn place(&mut self, e: &Expr) -> Result<Value<'ctx>> {
        let previous = self.span;
        self.location(e.span);
        let result = self.place_inner(e);
        self.location(previous);
        result
    }
    fn place_inner(&mut self, e: &Expr) -> Result<Value<'ctx>> {
        match &e.kind {
            ExprKind::Name(n) => self
                .binding(n)
                .map(|b| b.ptr)
                .or_else(|| self.globals.get(n).map(|g| g.0))
                .ok_or_else(|| error(format!("{n} has no address"))),
            ExprKind::Unary(UnaryOp::Deref, x) => Ok(self.expr(x)?),
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
                Ok(self
                    .builder
                    .build_struct_gep(self.ty(&t)?, p, i as u32, "field.ptr")?)
            }
            ExprKind::Index(x, i) => {
                let (ptr, len, t) = self.collection(x)?;
                let idx = self.expr(i)?;
                let wide = self.cast(idx, &i.ty, &Type::usize())?;
                let in_bounds = self.builder.build_int_compare(
                    IntPredicate::LLVMIntULT,
                    wide,
                    len,
                    "in.bounds",
                )?;
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
    fn autoderef_place(&mut self, e: &Expr) -> Result<(Value<'ctx>, Type)> {
        let mut ty = e.ty.clone();
        let mut p = self.place(e)?;
        while let Type::Ref(_, t) = ty {
            p = self.load(p, &Type::Ref(false, t.clone()))?;
            ty = *t;
        }
        Ok((p, ty))
    }
    fn collection(&mut self, e: &Expr) -> Result<(Value<'ctx>, Value<'ctx>, Type)> {
        match &e.ty {
            Type::Array(n, t) => Ok((
                self.place(e)?,
                self.usize_type().const_int(*n as u64, false),
                *t.clone(),
            )),
            Type::Ref(_, t) => {
                let p = self.expr(e)?;
                match &**t {
                    Type::Array(n, t) => {
                        Ok((p, self.usize_type().const_int(*n as u64, false), *t.clone()))
                    }
                    _ => Err(error("reference is not a collection")),
                }
            }
            Type::Slice(_, t) => {
                let v = self.expr(e)?;
                Ok((
                    self.builder.build_extract_value(v, 0, "data")?,
                    self.builder.build_extract_value(v, 1, "len")?,
                    *t.clone(),
                ))
            }
            Type::Str => {
                let v = self.expr(e)?;
                Ok((
                    self.builder.build_extract_value(v, 0, "data")?,
                    self.builder.build_extract_value(v, 1, "len")?,
                    Type::u8(),
                ))
            }
            _ => Err(error(format!("{} is not an array or slice", e.ty))),
        }
    }
    fn array_gep(&self, p: Value<'ctx>, t: &Type, i: Value<'ctx>) -> Result<Value<'ctx>> {
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
    fn guard(&mut self, valid: Value<'ctx>, check: &str, reason: &str) -> Result<()> {
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
        let trap = self
            .module
            .intrinsic("llvm.trap", &[])
            .ok_or_else(|| error("LLVM trap intrinsic is unavailable"))?;
        self.builder.build_call(trap, &[], "")?;
        self.builder.build_unreachable()?;
        Ok(())
    }
    fn test_runtime_call(&self, name: &str, args: &[Value<'ctx>]) -> Result<()> {
        let types: Vec<_> = args.iter().map(|v| v.get_type()).collect();
        let ty = self.context.void_type().fn_type(&types, false);
        let function = self
            .module
            .lookup_function(name)
            .unwrap_or_else(|| self.module.add_function(name, ty, None));
        if function.get_type() != ty {
            return Err(error(format!("{name} is reserved for the test runtime")));
        }
        let args: Vec<_> = args.to_vec();
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
                text,
                self.usize_type().const_int(report.len() as u64, false),
            ],
        )
    }
    fn report_value(&self, label: u64, value: Value<'ctx>, ty: &Type) -> Result<()> {
        if !self.testing {
            return Ok(());
        }
        let label = self.context.i32_type().const_int(label, false);
        match ty {
            Type::Bool => {
                let value =
                    self.builder
                        .build_int_z_extend(value, self.context.i32_type(), "test.bool")?;
                self.test_runtime_call("dodo_test_bool", &[label, value])
            }
            Type::Str => {
                let ptr = self.builder.build_extract_value(value, 0, "text.ptr")?;
                let len = self.builder.build_extract_value(value, 1, "text.len")?;
                self.test_runtime_call("dodo_test_text", &[label, ptr, len])
            }
            Type::Float(_) => {
                let value =
                    self.builder
                        .build_float_cast(value, self.context.f64_type(), "test.float")?;
                self.test_runtime_call("dodo_test_float", &[label, value])
            }
            _ => {
                let signed = matches!(ty, Type::Int { signed: true, .. });
                let value = if value.is_pointer_value() {
                    self.builder
                        .build_ptr_to_int(value, self.context.i64_type(), "test.address")?
                } else if value.is_struct_value() {
                    self.builder.build_extract_value(value, 0, "test.tag")?
                } else {
                    value
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
                    &[label, value],
                )
            }
        }
    }
    /// String assertions compare bytes, including embedded NULs, without adding
    /// a libc dependency to ordinary freestanding assertion users.
    fn strings_equal(&self, a: Value<'ctx>, b: Value<'ctx>) -> Result<Value<'ctx>> {
        let ap = self.builder.build_extract_value(a, 0, "left.ptr")?;
        let bp = self.builder.build_extract_value(b, 0, "right.ptr")?;
        let al = self.builder.build_extract_value(a, 1, "left.len")?;
        let bl = self.builder.build_extract_value(b, 1, "right.len")?;
        let result = self.alloca(self.context.bool_type(), "strings.equal")?;
        let index = self.alloca(self.usize_type(), "strings.index")?;
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
                .build_int_compare(IntPredicate::LLVMIntEQ, al, bl, "same.length")?;
        self.builder
            .build_conditional_branch(same_length, head, end)?;
        self.builder.position_at_end(head);
        let i = self.builder.build_load(self.usize_type(), index, "index")?;
        let done = self
            .builder
            .build_int_compare(IntPredicate::LLVMIntEQ, i, al, "done")?;
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
            .build_load(self.context.i8_type(), ap, "left")?;
        let bv = self
            .builder
            .build_load(self.context.i8_type(), bp, "right")?;
        let same = self
            .builder
            .build_int_compare(IntPredicate::LLVMIntEQ, av, bv, "same.byte")?;
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
        self.builder
            .build_load(self.context.bool_type(), result, "equal")
    }
    fn short_circuit(&mut self, op: BinaryOp, l: &Expr, r: &Expr) -> Result<Value<'ctx>> {
        let a = self.expr(l)?;
        let left = self.builder.get_insert_block().unwrap();
        let rhs = self.bb("logic.rhs");
        let end = self.bb("logic.end");
        if op == BinaryOp::And {
            self.builder.build_conditional_branch(a, rhs, end)?;
        } else {
            self.builder.build_conditional_branch(a, end, rhs)?;
        }
        self.builder.position_at_end(rhs);
        let b = self.expr(r)?;
        let right = self.builder.get_insert_block().unwrap();
        self.builder.build_unconditional_branch(end)?;
        self.builder.position_at_end(end);
        let phi = self.builder.build_phi(self.context.bool_type(), "logic")?;
        phi.add_incoming(&[(&a, left), (&b, right)]);
        Ok(phi)
    }
    fn binary(
        &mut self,
        op: BinaryOp,
        a: Value<'ctx>,
        b: Value<'ctx>,
        t: &Type,
    ) -> Result<Value<'ctx>> {
        use BinaryOp::*;
        if matches!(t, Type::Float(_)) {
            return Ok(match op {
                Add => self.builder.build_float_add(a, b, "add")?,
                Sub => self.builder.build_float_sub(a, b, "sub")?,
                Mul => self.builder.build_float_mul(a, b, "mul")?,
                Div => self.builder.build_float_div(a, b, "div")?,
                Rem => self.builder.build_float_rem(a, b, "rem")?,
                Eq | Ne | Lt | Le | Gt | Ge => self.builder.build_float_compare(
                    match op {
                        Eq => FloatPredicate::LLVMRealOEQ,
                        Ne => FloatPredicate::LLVMRealUNE,
                        Lt => FloatPredicate::LLVMRealOLT,
                        Le => FloatPredicate::LLVMRealOLE,
                        Gt => FloatPredicate::LLVMRealOGT,
                        _ => FloatPredicate::LLVMRealOGE,
                    },
                    a,
                    b,
                    "compare",
                )?,
                _ => return Err(error("invalid floating point operation")),
            });
        }
        let (a, b) = if a.is_struct_value() {
            (
                self.builder.build_extract_value(a, 0, "tag")?,
                self.builder.build_extract_value(b, 0, "tag")?,
            )
        } else if a.is_pointer_value() {
            let ai = self
                .builder
                .build_ptr_to_int(a, self.usize_type(), "address")?;
            let bi = self
                .builder
                .build_ptr_to_int(b, self.usize_type(), "address")?;
            (ai, bi)
        } else {
            (a, b)
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
                let intrinsic = self
                    .module
                    .intrinsic(&name, &[ity])
                    .ok_or_else(|| error("overflow intrinsic unavailable"))?;
                let out = self
                    .builder
                    .build_call(intrinsic, &[a, b], "checked.arithmetic")?
                    .basic()
                    .unwrap();
                let overflow = self.builder.build_extract_value(out, 1, "overflow")?;
                let valid = self.builder.build_not(overflow, "no.overflow")?;
                self.guard(valid, "arithmetic overflow", "integer overflow")?;
                self.builder.build_extract_value(out, 0, "result")?
            }
            Div | Rem => {
                let nz = self.builder.build_int_compare(
                    IntPredicate::LLVMIntNE,
                    b,
                    ity.const_zero(),
                    "nonzero",
                )?;
                self.guard(nz, "division by zero", "division or remainder by zero")?;
                if signed {
                    let min = ity.const_int(1u64 << (ity.get_bit_width() - 1), false);
                    let a_min = self.builder.build_int_compare(
                        IntPredicate::LLVMIntEQ,
                        a,
                        min,
                        "is.min",
                    )?;
                    let b_neg = self.builder.build_int_compare(
                        IntPredicate::LLVMIntEQ,
                        b,
                        ity.const_all_ones(),
                        "is.neg.one",
                    )?;
                    let bad = self.builder.build_and(a_min, b_neg, "division.overflow")?;
                    let valid = self.builder.build_not(bad, "division.valid")?;
                    self.guard(valid, "division overflow", "signed division overflow")?;
                }
                match (op, signed) {
                    (Div, true) => self.builder.build_int_signed_div(a, b, "div")?,
                    (Div, false) => self.builder.build_int_unsigned_div(a, b, "div")?,
                    (_, true) => self.builder.build_int_signed_rem(a, b, "rem")?,
                    _ => self.builder.build_int_unsigned_rem(a, b, "rem")?,
                }
            }
            Eq | Ne | Lt | Le | Gt | Ge => self.builder.build_int_compare(
                match op {
                    Eq => IntPredicate::LLVMIntEQ,
                    Ne => IntPredicate::LLVMIntNE,
                    Lt => {
                        if signed {
                            IntPredicate::LLVMIntSLT
                        } else {
                            IntPredicate::LLVMIntULT
                        }
                    }
                    Le => {
                        if signed {
                            IntPredicate::LLVMIntSLE
                        } else {
                            IntPredicate::LLVMIntULE
                        }
                    }
                    Gt => {
                        if signed {
                            IntPredicate::LLVMIntSGT
                        } else {
                            IntPredicate::LLVMIntUGT
                        }
                    }
                    _ => {
                        if signed {
                            IntPredicate::LLVMIntSGE
                        } else {
                            IntPredicate::LLVMIntUGE
                        }
                    }
                },
                a,
                b,
                "compare",
            )?,
            BitAnd => self.builder.build_and(a, b, "and")?,
            BitOr => self.builder.build_or(a, b, "or")?,
            BitXor => self.builder.build_xor(a, b, "xor")?,
            Shl | Shr => {
                let valid = self.builder.build_int_compare(
                    IntPredicate::LLVMIntULT,
                    b,
                    ity.const_int(ity.get_bit_width() as u64, false),
                    "shift.valid",
                )?;
                self.guard(valid, "shift amount", "shift count out of range")?;
                if op == Shl {
                    let v = self.builder.build_left_shift(a, b, "shl")?;
                    let back = self.builder.build_right_shift(v, b, signed, "shift.back")?;
                    let fits = self.builder.build_int_compare(
                        IntPredicate::LLVMIntEQ,
                        back,
                        a,
                        "shift.fits",
                    )?;
                    self.guard(fits, "shift overflow", "left shift overflow")?;
                    v
                } else {
                    self.builder.build_right_shift(a, b, signed, "shr")?
                }
            }
            And | Or => return Err(error("logical operator must use short-circuit lowering")),
        })
    }
    fn cast(&mut self, v: Value<'ctx>, from: &Type, to: &Type) -> Result<Value<'ctx>> {
        match (from, to) {
            (Type::Int { signed: fs, .. }, Type::Int { signed: ts, .. }) => {
                let out = self.ty(to)?;
                let source = v.get_type();
                if *fs && !*ts {
                    let nonnegative = self.builder.build_int_compare(
                        IntPredicate::LLVMIntSGE,
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
                    let fits = self.builder.build_int_compare(
                        IntPredicate::LLVMIntEQ,
                        back,
                        v,
                        "cast.fits",
                    )?;
                    self.guard(
                        fits,
                        "numeric conversion",
                        "integer conversion out of range",
                    )?;
                }
                if !*fs && *ts && out.get_bit_width() <= source.get_bit_width() {
                    let max = (1u64 << (out.get_bit_width() - 1)) - 1;
                    let fits = self.builder.build_int_compare(
                        IntPredicate::LLVMIntULE,
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
                Ok(converted)
            }
            (Type::Int { signed, .. }, Type::Float(_)) => {
                let t = self.ty(to)?;
                Ok(if *signed {
                    self.builder.build_signed_int_to_float(v, t, "cast")?
                } else {
                    self.builder.build_unsigned_int_to_float(v, t, "cast")?
                })
            }
            (Type::Float(_), Type::Int { signed, .. }) => {
                let t = self.ty(to)?;
                let bits = t.get_bit_width();
                let low = if *signed {
                    -(2f64).powi(bits as i32 - 1)
                } else {
                    0.
                };
                let high = (2f64).powi(bits as i32 - if *signed { 1 } else { 0 });
                let lo = self.builder.build_float_compare(
                    FloatPredicate::LLVMRealOGE,
                    v,
                    v.get_type().const_float(low),
                    "cast.lower",
                )?;
                let hi = self.builder.build_float_compare(
                    FloatPredicate::LLVMRealOLT,
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
                    self.builder.build_float_to_signed_int(v, t, "cast")?
                } else {
                    self.builder.build_float_to_unsigned_int(v, t, "cast")?
                })
            }
            (Type::Float(_), Type::Float(_)) => {
                let source = v;
                let target = self.ty(to)?;
                let out = self.builder.build_float_cast(source, target, "cast")?;
                if target.get_bit_width() < source.get_type().get_bit_width() {
                    let max = source.get_type().const_float(f32::MAX as f64);
                    let min = source.get_type().const_float(-(f32::MAX as f64));
                    let hi = self.builder.build_float_compare(
                        FloatPredicate::LLVMRealOLE,
                        source,
                        max,
                        "cast.upper",
                    )?;
                    let lo = self.builder.build_float_compare(
                        FloatPredicate::LLVMRealOGE,
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
                Ok(out)
            }
            (Type::Raw(..) | Type::Ref(..), Type::Raw(..) | Type::Ref(..)) => Ok(v),
            (Type::Raw(..) | Type::ExternFn(..), Type::Raw(..) | Type::ExternFn(..)) => Ok(v),
            (Type::Int { .. }, Type::ExternFn(..)) => {
                Ok(self
                    .builder
                    .build_int_to_ptr(v, self.context.ptr_type(0), "code")?)
            }
            (Type::ExternFn(..), Type::Int { .. }) => {
                Ok(self.builder.build_ptr_to_int(v, self.ty(to)?, "address")?)
            }
            (Type::Int { .. }, Type::Raw(..)) => {
                Ok(self
                    .builder
                    .build_int_to_ptr(v, self.context.ptr_type(0), "pointer")?)
            }
            (Type::Raw(..), Type::Int { .. }) => {
                Ok(self.builder.build_ptr_to_int(v, self.ty(to)?, "address")?)
            }
            _ if from == to => Ok(v),
            _ => Err(error(format!(
                "cannot lower conversion from {from} to {to}"
            ))),
        }
    }
    /// Lowers `asm(...)`. LLVM numbers outputs first, then `in` operands in
    /// source order, then the inputs tied to `inout` outputs, which the template
    /// cannot name separately. `out` operands are early-clobber: the template
    /// may write them before it has read every input.
    fn inline_asm(&mut self, asm: &InlineAsm, ty: &Type) -> Result<Value<'ctx>> {
        let indices = crate::sema::asm::llvm_operand_indices(asm);
        let template = crate::sema::asm::llvm_template(&asm.template, &indices).map_err(error)?;
        let register = |register: &AsmRegister| match register {
            AsmRegister::Any => "r".to_owned(),
            AsmRegister::Named(name) => format!("{{{}}}", name.to_ascii_lowercase()),
        };
        // Evaluate inputs in source order, whatever their LLVM position.
        let mut values = Vec::with_capacity(asm.operands.len());
        for operand in &asm.operands {
            values.push(match &operand.kind {
                AsmOperandKind::In(value) | AsmOperandKind::InOut(value) => Some(self.expr(value)?),
                AsmOperandKind::Out(_) => None,
            });
        }
        let mut constraints = vec![];
        let mut outputs = vec![];
        for operand in &asm.operands {
            match &operand.kind {
                AsmOperandKind::Out(ty) => {
                    constraints.push(format!("=&{}", register(&operand.register)));
                    outputs.push(self.ty(ty)?);
                }
                AsmOperandKind::InOut(value) => {
                    constraints.push(format!("={}", register(&operand.register)));
                    outputs.push(self.ty(&value.ty)?);
                }
                AsmOperandKind::In(_) => {}
            }
        }
        let mut args = vec![];
        for (operand, value) in asm.operands.iter().zip(&values) {
            if let (AsmOperandKind::In(_), Some(value)) = (&operand.kind, value) {
                constraints.push(register(&operand.register));
                args.push(*value);
            }
        }
        let mut output = 0;
        for (operand, value) in asm.operands.iter().zip(&values) {
            match (&operand.kind, value) {
                (AsmOperandKind::InOut(_), Some(value)) => {
                    constraints.push(output.to_string());
                    args.push(*value);
                    output += 1;
                }
                (AsmOperandKind::Out(_), _) => output += 1,
                _ => {}
            }
        }
        for clobber in &asm.clobbers {
            constraints.push(format!("~{{{}}}", clobber.to_ascii_lowercase()));
        }
        if !asm.nomem
            && !asm
                .clobbers
                .iter()
                .any(|c| c.eq_ignore_ascii_case("memory"))
        {
            constraints.push("~{memory}".into());
        }
        // Templates may change the condition flags, as GCC and Clang assume.
        let triple = self.module.get_triple();
        let arch = triple.as_str().to_string_lossy();
        let arch = arch.split('-').next().unwrap_or_default();
        if arch.starts_with("x86") || (arch.starts_with('i') && arch.ends_with("86")) {
            constraints.extend(["~{dirflag}", "~{fpsr}", "~{flags}"].map(str::to_owned));
        } else if ["arm", "thumb", "aarch64", "arm64"]
            .iter()
            .any(|prefix| arch.starts_with(prefix))
        {
            constraints.push("~{cc}".into());
        }
        let parameters: Vec<_> = args.iter().map(|value| value.get_type()).collect();
        let result = match outputs.as_slice() {
            [] => self.context.void_type(),
            [single] => *single,
            several => self.context.struct_type(several, false),
        };
        let value = self.builder.build_inline_asm_with(
            result.fn_type(&parameters, false),
            &template,
            &constraints.join(","),
            &args,
            "asm",
            !asm.pure,
        )?;
        Ok(match outputs.len() {
            0 => self.context.i8_type().const_zero(),
            1 => value,
            count => {
                let mut tuple = self.ty(ty)?.get_undef();
                for index in 0..count as u32 {
                    let element = self
                        .builder
                        .build_extract_value(value, index, "asm.output")?;
                    tuple = self
                        .builder
                        .build_insert_value(tuple, element, index, "asm.tuple")?;
                }
                tuple
            }
        })
    }
    /// Lowers `core/cpu`. Bare-metal Cortex-M and RISC-V (machine mode) use the
    /// architectural interrupt mask; hosted OS targets expose no interrupt mask
    /// to programs, so masking and waiting there are no-ops. Other freestanding
    /// targets are rejected rather than silently given a non-exclusive section.
    fn cpu_operation(&mut self, op: &str, args: &[Expr], unit: Value<'ctx>) -> Result<Value<'ctx>> {
        if op == "fence" {
            self.builder
                .build_fence(AtomicOrdering::LLVMAtomicOrderingSequentiallyConsistent)?;
            return Ok(unit);
        }
        let argument = match args.first() {
            Some(arg) => Some(self.expr(arg)?),
            None => None,
        };
        let triple = self
            .module
            .get_triple()
            .as_str()
            .to_string_lossy()
            .into_owned();
        let arch = triple.split('-').next().unwrap_or_default();
        let hosted = [
            "-linux", "-windows", "-darwin", "-macos", "-ios", "-freebsd", "-netbsd", "-openbsd",
            "-android", "-wasi",
        ]
        .iter()
        .any(|os| triple.contains(os));
        let cortex_m = ["thumbv6m", "thumbv7m", "thumbv7em", "thumbv8m"]
            .iter()
            .any(|prefix| arch.starts_with(prefix));
        let riscv = arch.starts_with("riscv32") || arch.starts_with("riscv64");
        let void = self.context.void_type();
        if hosted {
            return Ok(match op {
                "disable_interrupts" => self.context.bool_type().const_zero(),
                "core_id" => self.context.i32_type().const_zero(),
                _ => unit,
            });
        }
        if !cortex_m && !riscv {
            // Diagnosed after dead-code elimination (see `generate`), so a
            // package that merely contains interrupt control stays portable.
            let name = format!("{UNSUPPORTED_CPU}{op}");
            let result = match op {
                "disable_interrupts" => self.context.bool_type(),
                "core_id" => self.context.i32_type(),
                _ => void,
            };
            let marker = self.module.lookup_function(&name).unwrap_or_else(|| {
                self.module
                    .add_function(&name, result.fn_type(&[], false), None)
            });
            let value = self.builder.build_call(marker, &[], "cpu.unsupported")?;
            return Ok(if matches!(op, "disable_interrupts" | "core_id") {
                value
            } else {
                unit
            });
        }
        if op == "core_id" {
            return self.core_id(riscv);
        }
        // Cortex-M PRIMASK bit 0 masks interrupts; RISC-V mstatus.MIE is bit 3.
        let (register, enable_bit) = if cortex_m {
            (self.context.i32_type(), 1)
        } else {
            (self.usize_type(), 8)
        };
        match op {
            "wait_for_interrupt" => {
                self.builder.build_inline_asm(
                    void.fn_type(&[], false),
                    "wfi",
                    "~{memory}",
                    &[],
                    "",
                )?;
                Ok(unit)
            }
            "disable_interrupts" => {
                let assembly = if cortex_m {
                    "mrs $0, PRIMASK\ncpsid i"
                } else {
                    "csrrci $0, mstatus, 8"
                };
                let previous = self.builder.build_inline_asm(
                    register.fn_type(&[], false),
                    assembly,
                    "=r,~{memory}",
                    &[],
                    "cpu.previous",
                )?;
                let bit = register.const_int(enable_bit, false);
                let masked = self.builder.build_and(previous, bit, "cpu.mask")?;
                // PRIMASK set means masked; mstatus.MIE set means enabled.
                let predicate = if cortex_m {
                    IntPredicate::LLVMIntEQ
                } else {
                    IntPredicate::LLVMIntNE
                };
                self.builder.build_int_compare(
                    predicate,
                    masked,
                    register.const_zero(),
                    "cpu.were_enabled",
                )
            }
            "restore_interrupts" => {
                let enabled = argument.ok_or_else(|| error("missing interrupt state"))?;
                let wide = self
                    .builder
                    .build_int_z_extend(enabled, register, "cpu.state")?;
                // Branch-free restore: Cortex-M writes the old PRIMASK value;
                // RISC-V sets MIE only when it was previously set.
                let (assembly, value) = if cortex_m {
                    let one = register.const_int(1, false);
                    (
                        "msr PRIMASK, $0",
                        self.builder.build_xor(wide, one, "cpu.primask")?,
                    )
                } else {
                    let shift = register.const_int(3, false);
                    (
                        "csrs mstatus, $0",
                        self.builder.build_left_shift(wide, shift, "cpu.mie")?,
                    )
                };
                self.builder.build_inline_asm(
                    void.fn_type(&[register], false),
                    assembly,
                    "r,~{memory}",
                    &[value],
                    "",
                )?;
                Ok(unit)
            }
            _ => Err(error(format!("unknown CPU operation `{op}`"))),
        }
    }
    /// Lowers `cpu.core_id()` on bare metal. RISC-V reads `mhartid`. Cortex-M
    /// has no architectural core number, so a chip runtime with several cores
    /// defines `dodo_core_id`, which is referenced weakly: without one the
    /// chip is taken to have a single core, number 0.
    fn core_id(&mut self, riscv: bool) -> Result<Value<'ctx>> {
        let i32_type = self.context.i32_type();
        if riscv {
            let hart = self.builder.build_inline_asm(
                self.usize_type().fn_type(&[], false),
                "csrr $0, mhartid",
                "=r",
                &[],
                "cpu.hart",
            )?;
            return self
                .builder
                .build_int_cast_sign_flag(hart, i32_type, false, "cpu.core");
        }
        let hook = self
            .module
            .lookup_function("dodo_core_id")
            .unwrap_or_else(|| {
                self.module.add_function(
                    "dodo_core_id",
                    i32_type.fn_type(&[], false),
                    Some(LLVMExternalWeakLinkage),
                )
            });
        let start = self.builder.get_insert_block().unwrap();
        let ask = self.bb("core_id.ask");
        let done = self.bb("core_id.done");
        let missing = self.builder.build_is_null(hook, "core_id.missing")?;
        self.builder.build_conditional_branch(missing, done, ask)?;
        self.builder.position_at_end(ask);
        let number = self.builder.build_call(hook, &[], "core_id.number")?;
        self.builder.build_unconditional_branch(done)?;
        self.builder.position_at_end(done);
        let phi = self.builder.build_phi(i32_type, "cpu.core")?;
        phi.add_incoming(&[(&i32_type.const_zero(), start), (&number, ask)]);
        Ok(phi)
    }
    fn call(
        &mut self,
        name: &str,
        type_args: &[Type],
        args: &[Expr],
        ret: &Type,
    ) -> Result<Value<'ctx>> {
        let unit = self.context.i8_type().const_zero();
        if matches!(
            name,
            "core.wrapping_add" | "core.wrapping_sub" | "core.wrapping_mul"
        ) {
            let a = self.expr(&args[0])?;
            let b = self.expr(&args[1])?;
            // Plain LLVM integer operations wrap modulo 2^N. Do not attach
            // no-wrap flags or route these through checked arithmetic.
            return Ok(match name {
                "core.wrapping_add" => self.builder.build_int_add(a, b, "wrapping.add")?,
                "core.wrapping_sub" => self.builder.build_int_sub(a, b, "wrapping.sub")?,
                "core.wrapping_mul" => self.builder.build_int_mul(a, b, "wrapping.mul")?,
                _ => unreachable!(),
            });
        }
        if matches!(name, "core.assert" | "core.assert_eq" | "core.assert_ne") {
            let count = if name == "core.assert" { 1 } else { 2 };
            let values = args
                .iter()
                .map(|e| self.expr(e))
                .collect::<Result<Vec<_>>>()?;
            let valid = if count == 1 {
                values[0]
            } else {
                let equal = if args[0].ty == Type::Str {
                    self.strings_equal(values[0], values[1])?
                } else {
                    self.binary(BinaryOp::Eq, values[0], values[1], &args[0].ty)?
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
        if matches!(name, "mem.split_at_mut" | "core.mem.split_at_mut") {
            let input = self.expr(&args[0])?;
            let mid = self.expr(&args[1])?;
            let pointer = self.builder.build_extract_value(input, 0, "split.ptr")?;
            let length = self.builder.build_extract_value(input, 1, "split.len")?;
            let inside = self.builder.build_int_compare(
                IntPredicate::LLVMIntULE,
                mid,
                length,
                "split.inside",
            )?;
            self.guard(inside, "slice bounds", "slice bounds out of range")?;
            let Type::Slice(true, element) = &args[0].ty else {
                return Err(error("split requires a mutable slice"));
            };
            // Non-inbounds GEP, just as for ordinary slicing: empty and ZST
            // slices need no dereference and may have equal data addresses.
            let right_pointer = unsafe {
                self.builder
                    .build_gep(self.ty(element)?, pointer, &[mid], "split.right.ptr")?
            };
            let right_length = self.builder.build_int_sub(length, mid, "split.right.len")?;
            let left = self
                .builder
                .build_insert_value(input, mid, 1, "split.left")?;
            let right = self
                .builder
                .build_insert_value(input, right_pointer, 0, "split.right")?;
            let right = self
                .builder
                .build_insert_value(right, right_length, 1, "split.right")?;
            let pair = self.ty(ret)?.const_zero();
            let pair = self
                .builder
                .build_insert_value(pair, left, 0, "split.pair")?;
            return self
                .builder
                .build_insert_value(pair, right, 1, "split.pair");
        }
        if name == "ok" || name == "err" || name == "some" || name == "none" {
            if matches!(ret, Type::Result(..)) {
                let payload = args.first().map(|e| self.expr(e)).transpose()?;
                return self.tagged_value(
                    self.ty(ret)?,
                    self.context
                        .bool_type()
                        .const_int((name == "err") as u64, false),
                    payload,
                );
            }
            let mut v = self.ty(ret)?.const_zero();
            let tag = name == "err" || name == "some";
            v = self.builder.build_insert_value(
                v,
                self.context.bool_type().const_int(tag as u64, false),
                0,
                "tag",
            )?;
            if let Some(e) = args.first() {
                let x = self.expr(e)?;
                v = self.builder.build_insert_value(v, x, 1, "payload")?;
            }
            return Ok(v);
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
                    .const_int(u64::from(name.ends_with("align_of")), false));
            }
            let t = self.ty(argument)?;
            let data = TargetData::create(
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
                data.offset_of_element(&t, index as u32)
                    .ok_or_else(|| error("field offset is unavailable for this struct"))?
            } else {
                data.get_abi_alignment(&t) as u64
            };
            return Ok(self.usize_type().const_int(value, false));
        }
        if let Some(op) = name.strip_prefix("core.mem.") {
            if let Some(operation) = op.strip_prefix("atomic_") {
                let triple = self
                    .module
                    .get_triple()
                    .as_str()
                    .to_string_lossy()
                    .into_owned();
                let support = atomic_support(&triple);
                if support == AtomicSupport::None
                    || (support == AtomicSupport::LoadStore
                        && !matches!(operation, "load" | "store"))
                {
                    // Diagnosed after dead-code elimination (see `generate`),
                    // so a package that merely contains these operations
                    // stays portable. The arguments are still evaluated.
                    let mut values = Vec::new();
                    for arg in args {
                        values.push(self.expr(arg)?);
                    }
                    let result = match operation {
                        "store" => None,
                        "load" => Some(self.ty(ret)?),
                        _ => Some(values[1].get_type()),
                    };
                    let name = format!(
                        "{UNSUPPORTED_ATOMIC}{operation}.i{}",
                        result.map_or(0, |ty| ty.get_bit_width())
                    );
                    let marker = self.module.lookup_function(&name).unwrap_or_else(|| {
                        let ty = match result {
                            Some(ty) => ty.fn_type(&[], false),
                            None => self.context.void_type().fn_type(&[], false),
                        };
                        self.module.add_function(&name, ty, None)
                    });
                    let value = self.builder.build_call(marker, &[], "atomic.unsupported")?;
                    return Ok(if result.is_some() { value } else { unit });
                }
                let pointer = self.expr(&args[0])?;
                let order = |index: usize| -> Result<AtomicOrdering> {
                    let ExprKind::Int(value, _) = args[index].kind else {
                        return Err(error("atomic ordering was not checked"));
                    };
                    Ok(match value {
                        0 => AtomicOrdering::LLVMAtomicOrderingMonotonic,
                        1 => AtomicOrdering::LLVMAtomicOrderingAcquire,
                        2 => AtomicOrdering::LLVMAtomicOrderingRelease,
                        3 => AtomicOrdering::LLVMAtomicOrderingAcquireRelease,
                        4 => AtomicOrdering::LLVMAtomicOrderingSequentiallyConsistent,
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
                            .set_alignment(self.ty(ret)?.get_bit_width() / 8)
                            .map_err(error)?;
                        return Ok(value);
                    }
                    "store" => {
                        let value = self.expr(&args[1])?;
                        let instruction = self.builder.build_store(pointer, value)?;
                        instruction.set_atomic_ordering(order(2)?).map_err(error)?;
                        instruction
                            .set_alignment(value.get_type().get_bit_width() / 8)
                            .map_err(error)?;
                        return Ok(unit);
                    }
                    "exchange" | "fetch_add" => {
                        let value = self.expr(&args[1])?;
                        return self.builder.build_atomicrmw(
                            if operation == "exchange" {
                                AtomicRMWBinOp::LLVMAtomicRMWBinOpXchg
                            } else {
                                AtomicRMWBinOp::LLVMAtomicRMWBinOpAdd
                            },
                            pointer,
                            value,
                            order(2)?,
                        );
                    }
                    "compare_exchange" => {
                        let expected = self.expr(&args[1])?;
                        let replacement = self.expr(&args[2])?;
                        let result = self.builder.build_cmpxchg(
                            pointer,
                            expected,
                            replacement,
                            order(3)?,
                            order(4)?,
                        )?;
                        return self
                            .builder
                            .build_extract_value(result, 0, "atomic.observed");
                    }
                    _ => return Err(error("unknown atomic intrinsic")),
                }
            }
            match op {
                "storage_type" | "assert_send" | "assert_sync" => return Ok(unit),
                "callback" => {
                    let ExprKind::Name(target) = &args[0].kind else {
                        return Err(error("callback target was not checked"));
                    };
                    return self
                        .functions
                        .get(target)
                        .copied()
                        .ok_or_else(|| error("callback function is unavailable"));
                }
                "uninit" => {
                    // The bytes are deliberately opaque. Zero initialization is an
                    // implementation choice, never a claim that T is initialized.
                    return Ok(self.ty(ret)?.const_zero());
                }
                "init" => {
                    let value = self.expr(&args[0])?;
                    return self.builder.build_insert_value(
                        self.ty(ret)?.const_zero(),
                        value,
                        0,
                        "storage.init",
                    );
                }
                "assume_init" => {
                    let storage = self.expr(&args[0])?;
                    return self
                        .builder
                        .build_extract_value(storage, 0, "storage.assume_init");
                }
                "uninit_as_ptr" | "uninit_as_mut_ptr" | "str_bytes" | "str_from_utf8" => {
                    return self.expr(&args[0]);
                }
                "replace" | "swap" => {
                    let pointer = self.expr(&args[0])?;
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
                    let other = replacement;
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
        if let Some(op) = name.strip_prefix("core.cpu.") {
            return self.cpu_operation(op, args, unit);
        }
        let mmio = name
            .strip_prefix("mmio.")
            .or_else(|| name.strip_prefix("core.mmio."));
        if let Some(op) = mmio {
            let address = self.expr(&args[0])?;
            let pointer =
                self.builder
                    .build_int_to_ptr(address, self.context.ptr_type(0), "mmio.address")?;
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
                return self.builder.build_extract_value(input, 0, "slice.pointer");
            }
            let pointer = input;
            match op {
                "view" | "borrow" | "borrow_mut" => {
                    self.expr(&args[1])?;
                    return Ok(pointer);
                }
                "view_slice" | "borrow_slice" | "borrow_slice_mut" => {
                    let length = self.expr(&args[1])?;
                    self.expr(&args[2])?;
                    let value = self.builder.build_insert_value(
                        self.ty(ret)?.const_zero(),
                        pointer,
                        0,
                        "owned.slice.pointer",
                    )?;
                    return self
                        .builder
                        .build_insert_value(value, length, 1, "owned.slice.length");
                }
                "from_ref" | "from_mut" | "as_ptr" | "as_mut_ptr" => return Ok(pointer),
                "is_null" => return self.builder.build_is_null(pointer, "ptr.is_null"),
                "relocate" | "copy" | "copy_nonoverlapping" | "write_bytes" => {
                    let second = self.expr(&args[1])?;
                    let count = self.expr(&args[2])?;
                    if op == "relocate" {
                        self.expr(&args[3])?;
                    }
                    let Type::Raw(_, element) = &args[0].ty else {
                        return Err(error("memory operation requires a raw pointer"));
                    };
                    let data = TargetData::create(
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
                        "copy" | "relocate" => {
                            self.builder.build_memmove(second, 1, pointer, 1, bytes)?;
                        }
                        "copy_nonoverlapping" => {
                            self.builder.build_memcpy(second, 1, pointer, 1, bytes)?;
                        }
                        _ => {
                            self.builder.build_memset(pointer, 1, second, bytes)?;
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
                "take" | "read" | "read_unaligned" | "read_volatile" => {
                    if op == "take" {
                        self.expr(&args[1])?;
                    }
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
                "store" | "write" | "write_unaligned" | "write_volatile" => {
                    let value = self.expr(&args[1])?;
                    if op == "store" {
                        self.expr(&args[2])?;
                    }
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
                    let offset = self.expr(&args[1])?;
                    let Type::Raw(_, t) = ret else {
                        return Err(error("pointer offset requires raw pointer"));
                    };
                    return Ok(unsafe {
                        self.builder
                            .build_gep(self.ty(t)?, pointer, &[offset], "ptr.offset")?
                    });
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
            let st = self.ty(ret)?;
            let mut payload = self.variant_ty(&decl.variants[i].fields)?.const_zero();
            let mut pending = Vec::new();
            for (j, arg) in args.iter().enumerate() {
                let x = self.stage(arg, &mut pending)?;
                payload = self
                    .builder
                    .build_insert_value(payload, x, j as u32, "payload")?;
            }
            let v = self.tagged_value(
                st,
                self.context.i32_type().const_int(i as u64, false),
                Some(payload),
            )?;
            self.transfer(&pending)?;
            return Ok(v);
        }
        if let Some((interface, method)) = name.rsplit_once('.')
            && self.interfaces.contains_key(interface)
        {
            return self.dynamic_call(interface, method, args, ret);
        }
        let f = self
            .functions
            .get(name)
            .copied()
            .ok_or_else(|| error(format!("unknown function {name}")))?;
        if let Some(lowering) = self.c_abi.get(name).cloned() {
            // A @link_name declaration may bind the symbol with its own signature.
            let fn_type = self
                .foreign_signatures
                .get(name)
                .copied()
                .unwrap_or_else(|| f.get_type());
            let params: Vec<Type> = self
                .program
                .functions
                .iter()
                .find(|function| function.name == name)
                .map(|function| function.params.iter().map(|p| p.ty.clone()).collect())
                .unwrap_or_default();
            return self.c_call(f, fn_type, (&params, ret), Some(&lowering), args);
        }
        let (indirect_return, indirect_params) = self.indirect[name].clone();
        let storage = if indirect_return {
            Some(self.alloca(self.ty(ret)?, "call.result")?)
        } else {
            None
        };
        let mut pending = Vec::new();
        let mut vs = Vec::new();
        if let Some(storage) = storage {
            vs.push(storage);
        }
        for (e, indirect) in args.iter().zip(&indirect_params) {
            let value = self.stage(e, &mut pending)?;
            if *indirect {
                let argument = self.alloca(self.ty(&e.ty)?, "call.argument")?;
                self.store_value(argument, value)?;
                vs.push(argument);
            } else {
                vs.push(value);
            }
        }
        self.transfer(&pending)?;
        let label = if *ret == Type::Void || indirect_return {
            ""
        } else {
            "call"
        };
        let call = match self.foreign_signatures.get(name) {
            Some(&ty) => self.builder.build_indirect_call(ty, f, &vs, label)?,
            None => self.builder.build_call(f, &vs, label)?,
        };
        if let Some(function) = self
            .program
            .functions
            .iter()
            .find(|f| f.name == name && f.extern_)
        {
            for (location, attribute) in self.c_abi_attributes(function)? {
                call.add_attribute(location, attribute);
            }
        }
        if let Some(storage) = storage {
            self.load(storage, ret)
        } else {
            Ok(call.basic().unwrap_or(unit))
        }
    }
    /// Slot calling convention of an interface method: large values pass
    /// through memory exactly as for internal functions, and the receiver is
    /// the value's address.
    fn dispatch_convention(&self, method: &Function) -> Result<(bool, Vec<bool>)> {
        let large = |ty: &Type| -> Result<bool> {
            Ok(*ty != Type::Void
                && !mentions_dyn_self(ty)
                && self.data.get_abi_size(&self.ty(ty)?) > 1024)
        };
        let params = method
            .params
            .iter()
            .enumerate()
            .map(|(index, p)| if index == 0 { Ok(false) } else { large(&p.ty) })
            .collect::<Result<Vec<_>>>()?;
        Ok((large(&method.ret)?, params))
    }
    fn dispatch_type(&self, method: &Function) -> Result<LlvmType<'ctx>> {
        let (indirect_return, indirect_params) = self.dispatch_convention(method)?;
        let pointer = self.context.ptr_type(0);
        let mut params = Vec::new();
        if indirect_return {
            params.push(pointer);
        }
        for (index, (p, indirect)) in method.params.iter().zip(&indirect_params).enumerate() {
            params.push(if index == 0 || *indirect {
                pointer
            } else {
                self.ty(&p.ty)?
            });
        }
        Ok(if method.ret == Type::Void || indirect_return {
            self.context.void_type().fn_type(&params, false)
        } else {
            self.ty(&method.ret)?.fn_type(&params, false)
        })
    }
    /// The method table of `ty` for `interface`: one function per interface
    /// method, in declaration order with parents first.
    fn vtable(&mut self, ty: &Type, interface: &str) -> Result<Value<'ctx>> {
        let key = (ty.to_string(), interface.to_owned());
        if let Some(table) = self.vtables.get(&key) {
            return Ok(*table);
        }
        let Type::Named(owner) = ty else {
            return Err(error(format!(
                "`{ty}` has no methods for `dyn {interface}`"
            )));
        };
        let methods = self.interfaces[interface].clone();
        let mut slots = Vec::new();
        for method in &methods {
            let name = format!("{owner}.{}", method.name);
            let function = self
                .functions
                .get(&name)
                .copied()
                .ok_or_else(|| error(format!("missing method {name} for dyn {interface}")))?;
            let wanted = self.dispatch_convention(method)?;
            slots.push(if self.indirect[&name] == wanted {
                function
            } else {
                self.dispatch_thunk(&name, method, function, wanted)?
            });
        }
        let pointer = self.context.ptr_type(0);
        let table_type = pointer.array_type(slots.len() as u32);
        let table = self.module.add_global(
            table_type,
            None,
            &format!("dodo.{}.vtable.{owner}.{interface}", self.program.package),
        );
        table.set_initializer(&pointer.const_array(&slots));
        table.set_constant(true);
        table.set_unnamed_addr(true);
        table.set_linkage(LLVMInternalLinkage);
        self.vtables.insert(key, table);
        Ok(table)
    }
    /// Adapt a method whose parameters pass differently from its slot.
    fn dispatch_thunk(
        &self,
        name: &str,
        method: &Function,
        target: Value<'ctx>,
        (slot_return, slot_params): (bool, Vec<bool>),
    ) -> Result<Value<'ctx>> {
        let thunk = self.module.add_function(
            &format!("dodo.{}.dispatch.{name}", self.program.package),
            self.dispatch_type(method)?,
            Some(LLVMInternalLinkage),
        );
        let builder = self.context.create_builder();
        builder.position_at_end(self.context.append_basic_block(thunk, "entry"));
        let (target_return, target_params) = self.indirect[name].clone();
        let mut incoming = thunk.get_param_iter();
        let slot_storage = if slot_return { incoming.next() } else { None };
        let target_storage = if target_return {
            Some(builder.build_alloca(self.ty(&method.ret)?, "result")?)
        } else {
            None
        };
        let mut args: Vec<Value<'ctx>> = target_storage.into_iter().collect();
        for (index, (value, param)) in incoming.zip(&method.params).enumerate() {
            let slot_indirect = slot_params[index];
            let target_indirect = target_params[index];
            args.push(match (slot_indirect, target_indirect) {
                (true, false) => builder.build_load(self.ty(&param.ty)?, value, "argument")?,
                (false, true) => {
                    let storage = builder.build_alloca(self.ty(&param.ty)?, "argument")?;
                    builder.build_store(storage, value)?;
                    storage
                }
                _ => value,
            });
        }
        let call = builder.build_call(target, &args, "result")?;
        if method.ret == Type::Void {
            builder.build_return(None)?;
        } else {
            let result = match target_storage {
                Some(storage) => builder.build_load(self.ty(&method.ret)?, storage, "result")?,
                None => call,
            };
            match slot_storage {
                Some(storage) => {
                    builder.build_store(storage, result)?;
                    builder.build_return(None)?;
                }
                None => {
                    builder.build_return(Some(&result))?;
                }
            }
        }
        Ok(thunk)
    }
    fn dynamic_call(
        &mut self,
        interface: &str,
        method: &str,
        args: &[Expr],
        ret: &Type,
    ) -> Result<Value<'ctx>> {
        let methods = &self.interfaces[interface];
        let slot = methods
            .iter()
            .position(|m| m.name == method)
            .ok_or_else(|| error(format!("unknown interface method {interface}.{method}")))?;
        let declaration = methods[slot].clone();
        let (indirect_return, indirect_params) = self.dispatch_convention(&declaration)?;
        let storage = if indirect_return {
            Some(self.alloca(self.ty(ret)?, "call.result")?)
        } else {
            None
        };
        let mut pending = Vec::new();
        let mut values: Vec<Value<'ctx>> = storage.into_iter().collect();
        let mut table = None;
        for (index, (e, indirect)) in args.iter().zip(&indirect_params).enumerate() {
            let value = self.stage(e, &mut pending)?;
            if index == 0 {
                values.push(self.builder.build_extract_value(value, 0, "dyn.data")?);
                table = Some(self.builder.build_extract_value(value, 1, "dyn.table")?);
            } else if *indirect {
                let argument = self.alloca(self.ty(&e.ty)?, "call.argument")?;
                self.store_value(argument, value)?;
                values.push(argument);
            } else {
                values.push(value);
            }
        }
        self.transfer(&pending)?;
        let table = table.ok_or_else(|| error("dynamic call without a receiver"))?;
        let pointer = self.context.ptr_type(0);
        let entry = unsafe {
            self.builder.build_gep(
                pointer,
                table,
                &[self.usize_type().const_int(slot as u64, false)],
                "dyn.slot",
            )?
        };
        let function = self.builder.build_load(pointer, entry, "dyn.method")?;
        let call = self.builder.build_indirect_call(
            self.dispatch_type(&declaration)?,
            function,
            &values,
            "call",
        )?;
        if let Some(storage) = storage {
            self.load(storage, ret)
        } else {
            Ok(call.basic().unwrap_or(self.context.i8_type().const_zero()))
        }
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
        let counter = self.alloca(self.usize_type(), "foreach.index")?;
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
            .build_load(self.usize_type(), counter, "index")?;
        let test =
            self.builder
                .build_int_compare(IntPredicate::LLVMIntULT, i, len, "foreach.more")?;
        self.builder.build_conditional_branch(test, bb, end)?;
        self.builder.position_at_end(bb);
        self.loops.push(Loop {
            end,
            next: step,
            depth: self.scopes.len(),
        });
        self.push_scope();
        if let Some(name) = index {
            self.bind(name, &Type::usize(), Some(i))?;
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
            self.bind(name, &Type::Ref(mutable, Box::new(element)), Some(p))?;
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
    ) -> Result<(Value<'ctx>, Type, Option<bool>, Option<Binding<'ctx>>)> {
        if let Type::Ref(mutable, inner) = &value.ty {
            Ok((self.expr(value)?, *inner.clone(), Some(*mutable), None))
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
        mut ptr: Value<'ctx>,
        ty: &Type,
        mut borrowed: Option<bool>,
    ) -> Result<(Value<'ctx>, Type, Option<bool>)> {
        let mut ty = ty.clone();
        if !matches!(pattern, Pattern::Binding(_) | Pattern::Wildcard) {
            while let Type::Ref(mutable, inner) = &ty {
                ptr = self.load(ptr, &ty)?;
                borrowed = Some(borrowed.is_none_or(|outer| outer) && *mutable);
                ty = *inner.clone();
            }
        }
        Ok((ptr, ty, borrowed))
    }
    fn struct_pattern_fields(
        &self,
        ptr: Value<'ctx>,
        ty: &Type,
    ) -> Result<Vec<(String, Value<'ctx>, Type)>> {
        let Type::Named(name) = ty else {
            return Err(error("struct pattern on non-struct value"));
        };
        let declaration = self
            .program
            .structs
            .iter()
            .find(|s| s.name == *name)
            .ok_or_else(|| error("unknown struct pattern"))?;
        let st = self.ty(ty)?;
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
        ptr: Value<'ctx>,
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
                let value_ir = self.load(ptr, &ty)?;
                let signed = matches!(ty, Type::Int { signed: true, .. });
                let lower = value_ir.get_type().const_int(*value, false);
                let test = if let Pattern::Range(_, upper, inclusive) = pattern {
                    let lower_test = self.builder.build_int_compare(
                        if signed {
                            IntPredicate::LLVMIntSGE
                        } else {
                            IntPredicate::LLVMIntUGE
                        },
                        value_ir,
                        lower,
                        "pattern.lower",
                    )?;
                    let upper_test = self.builder.build_int_compare(
                        match (signed, inclusive) {
                            (true, true) => IntPredicate::LLVMIntSLE,
                            (true, false) => IntPredicate::LLVMIntSLT,
                            (false, true) => IntPredicate::LLVMIntULE,
                            (false, false) => IntPredicate::LLVMIntULT,
                        },
                        value_ir,
                        value_ir.get_type().const_int(*upper, false),
                        "pattern.upper",
                    )?;
                    self.builder
                        .build_and(lower_test, upper_test, "pattern.range")?
                } else {
                    self.builder.build_int_compare(
                        IntPredicate::LLVMIntEQ,
                        value_ir,
                        lower,
                        "pattern.equal",
                    )?
                };
                self.builder.build_conditional_branch(test, yes, no)?;
            }
            Pattern::Bool(value) => {
                let value_ir = self.load(ptr, &ty)?;
                let test = self.builder.build_int_compare(
                    IntPredicate::LLVMIntEQ,
                    value_ir,
                    self.context.bool_type().const_int(*value as u64, false),
                    "pattern.bool",
                )?;
                self.builder.build_conditional_branch(test, yes, no)?;
            }
            Pattern::Variant(name, fields) => {
                let tagptr = self
                    .builder
                    .build_struct_gep(self.ty(&ty)?, ptr, 0, "pattern.tag")?;
                let tag_type = if matches!(ty, Type::Named(_)) {
                    self.context.i32_type()
                } else {
                    self.context.bool_type()
                };
                let tag = self.builder.build_load(tag_type, tagptr, "tag")?;
                let test = self.builder.build_int_compare(
                    IntPredicate::LLVMIntEQ,
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
        ptr: Value<'ctx>,
        ty: &Type,
        borrowed: Option<bool>,
        preview: bool,
        destinations: Option<&HashMap<String, Binding<'ctx>>>,
    ) -> Result<()> {
        let (ptr, ty, borrowed) = self.pattern_place(pattern, ptr, ty, borrowed)?;
        match pattern {
            Pattern::Binding(name) => {
                let (binding_ty, value) = if let Some(mutable) = borrowed {
                    (Type::Ref(mutable, Box::new(ty)), ptr)
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
    /// Integer values selected by an arm that can join a `switch`: no guard,
    /// and every alternative is an integer literal or a small range. Values
    /// are u64 bit patterns like `Pattern::Int`.
    fn switch_arm_values(arm: &MatchArm, signed: bool) -> Option<Vec<u64>> {
        if arm.guard.is_some() {
            return None;
        }
        let mut values = vec![];
        for pattern in Self::pattern_alternatives(&arm.pattern) {
            match pattern {
                Pattern::Int(value) => values.push(value),
                Pattern::Range(lower, upper, inclusive) => {
                    let widen = |v: u64| if signed { v as i64 as i128 } else { v as i128 };
                    let lower = widen(lower);
                    let upper = widen(upper) - if inclusive { 0 } else { 1 };
                    if upper - lower >= MAX_SWITCH_RANGE {
                        return None;
                    }
                    values.extend((lower..=upper).map(|v| v as u64));
                }
                _ => return None,
            }
        }
        Some(values)
    }
    /// Whether a match dispatches as an interpreter loop: it is the last
    /// statement of an unconditional `for` loop, has many cases on an 8-bit
    /// value (so a 256-entry table needs no range check), and the statements
    /// before it can run again at the end of each arm. Their `let` bindings
    /// are then stored into the bindings of the loop body, which must hold
    /// nothing to drop and shadow no outer name.
    fn threaded_dispatch(
        &self,
        value: &Expr,
        actual: &Type,
        bits: u32,
        cases: usize,
    ) -> Option<ThreadedDispatch<'ctx>> {
        let dispatch = self.dispatch_loop.as_ref()?;
        if !std::ptr::eq(dispatch.value, value)
            || bits > 8
            || cases < MIN_DISPATCH_CASES
            || matches!(actual, Type::Ref(..))
        {
            return None;
        }
        let scopes = self.scopes.get(dispatch.body_scope..)?;
        if scopes
            .iter()
            .flatten()
            .any(|binding| binding.live.is_some())
        {
            return None;
        }
        let body = &scopes[0];
        let mut rebind = HashMap::new();
        for statement in &dispatch.prefix {
            match &statement.kind {
                StmtKind::LetPattern { .. } => return None,
                StmtKind::Let { name, value, .. } => {
                    let shadows = self.scopes[..dispatch.body_scope]
                        .iter()
                        .flatten()
                        .any(|binding| binding.name == *name)
                        || self.globals.contains_key(name);
                    let simple = value
                        .as_ref()
                        .is_some_and(|value| !matches!(value.kind, ExprKind::Repeat(..)));
                    let binding = body.iter().rev().find(|binding| binding.name == *name)?;
                    if shadows || !simple || rebind.insert(name.clone(), binding.clone()).is_some()
                    {
                        return None;
                    }
                }
                _ => {}
            }
        }
        Some(ThreadedDispatch {
            prefix: dispatch.prefix.clone(),
            rebind,
        })
    }
    /// Branch through a dispatch table of block addresses.
    fn indirect_dispatch(
        &self,
        table: Value<'ctx>,
        size: usize,
        value: Value<'ctx>,
        destinations: &[BasicBlock<'ctx>],
    ) -> Result<()> {
        let i64_type = self.context.i64_type();
        let index = self
            .builder
            .build_int_z_extend(value, i64_type, "dispatch.index")?;
        let pointer = self.context.ptr_type(0);
        // SAFETY: the table has an entry for every value of the 8-bit index.
        let slot = unsafe {
            self.builder.build_gep(
                pointer.array_type(size as u32),
                table,
                &[i64_type.const_zero(), index],
                "dispatch.slot",
            )?
        };
        let target = self.builder.build_load(pointer, slot, "dispatch.target")?;
        self.builder.build_indirect_br(target, destinations)?;
        Ok(())
    }
    /// Dispatch the leading run of integer-literal arms of a match with one
    /// `switch` (the first arm listing a value wins, as in the test chain).
    /// Returns how many arms it emitted; the caller continues with the test
    /// chain for the rest from the switch's default block. A chain of
    /// compares is left to LLVM to rebuild, which it only does partially for
    /// large matches such as interpreter dispatch loops.
    #[allow(clippy::too_many_arguments)]
    fn match_switch_prefix(
        &mut self,
        value: &Expr,
        arms: &[MatchArm],
        ptr: Value<'ctx>,
        actual: &Type,
        borrowed: Option<bool>,
        owner: &Option<Binding<'ctx>>,
        end: BasicBlock<'ctx>,
        reaches_end: &mut bool,
    ) -> Result<usize> {
        let probe = Pattern::Int(0);
        let (place, ty, _) = self.pattern_place(&probe, ptr, actual, None)?;
        let Type::Int { signed, .. } = ty else {
            return Ok(0);
        };
        let arm_values: Vec<Vec<u64>> = arms
            .iter()
            .map_while(|arm| Self::switch_arm_values(arm, signed))
            .collect();
        if arm_values.iter().map(Vec::len).sum::<usize>() < MIN_SWITCH_CASES {
            return Ok(0);
        }
        let value_ir = self.load(place, &ty)?;
        let int_type = value_ir.get_type();
        let bits = int_type.get_bit_width();
        let mask = if bits < u64::BITS {
            (1u64 << bits) - 1
        } else {
            u64::MAX
        };
        let default = self.bb("match.next");
        let mut seen = HashSet::new();
        let mut cases = vec![];
        let mut raw_cases = vec![];
        let mut targets = vec![];
        for values in &arm_values {
            let yes = self.bb("match.pattern");
            for value in values {
                if seen.insert(value & mask) {
                    cases.push((int_type.const_int(*value, false), yes));
                    raw_cases.push((value & mask, yes));
                }
            }
            targets.push(yes);
        }
        let threaded = self.threaded_dispatch(value, actual, bits, cases.len());
        let mut destinations = targets.clone();
        destinations.push(default);
        let table = if threaded.is_some() {
            let function = self.function.unwrap();
            let mut entries = vec![default.address(function); 1usize << bits];
            for (value, block) in &raw_cases {
                entries[*value as usize] = block.address(function);
            }
            let pointer = self.context.ptr_type(0);
            let global = self.module.add_global(
                pointer.array_type(entries.len() as u32),
                None,
                "match.dispatch",
            );
            global.set_initializer(&pointer.const_array(&entries));
            global.set_constant(true);
            global.set_linkage(LLVMPrivateLinkage);
            global.set_unnamed_addr(true);
            self.indirect_dispatch(global, entries.len(), value_ir, &destinations)?;
            Some((global, entries.len()))
        } else {
            self.builder.build_switch(value_ir, default, &cases)?;
            None
        };
        for (arm, yes) in arms.iter().zip(targets) {
            self.builder.position_at_end(yes);
            self.push_scope();
            self.pattern_commit(owner)?;
            self.pattern_bind(&probe, ptr, actual, borrowed, false, None)?;
            self.block(&arm.body)?;
            if !self.terminated() {
                self.cleanup_to(self.scopes.len() - 1)?;
                if let (Some(plan), Some((global, size))) = (&threaded, table) {
                    // Run the loop's next iteration up to its dispatch here,
                    // so every arm ends in its own indirect branch.
                    self.pop_scope();
                    self.push_scope();
                    let outer = self
                        .rebind
                        .replace((self.scopes.len(), plan.rebind.clone()));
                    for statement in &plan.prefix {
                        if self.terminated() {
                            break;
                        }
                        self.stmt(statement)?;
                    }
                    self.rebind = outer;
                    if !self.terminated() {
                        let next = self.expr(value)?;
                        self.builder.build_store(place, next)?;
                        self.cleanup_to(self.scopes.len() - 1)?;
                        self.indirect_dispatch(global, size, next, &destinations)?;
                    }
                } else {
                    self.builder.build_unconditional_branch(end)?;
                    *reaches_end = true;
                }
            }
            self.pop_scope();
        }
        self.builder.position_at_end(default);
        Ok(arm_values.len())
    }
    fn match_stmt(&mut self, value: &Expr, arms: &[MatchArm]) -> Result<()> {
        self.push_scope();
        let (ptr, actual, borrowed, owner) = self.pattern_value(value)?;
        let end = self.bb("match.end");
        let mut reaches_end = false;
        let switched = self.match_switch_prefix(
            value,
            arms,
            ptr,
            &actual,
            borrowed,
            &owner,
            end,
            &mut reaches_end,
        )?;
        for arm in &arms[switched..] {
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
                    let condition = self.expr(guard)?;
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
    /// `?` wraps a `source` error in the `target` variant carrying it.
    fn wrap_error(
        &self,
        target: &Type,
        source: &Type,
        value: Option<Value<'ctx>>,
    ) -> Result<Value<'ctx>> {
        let Type::Named(name) = target else {
            return Err(error(format!("cannot convert `{source}` to `{target}`")));
        };
        let decl = self
            .program
            .enums
            .iter()
            .find(|e| e.name == *name)
            .ok_or_else(|| error(format!("cannot convert `{source}` to `{target}`")))?;
        let variants = decl.wrapping_variants(source);
        let [variant] = variants.as_slice() else {
            return Err(error(format!("cannot convert `{source}` to `{target}`")));
        };
        let tag = decl
            .variants
            .iter()
            .position(|v| v.name == variant.name)
            .unwrap();
        let mut payload = self.variant_ty(&variant.fields)?.const_zero();
        if let Some(value) = value {
            payload = self
                .builder
                .build_insert_value(payload, value, 0, "wrapped.error")?;
        }
        self.tagged_value(
            self.ty(target)?,
            self.context.i32_type().const_int(tag as u64, false),
            Some(payload),
        )
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
        ptr: Value<'ctx>,
        t: &Type,
        name: &str,
    ) -> Result<Vec<(Value<'ctx>, Type)>> {
        let tag = self.variant_tag(t, name)?;
        let st = self.ty(t)?;
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
