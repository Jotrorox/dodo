//! The compiler's LLVM C API boundary. Owners release LLVM allocations in Drop;
//! borrowed IR handles stay internal and are only used while their module and
//! context are alive. Semantic checking supplies the operand/type invariants.
//! Public function views borrow the module so callers cannot retain stale IR.
use super::{Options, Result, error};
use llvm_sys::{core::*, prelude::*, target::*, target_machine::*, transforms::pass_builder::*, *};
use std::{
    ffi::{CStr, CString},
    marker::PhantomData,
    path::Path,
    ptr,
    sync::Once,
};

mod debug;
pub(super) use debug::{DebugBuilder, Metadata};

fn cstring(text: &str) -> CString {
    CString::new(text).expect("LLVM identifier contains a NUL byte")
}
fn path_string(path: &Path) -> Result<CString> {
    CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| error("output path contains a NUL byte"))
}
/// Take ownership of a message allocated by LLVM, including error messages.
unsafe fn message(raw: *mut std::ffi::c_char) -> String {
    if raw.is_null() {
        return String::new();
    }
    let result = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe { LLVMDisposeMessage(raw) };
    result
}

pub struct Context(LLVMContextRef);
impl Context {
    pub fn create() -> Self {
        Self(unsafe { LLVMContextCreate() })
    }
    pub(super) fn create_module(&self, name: &str) -> Module<'_> {
        Module(
            unsafe { LLVMModuleCreateWithNameInContext(cstring(name).as_ptr(), self.0) },
            PhantomData,
        )
    }
    pub(super) fn create_builder(&self) -> Builder<'_> {
        Builder(unsafe { LLVMCreateBuilderInContext(self.0) }, PhantomData)
    }
    pub(super) fn custom_width_int_type(&self, bits: std::num::NonZeroU32) -> Option<LlvmType<'_>> {
        (bits.get() <= (1 << 23)).then(|| {
            LlvmType(
                unsafe { LLVMIntTypeInContext(self.0, bits.get()) },
                PhantomData,
            )
        })
    }
    pub(super) fn bool_type(&self) -> LlvmType<'_> {
        self.custom_width_int_type(std::num::NonZeroU32::new(1).unwrap())
            .unwrap()
    }
    pub(super) fn i8_type(&self) -> LlvmType<'_> {
        self.custom_width_int_type(std::num::NonZeroU32::new(8).unwrap())
            .unwrap()
    }
    pub(super) fn i32_type(&self) -> LlvmType<'_> {
        self.custom_width_int_type(std::num::NonZeroU32::new(32).unwrap())
            .unwrap()
    }
    pub(super) fn i64_type(&self) -> LlvmType<'_> {
        self.custom_width_int_type(std::num::NonZeroU32::new(64).unwrap())
            .unwrap()
    }
    pub(super) fn f32_type(&self) -> LlvmType<'_> {
        LlvmType(unsafe { LLVMFloatTypeInContext(self.0) }, PhantomData)
    }
    pub(super) fn f64_type(&self) -> LlvmType<'_> {
        LlvmType(unsafe { LLVMDoubleTypeInContext(self.0) }, PhantomData)
    }
    pub(super) fn void_type(&self) -> LlvmType<'_> {
        LlvmType(unsafe { LLVMVoidTypeInContext(self.0) }, PhantomData)
    }
    pub(super) fn ptr_type(&self, space: u32) -> LlvmType<'_> {
        LlvmType(
            unsafe { LLVMPointerTypeInContext(self.0, space) },
            PhantomData,
        )
    }
    pub(super) fn struct_type<'ctx>(
        &'ctx self,
        fields: &[LlvmType<'ctx>],
        packed: bool,
    ) -> LlvmType<'ctx> {
        let mut fields: Vec<_> = fields.iter().map(|t| t.0).collect();
        LlvmType(
            unsafe {
                LLVMStructTypeInContext(
                    self.0,
                    fields.as_mut_ptr(),
                    fields.len() as u32,
                    packed.into(),
                )
            },
            PhantomData,
        )
    }
    pub(super) fn opaque_struct_type(&self, name: &str) -> LlvmType<'_> {
        LlvmType(
            unsafe { LLVMStructCreateNamed(self.0, cstring(name).as_ptr()) },
            PhantomData,
        )
    }
    pub(super) fn const_string(&self, bytes: &[u8], terminated: bool) -> Value<'_> {
        Value(
            unsafe {
                LLVMConstStringInContext2(
                    self.0,
                    bytes.as_ptr().cast(),
                    bytes.len(),
                    (!terminated).into(),
                )
            },
            PhantomData,
        )
    }
    pub(super) fn append_basic_block<'ctx>(
        &'ctx self,
        function: Value<'ctx>,
        name: &str,
    ) -> BasicBlock<'ctx> {
        BasicBlock(
            unsafe { LLVMAppendBasicBlockInContext(self.0, function.0, cstring(name).as_ptr()) },
            PhantomData,
        )
    }
    pub(super) fn create_enum_attribute(&self, kind: u32, value: u64) -> Attribute<'_> {
        Attribute(
            unsafe { LLVMCreateEnumAttribute(self.0, kind, value) },
            PhantomData,
        )
    }
    pub(super) fn create_string_attribute(&self, key: &str, value: &str) -> Attribute<'_> {
        Attribute(
            unsafe {
                LLVMCreateStringAttribute(
                    self.0,
                    key.as_ptr().cast(),
                    key.len() as u32,
                    value.as_ptr().cast(),
                    value.len() as u32,
                )
            },
            PhantomData,
        )
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        unsafe { LLVMContextDispose(self.0) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LlvmType<'ctx>(LLVMTypeRef, PhantomData<&'ctx Context>);
impl<'ctx> LlvmType<'ctx> {
    pub(super) fn fn_type(self, params: &[Self], variadic: bool) -> Self {
        let mut params: Vec<_> = params.iter().map(|t| t.0).collect();
        Self(
            unsafe {
                LLVMFunctionType(
                    self.0,
                    params.as_mut_ptr(),
                    params.len() as u32,
                    variadic.into(),
                )
            },
            PhantomData,
        )
    }
    pub(super) fn array_type(self, len: u32) -> Self {
        Self(unsafe { LLVMArrayType2(self.0, len.into()) }, PhantomData)
    }
    pub(super) fn const_array(self, values: &[Value<'ctx>]) -> Value<'ctx> {
        let mut values: Vec<_> = values.iter().map(|v| v.0).collect();
        Value(
            unsafe { LLVMConstArray2(self.0, values.as_mut_ptr(), values.len() as u64) },
            PhantomData,
        )
    }
    pub(super) fn const_named_struct(self, values: &[Value<'ctx>]) -> Value<'ctx> {
        let mut values: Vec<_> = values.iter().map(|v| v.0).collect();
        Value(
            unsafe { LLVMConstNamedStruct(self.0, values.as_mut_ptr(), values.len() as u32) },
            PhantomData,
        )
    }
    pub(super) fn const_int(self, value: u64, sign: bool) -> Value<'ctx> {
        Value(
            unsafe { LLVMConstInt(self.0, value, sign.into()) },
            PhantomData,
        )
    }
    pub(super) fn const_float(self, value: f64) -> Value<'ctx> {
        Value(unsafe { LLVMConstReal(self.0, value) }, PhantomData)
    }
    pub(super) fn const_zero(self) -> Value<'ctx> {
        Value(unsafe { LLVMConstNull(self.0) }, PhantomData)
    }
    pub(super) fn const_all_ones(self) -> Value<'ctx> {
        Value(unsafe { LLVMConstAllOnes(self.0) }, PhantomData)
    }
    pub(super) fn get_undef(self) -> Value<'ctx> {
        Value(unsafe { LLVMGetUndef(self.0) }, PhantomData)
    }
    pub(super) fn is_sized(self) -> bool {
        unsafe { LLVMTypeIsSized(self.0) != 0 }
    }
    pub(super) fn is_opaque(self) -> bool {
        unsafe { LLVMIsOpaqueStruct(self.0) != 0 }
    }
    pub(super) fn set_body(self, fields: &[Self], packed: bool) {
        let mut fields: Vec<_> = fields.iter().map(|t| t.0).collect();
        unsafe {
            LLVMStructSetBody(
                self.0,
                fields.as_mut_ptr(),
                fields.len() as u32,
                packed.into(),
            )
        }
    }
    pub(super) fn get_field_type_at_index(self, index: u32) -> Option<Self> {
        (index < unsafe { LLVMCountStructElementTypes(self.0) }).then(|| {
            Self(
                unsafe { LLVMStructGetTypeAtIndex(self.0, index) },
                PhantomData,
            )
        })
    }
    pub(super) fn get_bit_width(self) -> u32 {
        match unsafe { LLVMGetTypeKind(self.0) } {
            LLVMTypeKind::LLVMIntegerTypeKind => unsafe { LLVMGetIntTypeWidth(self.0) },
            LLVMTypeKind::LLVMFloatTypeKind => 32,
            LLVMTypeKind::LLVMDoubleTypeKind => 64,
            _ => panic!("expected a numeric LLVM type"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Value<'ctx>(LLVMValueRef, PhantomData<&'ctx Context>);
impl<'ctx> Value<'ctx> {
    fn optional(raw: LLVMValueRef) -> Option<Self> {
        (!raw.is_null()).then_some(Self(raw, PhantomData))
    }
    pub(super) fn get_type(self) -> LlvmType<'ctx> {
        LlvmType(
            unsafe {
                if LLVMIsAFunction(self.0).is_null() {
                    LLVMTypeOf(self.0)
                } else {
                    LLVMGlobalGetValueType(self.0)
                }
            },
            PhantomData,
        )
    }
    pub(super) fn is_null(self) -> bool {
        unsafe { LLVMIsNull(self.0) != 0 }
    }
    pub(super) fn is_pointer_value(self) -> bool {
        unsafe { LLVMGetTypeKind(LLVMTypeOf(self.0)) == LLVMTypeKind::LLVMPointerTypeKind }
    }
    pub(super) fn is_struct_value(self) -> bool {
        unsafe { LLVMGetTypeKind(LLVMTypeOf(self.0)) == LLVMTypeKind::LLVMStructTypeKind }
    }
    pub(super) fn const_neg(self) -> Self {
        Self(unsafe { LLVMConstNeg(self.0) }, PhantomData)
    }
    pub(super) fn basic(self) -> Option<Self> {
        (unsafe { LLVMGetTypeKind(LLVMTypeOf(self.0)) } != LLVMTypeKind::LLVMVoidTypeKind)
            .then_some(self)
    }
    pub(super) fn as_instruction_value(self) -> Option<Self> {
        Self::optional(unsafe { LLVMIsAInstruction(self.0) })
    }
    pub(super) fn get_next_instruction(self) -> Option<Self> {
        Self::optional(unsafe { LLVMGetNextInstruction(self.0) })
    }
    pub(super) fn get_name(&self) -> &CStr {
        unsafe { CStr::from_ptr(LLVMGetValueName2(self.0, &mut 0)) }
    }
    pub(super) fn get_linkage(self) -> LLVMLinkage {
        unsafe { LLVMGetLinkage(self.0) }
    }
    pub(super) fn set_linkage(self, linkage: LLVMLinkage) {
        unsafe { LLVMSetLinkage(self.0, linkage) }
    }
    pub(super) fn set_constant(self, constant: bool) {
        unsafe { LLVMSetGlobalConstant(self.0, constant.into()) }
    }
    pub(super) fn set_unnamed_addr(self, unnamed: bool) {
        unsafe {
            LLVMSetUnnamedAddress(
                self.0,
                if unnamed {
                    LLVMUnnamedAddr::LLVMGlobalUnnamedAddr
                } else {
                    LLVMUnnamedAddr::LLVMNoUnnamedAddr
                },
            )
        }
    }
    pub(super) fn set_initializer(self, value: &Self) {
        unsafe { LLVMSetInitializer(self.0, value.0) }
    }
    pub(super) fn set_section(self, section: Option<&str>) {
        unsafe { LLVMSetSection(self.0, cstring(section.unwrap_or("")).as_ptr()) }
    }
    pub(super) fn count_basic_blocks(self) -> u32 {
        unsafe { LLVMCountBasicBlocks(self.0) }
    }
    pub(super) fn get_first_basic_block(self) -> Option<BasicBlock<'ctx>> {
        BasicBlock::optional(unsafe { LLVMGetFirstBasicBlock(self.0) })
    }
    pub(super) fn get_nth_param(self, index: u32) -> Option<Self> {
        (index < unsafe { LLVMCountParams(self.0) })
            .then(|| Self(unsafe { LLVMGetParam(self.0, index) }, PhantomData))
    }
    pub(super) fn get_param_iter(self) -> impl Iterator<Item = Self> {
        (0..unsafe { LLVMCountParams(self.0) }).map(move |i| self.get_nth_param(i).unwrap())
    }
    pub(super) fn add_attribute(self, location: AttributeLoc, attribute: Attribute<'ctx>) {
        unsafe {
            if LLVMIsAFunction(self.0).is_null() {
                LLVMAddCallSiteAttribute(self.0, location.index(), attribute.0)
            } else {
                LLVMAddAttributeAtIndex(self.0, location.index(), attribute.0)
            }
        }
    }
    pub(super) fn set_alignment(self, alignment: u32) -> Result<()> {
        if !alignment.is_power_of_two() {
            return Err(error("LLVM alignment must be a power of two"));
        }
        unsafe { LLVMSetAlignment(self.0, alignment) };
        Ok(())
    }
    pub(super) fn set_atomic_ordering(self, ordering: LLVMAtomicOrdering) -> Result<()> {
        unsafe { LLVMSetOrdering(self.0, ordering) };
        Ok(())
    }
    pub(super) fn set_volatile(self, volatile: bool) -> Result<()> {
        unsafe { LLVMSetVolatile(self.0, volatile.into()) };
        Ok(())
    }
    pub(super) fn add_incoming(self, incoming: &[(&Self, BasicBlock<'ctx>)]) {
        let mut values: Vec<_> = incoming.iter().map(|(v, _)| v.0).collect();
        let mut blocks: Vec<_> = incoming.iter().map(|(_, b)| b.0).collect();
        unsafe {
            LLVMAddIncoming(
                self.0,
                values.as_mut_ptr(),
                blocks.as_mut_ptr(),
                incoming.len() as u32,
            )
        }
    }
}
#[derive(Clone, Copy)]
pub(super) struct Attribute<'ctx>(LLVMAttributeRef, PhantomData<&'ctx Context>);
impl Attribute<'_> {
    pub(super) fn get_named_enum_kind_id(name: &str) -> u32 {
        unsafe { LLVMGetEnumAttributeKindForName(name.as_ptr().cast(), name.len()) }
    }
}
#[derive(Clone, Copy)]
pub(super) enum AttributeLoc {
    Function,
    Return,
    Param(u32),
}
impl AttributeLoc {
    fn index(self) -> u32 {
        match self {
            Self::Function => LLVMAttributeFunctionIndex,
            Self::Return => 0,
            Self::Param(i) => i + 1,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct BasicBlock<'ctx>(LLVMBasicBlockRef, PhantomData<&'ctx Context>);
impl<'ctx> BasicBlock<'ctx> {
    fn optional(raw: LLVMBasicBlockRef) -> Option<Self> {
        (!raw.is_null()).then_some(Self(raw, PhantomData))
    }
    pub(super) fn get_first_instruction(self) -> Option<Value<'ctx>> {
        Value::optional(unsafe { LLVMGetFirstInstruction(self.0) })
    }
    pub(super) fn get_terminator(self) -> Option<Value<'ctx>> {
        Value::optional(unsafe { LLVMGetBasicBlockTerminator(self.0) })
    }
}

pub struct Module<'ctx>(LLVMModuleRef, PhantomData<&'ctx Context>);
impl<'ctx> Module<'ctx> {
    pub(super) fn set_source_file_name(&self, name: &str) {
        unsafe { LLVMSetSourceFileName(self.0, name.as_ptr().cast(), name.len()) }
    }
    pub(super) fn set_triple(&self, triple: &TargetTriple) {
        unsafe { LLVMSetTarget(self.0, triple.0.as_ptr()) }
    }
    pub fn get_triple(&self) -> TargetTriple {
        TargetTriple(unsafe { CStr::from_ptr(LLVMGetTarget(self.0)) }.into())
    }
    pub(super) fn set_data_layout(&self, layout: &TargetTriple) {
        unsafe { LLVMSetDataLayout(self.0, layout.0.as_ptr()) }
    }
    pub(super) fn get_data_layout(&self) -> TargetTriple {
        TargetTriple(unsafe { CStr::from_ptr(LLVMGetDataLayoutStr(self.0)) }.into())
    }
    pub(super) fn add_function(
        &self,
        name: &str,
        ty: LlvmType<'ctx>,
        linkage: Option<LLVMLinkage>,
    ) -> Value<'ctx> {
        let value = Value(
            unsafe { LLVMAddFunction(self.0, cstring(name).as_ptr(), ty.0) },
            PhantomData,
        );
        if let Some(linkage) = linkage {
            value.set_linkage(linkage);
        }
        value
    }
    pub(super) fn lookup_function(&self, name: &str) -> Option<Value<'ctx>> {
        Value::optional(unsafe { LLVMGetNamedFunction(self.0, cstring(name).as_ptr()) })
    }
    pub fn get_function(&self, name: &str) -> Option<Function<'_>> {
        self.lookup_function(name).map(Function)
    }
    pub fn get_functions(&self) -> impl Iterator<Item = Function<'_>> {
        self.functions().map(Function)
    }
    pub(super) fn functions(&self) -> impl Iterator<Item = Value<'ctx>> {
        let first = Value::optional(unsafe { LLVMGetFirstFunction(self.0) });
        std::iter::successors(first, |v| {
            Value::optional(unsafe { LLVMGetNextFunction(v.0) })
        })
    }
    pub(super) fn add_global(
        &self,
        ty: LlvmType<'ctx>,
        space: Option<u32>,
        name: &str,
    ) -> Value<'ctx> {
        Value(
            unsafe {
                LLVMAddGlobalInAddressSpace(
                    self.0,
                    ty.0,
                    cstring(name).as_ptr(),
                    space.unwrap_or(0),
                )
            },
            PhantomData,
        )
    }
    pub(super) fn get_global(&self, name: &str) -> Option<Value<'ctx>> {
        Value::optional(unsafe { LLVMGetNamedGlobal(self.0, cstring(name).as_ptr()) })
    }
    pub(super) fn set_noduplicate_comdat(&self, function: Value<'ctx>, name: &str) {
        unsafe {
            let comdat = comdat::LLVMGetOrInsertComdat(self.0, cstring(name).as_ptr());
            comdat::LLVMSetComdatSelectionKind(
                comdat,
                comdat::LLVMComdatSelectionKind::LLVMNoDuplicatesComdatSelectionKind,
            );
            comdat::LLVMSetComdat(function.0, comdat);
        }
    }
    pub(super) fn intrinsic(&self, name: &str, types: &[LlvmType<'ctx>]) -> Option<Value<'ctx>> {
        let id = unsafe { LLVMLookupIntrinsicID(name.as_ptr().cast(), name.len()) };
        if id == 0 {
            return None;
        }
        let mut types: Vec<_> = types.iter().map(|t| t.0).collect();
        Value::optional(unsafe {
            LLVMGetIntrinsicDeclaration(self.0, id, types.as_mut_ptr(), types.len())
        })
    }
    pub fn verify(&self) -> Result<()> {
        let mut msg = ptr::null_mut();
        let failed = unsafe {
            analysis::LLVMVerifyModule(
                self.0,
                analysis::LLVMVerifierFailureAction::LLVMReturnStatusAction,
                &mut msg,
            )
        };
        let msg = unsafe { message(msg) };
        if failed != 0 { Err(error(msg)) } else { Ok(()) }
    }
    pub(super) fn run_passes(&self, passes: &str, machine: &TargetMachine) -> Result<()> {
        let passes = cstring(passes);
        unsafe {
            let options = LLVMCreatePassBuilderOptions();
            let err = LLVMRunPasses(self.0, passes.as_ptr(), machine.0, options);
            LLVMDisposePassBuilderOptions(options);
            if err.is_null() {
                return Ok(());
            }
            let msg = llvm_sys::error::LLVMGetErrorMessage(err);
            let text = CStr::from_ptr(msg).to_string_lossy().into_owned();
            llvm_sys::error::LLVMDisposeErrorMessage(msg);
            Err(error(text))
        }
    }
    pub fn print_to_string(&self) -> String {
        unsafe { message(LLVMPrintModuleToString(self.0)) }
    }
    pub fn print_to_file(&self, path: &Path) -> Result<()> {
        let path = path_string(path)?;
        let mut msg = ptr::null_mut();
        let failed = unsafe { LLVMPrintModuleToFile(self.0, path.as_ptr(), &mut msg) };
        let msg = unsafe { message(msg) };
        if failed != 0 { Err(error(msg)) } else { Ok(()) }
    }
    pub fn write_bitcode_to_path(&self, path: &Path) -> bool {
        let Ok(path) = path_string(path) else {
            return false;
        };
        unsafe { bit_writer::LLVMWriteBitcodeToFile(self.0, path.as_ptr()) == 0 }
    }
}
impl Drop for Module<'_> {
    fn drop(&mut self) {
        unsafe { LLVMDisposeModule(self.0) }
    }
}

/// Read-only function information, borrowing the owning module.
pub struct Function<'module>(Value<'module>);
impl Function<'_> {
    pub fn get_name(&self) -> &CStr {
        self.0.get_name()
    }
    pub fn get_linkage(&self) -> LLVMLinkage {
        self.0.get_linkage()
    }
    pub fn count_basic_blocks(&self) -> u32 {
        self.0.count_basic_blocks()
    }
}

pub struct TargetTriple(CString);
impl TargetTriple {
    pub fn as_str(&self) -> &CStr {
        &self.0
    }
}
pub struct TargetMachine(LLVMTargetMachineRef);
impl TargetMachine {
    pub(super) fn create(options: &Options) -> Result<Self> {
        static INITIALIZE: Once = Once::new();
        INITIALIZE.call_once(|| unsafe {
            LLVM_InitializeAllTargetInfos();
            LLVM_InitializeAllTargets();
            LLVM_InitializeAllTargetMCs();
            LLVM_InitializeAllAsmPrinters();
            LLVM_InitializeAllAsmParsers();
            LLVM_InitializeAllDisassemblers();
        });
        let triple = match &options.target {
            Some(t) => CString::new(t.as_str()).map_err(|_| error("target contains a NUL byte"))?,
            None => Self::get_default_triple().0,
        };
        let cpu = CString::new(options.cpu.as_deref().unwrap_or("generic"))
            .map_err(|_| error("CPU contains a NUL byte"))?;
        let features = CString::new(options.features.as_str())
            .map_err(|_| error("target features contain a NUL byte"))?;
        let mut target = ptr::null_mut();
        let mut msg = ptr::null_mut();
        unsafe {
            let failed = LLVMGetTargetFromTriple(triple.as_ptr(), &mut target, &mut msg);
            let msg = message(msg);
            if failed != 0 {
                return Err(error(msg));
            }
            let level = match options.optimization {
                0 => LLVMCodeGenOptLevel::LLVMCodeGenLevelNone,
                1 => LLVMCodeGenOptLevel::LLVMCodeGenLevelLess,
                2 => LLVMCodeGenOptLevel::LLVMCodeGenLevelDefault,
                _ => LLVMCodeGenOptLevel::LLVMCodeGenLevelAggressive,
            };
            let raw = LLVMCreateTargetMachine(
                target,
                triple.as_ptr(),
                cpu.as_ptr(),
                features.as_ptr(),
                level,
                LLVMRelocMode::LLVMRelocPIC,
                LLVMCodeModel::LLVMCodeModelDefault,
            );
            if raw.is_null() {
                Err(error("LLVM could not create the requested target machine"))
            } else {
                Ok(Self(raw))
            }
        }
    }
    pub fn get_default_triple() -> TargetTriple {
        TargetTriple(cstring(&unsafe { message(LLVMGetDefaultTargetTriple()) }))
    }
    pub(super) fn normalize_triple(triple: &TargetTriple) -> TargetTriple {
        TargetTriple(cstring(&unsafe {
            message(LLVMNormalizeTargetTriple(triple.0.as_ptr()))
        }))
    }
    pub fn get_triple(&self) -> TargetTriple {
        TargetTriple(cstring(&unsafe {
            message(LLVMGetTargetMachineTriple(self.0))
        }))
    }
    pub(super) fn get_target_data(&self) -> TargetData {
        TargetData(unsafe { LLVMCreateTargetDataLayout(self.0) })
    }
    pub fn write_to_file(&self, module: &Module<'_>, kind: FileType, path: &Path) -> Result<()> {
        let path = path_string(path)?;
        let mut msg = ptr::null_mut();
        let failed = unsafe {
            LLVMTargetMachineEmitToFile(self.0, module.0, path.as_ptr(), kind.raw(), &mut msg)
        };
        let msg = unsafe { message(msg) };
        if failed != 0 { Err(error(msg)) } else { Ok(()) }
    }
    pub fn write_to_memory_buffer(
        &self,
        module: &Module<'_>,
        kind: FileType,
    ) -> Result<MemoryBuffer> {
        let mut msg = ptr::null_mut();
        let mut buffer = ptr::null_mut();
        let failed = unsafe {
            LLVMTargetMachineEmitToMemoryBuffer(self.0, module.0, kind.raw(), &mut msg, &mut buffer)
        };
        let msg = unsafe { message(msg) };
        if failed != 0 {
            Err(error(msg))
        } else {
            Ok(MemoryBuffer(buffer))
        }
    }
}
impl Drop for TargetMachine {
    fn drop(&mut self) {
        unsafe { LLVMDisposeTargetMachine(self.0) }
    }
}
#[derive(Clone, Copy)]
pub enum FileType {
    Assembly,
    Object,
}
impl FileType {
    fn raw(self) -> LLVMCodeGenFileType {
        match self {
            Self::Assembly => LLVMCodeGenFileType::LLVMAssemblyFile,
            Self::Object => LLVMCodeGenFileType::LLVMObjectFile,
        }
    }
}

pub(super) struct TargetData(LLVMTargetDataRef);
impl TargetData {
    pub(super) fn create(layout: &str) -> Self {
        Self(unsafe { LLVMCreateTargetData(cstring(layout).as_ptr()) })
    }
    pub(super) fn get_data_layout(&self) -> TargetTriple {
        TargetTriple(cstring(&unsafe {
            message(LLVMCopyStringRepOfTargetData(self.0))
        }))
    }
    pub(super) fn get_pointer_byte_size(&self) -> u32 {
        unsafe { LLVMPointerSize(self.0) }
    }
    pub(super) fn get_abi_size(&self, ty: &LlvmType<'_>) -> u64 {
        unsafe { LLVMABISizeOfType(self.0, ty.0) }
    }
    pub(super) fn get_abi_alignment(&self, ty: &LlvmType<'_>) -> u32 {
        unsafe { LLVMABIAlignmentOfType(self.0, ty.0) }
    }
    pub(super) fn offset_of_element(&self, ty: &LlvmType<'_>, index: u32) -> Option<u64> {
        ty.get_field_type_at_index(index)
            .map(|_| unsafe { LLVMOffsetOfElement(self.0, ty.0, index) })
    }
}
impl Drop for TargetData {
    fn drop(&mut self) {
        unsafe { LLVMDisposeTargetData(self.0) }
    }
}

pub struct MemoryBuffer(LLVMMemoryBufferRef);
impl MemoryBuffer {
    pub fn as_slice(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(LLVMGetBufferStart(self.0).cast(), LLVMGetBufferSize(self.0))
        }
    }
    /// Inspect object sections and relocation symbols while keeping all LLVM
    /// object iterators private and disposing them before the buffer is released.
    pub fn sections(&self) -> Result<Vec<ObjectSection>> {
        use llvm_sys::object::*;
        unsafe {
            let mut msg = ptr::null_mut();
            let binary = LLVMCreateBinary(self.0, ptr::null_mut(), &mut msg);
            if binary.is_null() {
                return Err(error(message(msg)));
            }
            let section = LLVMObjectFileCopySectionIterator(binary);
            let mut result = Vec::new();
            if !section.is_null() {
                while LLVMObjectFileIsSectionIteratorAtEnd(binary, section) == 0 {
                    // ELF's null section has no name and LLVM returns null.
                    let name = LLVMGetSectionName(section);
                    let name = if name.is_null() {
                        String::new()
                    } else {
                        CStr::from_ptr(name).to_string_lossy().into_owned()
                    };
                    let size = LLVMGetSectionSize(section);
                    let reloc = LLVMGetRelocations(section);
                    let mut symbols = Vec::new();
                    if !reloc.is_null() {
                        while LLVMIsRelocationIteratorAtEnd(section, reloc) == 0 {
                            let symbol = LLVMGetRelocationSymbol(reloc);
                            if !symbol.is_null() {
                                let name =
                                    if LLVMObjectFileIsSymbolIteratorAtEnd(binary, symbol) == 0 {
                                        LLVMGetSymbolName(symbol)
                                    } else {
                                        ptr::null()
                                    };
                                if !name.is_null() {
                                    symbols
                                        .push(CStr::from_ptr(name).to_string_lossy().into_owned());
                                }
                                LLVMDisposeSymbolIterator(symbol);
                            }
                            LLVMMoveToNextRelocation(reloc);
                        }
                        LLVMDisposeRelocationIterator(reloc);
                    }
                    result.push(ObjectSection {
                        name,
                        size,
                        relocation_symbols: symbols,
                    });
                    LLVMMoveToNextSection(section);
                }
                LLVMDisposeSectionIterator(section);
            }
            LLVMDisposeBinary(binary);
            Ok(result)
        }
    }
}
impl Drop for MemoryBuffer {
    fn drop(&mut self) {
        unsafe { LLVMDisposeMemoryBuffer(self.0) }
    }
}
pub struct ObjectSection {
    pub name: String,
    pub size: u64,
    pub relocation_symbols: Vec<String>,
}

pub(super) struct Builder<'ctx>(LLVMBuilderRef, PhantomData<&'ctx Context>);
impl Drop for Builder<'_> {
    fn drop(&mut self) {
        unsafe { LLVMDisposeBuilder(self.0) }
    }
}

macro_rules! binary_ops {
    ($($name:ident => $llvm:ident),* $(,)?) => { $(
        pub(super) fn $name(&self, left: Value<'ctx>, right: Value<'ctx>, name: &str) -> Result<Value<'ctx>> {
            self.positioned()?;
            Ok(Value(unsafe { $llvm(self.0, left.0, right.0, cstring(name).as_ptr()) }, PhantomData))
        }
    )* };
}
macro_rules! unary_ops {
    ($($name:ident => $llvm:ident),* $(,)?) => { $(
        pub(super) fn $name(&self, value: Value<'ctx>, name: &str) -> Result<Value<'ctx>> {
            self.positioned()?;
            Ok(Value(unsafe { $llvm(self.0, value.0, cstring(name).as_ptr()) }, PhantomData))
        }
    )* };
}
macro_rules! cast_ops {
    ($($name:ident => $llvm:ident),* $(,)?) => { $(
        pub(super) fn $name(&self, value: Value<'ctx>, ty: LlvmType<'ctx>, name: &str) -> Result<Value<'ctx>> {
            self.positioned()?;
            Ok(Value(unsafe { $llvm(self.0, value.0, ty.0, cstring(name).as_ptr()) }, PhantomData))
        }
    )* };
}
impl<'ctx> Builder<'ctx> {
    fn positioned(&self) -> Result<()> {
        if self.get_insert_block().is_none() {
            Err(error("LLVM builder has no insertion block"))
        } else {
            Ok(())
        }
    }
    pub(super) fn position_at_end(&self, block: BasicBlock<'ctx>) {
        unsafe { LLVMPositionBuilderAtEnd(self.0, block.0) }
    }
    pub(super) fn position_before(&self, instruction: &Value<'ctx>) {
        unsafe { LLVMPositionBuilderBefore(self.0, instruction.0) }
    }
    pub(super) fn get_insert_block(&self) -> Option<BasicBlock<'ctx>> {
        BasicBlock::optional(unsafe { LLVMGetInsertBlock(self.0) })
    }
    binary_ops! {
        build_int_add => LLVMBuildAdd, build_int_sub => LLVMBuildSub, build_int_mul => LLVMBuildMul,
        build_int_signed_div => LLVMBuildSDiv, build_int_unsigned_div => LLVMBuildUDiv,
        build_int_signed_rem => LLVMBuildSRem, build_int_unsigned_rem => LLVMBuildURem,
        build_float_add => LLVMBuildFAdd, build_float_sub => LLVMBuildFSub, build_float_mul => LLVMBuildFMul,
        build_float_div => LLVMBuildFDiv, build_float_rem => LLVMBuildFRem,
        build_and => LLVMBuildAnd, build_or => LLVMBuildOr, build_xor => LLVMBuildXor,
        build_left_shift => LLVMBuildShl,
    }
    unary_ops! { build_not => LLVMBuildNot, build_float_neg => LLVMBuildFNeg, build_is_null => LLVMBuildIsNull }
    cast_ops! {
        build_int_z_extend => LLVMBuildZExt, build_signed_int_to_float => LLVMBuildSIToFP,
        build_unsigned_int_to_float => LLVMBuildUIToFP, build_float_to_signed_int => LLVMBuildFPToSI,
        build_float_to_unsigned_int => LLVMBuildFPToUI, build_float_cast => LLVMBuildFPCast,
        build_int_to_ptr => LLVMBuildIntToPtr, build_ptr_to_int => LLVMBuildPtrToInt,
    }
    pub(super) fn build_right_shift(
        &self,
        left: Value<'ctx>,
        right: Value<'ctx>,
        signed: bool,
        name: &str,
    ) -> Result<Value<'ctx>> {
        self.positioned()?;
        let op = if signed { LLVMBuildAShr } else { LLVMBuildLShr };
        Ok(Value(
            unsafe { op(self.0, left.0, right.0, cstring(name).as_ptr()) },
            PhantomData,
        ))
    }
    pub(super) fn build_int_compare(
        &self,
        predicate: LLVMIntPredicate,
        left: Value<'ctx>,
        right: Value<'ctx>,
        name: &str,
    ) -> Result<Value<'ctx>> {
        self.positioned()?;
        Ok(Value(
            unsafe { LLVMBuildICmp(self.0, predicate, left.0, right.0, cstring(name).as_ptr()) },
            PhantomData,
        ))
    }
    pub(super) fn build_float_compare(
        &self,
        predicate: LLVMRealPredicate,
        left: Value<'ctx>,
        right: Value<'ctx>,
        name: &str,
    ) -> Result<Value<'ctx>> {
        self.positioned()?;
        Ok(Value(
            unsafe { LLVMBuildFCmp(self.0, predicate, left.0, right.0, cstring(name).as_ptr()) },
            PhantomData,
        ))
    }
    pub(super) fn build_int_cast_sign_flag(
        &self,
        value: Value<'ctx>,
        ty: LlvmType<'ctx>,
        signed: bool,
        name: &str,
    ) -> Result<Value<'ctx>> {
        self.positioned()?;
        Ok(Value(
            unsafe {
                LLVMBuildIntCast2(self.0, value.0, ty.0, signed.into(), cstring(name).as_ptr())
            },
            PhantomData,
        ))
    }
    pub(super) fn build_alloca(&self, ty: LlvmType<'ctx>, name: &str) -> Result<Value<'ctx>> {
        self.positioned()?;
        Ok(Value(
            unsafe { LLVMBuildAlloca(self.0, ty.0, cstring(name).as_ptr()) },
            PhantomData,
        ))
    }
    pub(super) fn build_load(
        &self,
        ty: LlvmType<'ctx>,
        ptr: Value<'ctx>,
        name: &str,
    ) -> Result<Value<'ctx>> {
        self.positioned()?;
        Ok(Value(
            unsafe { LLVMBuildLoad2(self.0, ty.0, ptr.0, cstring(name).as_ptr()) },
            PhantomData,
        ))
    }
    pub(super) fn build_store(&self, ptr: Value<'ctx>, value: Value<'ctx>) -> Result<Value<'ctx>> {
        self.positioned()?;
        Ok(Value(
            unsafe { LLVMBuildStore(self.0, value.0, ptr.0) },
            PhantomData,
        ))
    }
    pub(super) fn build_struct_gep(
        &self,
        ty: LlvmType<'ctx>,
        ptr: Value<'ctx>,
        index: u32,
        name: &str,
    ) -> Result<Value<'ctx>> {
        self.positioned()?;
        if ty.get_field_type_at_index(index).is_none() {
            return Err(error("LLVM struct field index is out of bounds"));
        }
        Ok(Value(
            unsafe { LLVMBuildStructGEP2(self.0, ty.0, ptr.0, index, cstring(name).as_ptr()) },
            PhantomData,
        ))
    }
    pub(super) unsafe fn build_gep(
        &self,
        ty: LlvmType<'ctx>,
        ptr: Value<'ctx>,
        indices: &[Value<'ctx>],
        name: &str,
    ) -> Result<Value<'ctx>> {
        self.positioned()?;
        let mut indices: Vec<_> = indices.iter().map(|v| v.0).collect();
        Ok(Value(
            unsafe {
                LLVMBuildGEP2(
                    self.0,
                    ty.0,
                    ptr.0,
                    indices.as_mut_ptr(),
                    indices.len() as u32,
                    cstring(name).as_ptr(),
                )
            },
            PhantomData,
        ))
    }
    pub(super) fn build_extract_value(
        &self,
        value: Value<'ctx>,
        index: u32,
        name: &str,
    ) -> Result<Value<'ctx>> {
        self.positioned()?;
        Ok(Value(
            unsafe { LLVMBuildExtractValue(self.0, value.0, index, cstring(name).as_ptr()) },
            PhantomData,
        ))
    }
    pub(super) fn build_insert_value(
        &self,
        aggregate: Value<'ctx>,
        value: Value<'ctx>,
        index: u32,
        name: &str,
    ) -> Result<Value<'ctx>> {
        self.positioned()?;
        Ok(Value(
            unsafe {
                LLVMBuildInsertValue(self.0, aggregate.0, value.0, index, cstring(name).as_ptr())
            },
            PhantomData,
        ))
    }
    pub(super) fn build_call(
        &self,
        function: Value<'ctx>,
        args: &[Value<'ctx>],
        name: &str,
    ) -> Result<Value<'ctx>> {
        self.positioned()?;
        let mut args: Vec<_> = args.iter().map(|v| v.0).collect();
        let ty = function.get_type();
        let name = if unsafe { LLVMGetTypeKind(LLVMGetReturnType(ty.0)) }
            == LLVMTypeKind::LLVMVoidTypeKind
        {
            ""
        } else {
            name
        };
        Ok(Value(
            unsafe {
                LLVMBuildCall2(
                    self.0,
                    ty.0,
                    function.0,
                    args.as_mut_ptr(),
                    args.len() as u32,
                    cstring(name).as_ptr(),
                )
            },
            PhantomData,
        ))
    }
    pub(super) fn build_global_string_ptr(&self, text: &str, name: &str) -> Result<Value<'ctx>> {
        self.positioned()?;
        let text =
            CString::new(text).map_err(|_| error("LLVM global string contains a NUL byte"))?;
        Ok(Value(
            unsafe { LLVMBuildGlobalString(self.0, text.as_ptr(), cstring(name).as_ptr()) },
            PhantomData,
        ))
    }
    pub(super) fn build_return(&self, value: Option<&Value<'ctx>>) -> Result<Value<'ctx>> {
        self.positioned()?;
        Ok(Value(
            unsafe {
                match value {
                    Some(v) => LLVMBuildRet(self.0, v.0),
                    None => LLVMBuildRetVoid(self.0),
                }
            },
            PhantomData,
        ))
    }
    pub(super) fn build_unreachable(&self) -> Result<Value<'ctx>> {
        self.positioned()?;
        Ok(Value(unsafe { LLVMBuildUnreachable(self.0) }, PhantomData))
    }
    pub(super) fn build_unconditional_branch(
        &self,
        block: BasicBlock<'ctx>,
    ) -> Result<Value<'ctx>> {
        self.positioned()?;
        Ok(Value(unsafe { LLVMBuildBr(self.0, block.0) }, PhantomData))
    }
    pub(super) fn build_conditional_branch(
        &self,
        cond: Value<'ctx>,
        yes: BasicBlock<'ctx>,
        no: BasicBlock<'ctx>,
    ) -> Result<Value<'ctx>> {
        self.positioned()?;
        Ok(Value(
            unsafe { LLVMBuildCondBr(self.0, cond.0, yes.0, no.0) },
            PhantomData,
        ))
    }
    pub(super) fn build_switch(
        &self,
        value: Value<'ctx>,
        default: BasicBlock<'ctx>,
        cases: &[(Value<'ctx>, BasicBlock<'ctx>)],
    ) -> Result<Value<'ctx>> {
        self.positioned()?;
        let switch = unsafe { LLVMBuildSwitch(self.0, value.0, default.0, cases.len() as u32) };
        for (value, block) in cases {
            unsafe { LLVMAddCase(switch, value.0, block.0) }
        }
        Ok(Value(switch, PhantomData))
    }
    pub(super) fn build_phi(&self, ty: LlvmType<'ctx>, name: &str) -> Result<Value<'ctx>> {
        self.positioned()?;
        Ok(Value(
            unsafe { LLVMBuildPhi(self.0, ty.0, cstring(name).as_ptr()) },
            PhantomData,
        ))
    }
    pub(super) fn build_atomicrmw(
        &self,
        op: LLVMAtomicRMWBinOp,
        ptr: Value<'ctx>,
        value: Value<'ctx>,
        order: LLVMAtomicOrdering,
    ) -> Result<Value<'ctx>> {
        self.positioned()?;
        Ok(Value(
            unsafe { LLVMBuildAtomicRMW(self.0, op, ptr.0, value.0, order, 0) },
            PhantomData,
        ))
    }
    pub(super) fn build_cmpxchg(
        &self,
        ptr: Value<'ctx>,
        expected: Value<'ctx>,
        value: Value<'ctx>,
        success: LLVMAtomicOrdering,
        failure: LLVMAtomicOrdering,
    ) -> Result<Value<'ctx>> {
        self.positioned()?;
        Ok(Value(
            unsafe {
                LLVMBuildAtomicCmpXchg(self.0, ptr.0, expected.0, value.0, success, failure, 0)
            },
            PhantomData,
        ))
    }
    pub(super) fn build_memcpy(
        &self,
        dst: Value<'ctx>,
        dst_align: u32,
        src: Value<'ctx>,
        src_align: u32,
        size: Value<'ctx>,
    ) -> Result<Value<'ctx>> {
        self.positioned()?;
        Ok(Value(
            unsafe { LLVMBuildMemCpy(self.0, dst.0, dst_align, src.0, src_align, size.0) },
            PhantomData,
        ))
    }
    pub(super) fn build_memmove(
        &self,
        dst: Value<'ctx>,
        dst_align: u32,
        src: Value<'ctx>,
        src_align: u32,
        size: Value<'ctx>,
    ) -> Result<Value<'ctx>> {
        self.positioned()?;
        Ok(Value(
            unsafe { LLVMBuildMemMove(self.0, dst.0, dst_align, src.0, src_align, size.0) },
            PhantomData,
        ))
    }
    pub(super) fn build_memset(
        &self,
        dst: Value<'ctx>,
        align: u32,
        value: Value<'ctx>,
        size: Value<'ctx>,
    ) -> Result<Value<'ctx>> {
        self.positioned()?;
        Ok(Value(
            unsafe { LLVMBuildMemSet(self.0, dst.0, value.0, size.0, align) },
            PhantomData,
        ))
    }
}
