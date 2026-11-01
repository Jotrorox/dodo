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
/// Errors LLVM reports through the context while generating code, such as an
/// inline assembly template the target's assembler rejects. Without a handler
/// LLVM prints them and exits the process.
type Diagnostics = std::cell::RefCell<Vec<String>>;
extern "C" fn collect_diagnostic(info: LLVMDiagnosticInfoRef, sink: *mut std::ffi::c_void) {
    unsafe {
        if LLVMGetDiagInfoSeverity(info) == LLVMDiagnosticSeverity::LLVMDSError {
            let text = message(LLVMGetDiagInfoDescription(info));
            (*sink.cast::<Diagnostics>()).borrow_mut().push(text);
        }
    }
}
impl Context {
    pub fn create() -> Self {
        let context = unsafe { LLVMContextCreate() };
        let sink = Box::into_raw(Box::new(Diagnostics::default()));
        unsafe { LLVMContextSetDiagnosticHandler(context, Some(collect_diagnostic), sink.cast()) };
        Self(context)
    }
    /// Take the errors LLVM reported since the last call.
    fn take_errors(context: LLVMContextRef) -> Vec<String> {
        let sink = unsafe { LLVMContextGetDiagnosticContext(context) }.cast::<Diagnostics>();
        if sink.is_null() {
            return vec![];
        }
        std::mem::take(&mut *unsafe { &*sink }.borrow_mut())
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
    pub(super) fn create_type_attribute<'ctx>(
        &'ctx self,
        kind: u32,
        ty: LlvmType<'ctx>,
    ) -> Attribute<'ctx> {
        Attribute(
            unsafe { LLVMCreateTypeAttribute(self.0, kind, ty.0) },
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
        unsafe {
            let sink = LLVMContextGetDiagnosticContext(self.0).cast::<Diagnostics>();
            LLVMContextDispose(self.0);
            if !sink.is_null() {
                drop(Box::from_raw(sink));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_constants_are_canonical_at_their_llvm_width() {
        let context = Context::create();
        for bits in [1, 8, 16, 32, 64] {
            let ty = context
                .custom_width_int_type(std::num::NonZeroU32::new(bits).unwrap())
                .unwrap();
            let mask = u64::MAX >> (64 - bits);
            for value in [0, 1, 127, 128, u64::MAX - 127, u64::MAX] {
                for sign in [false, true] {
                    let constant = ty.const_int(value, sign);
                    // Printed IR can hide noncanonical high bits. Check the
                    // stored value and LLVM's uniquing identity directly.
                    assert_eq!(
                        unsafe { LLVMConstIntGetZExtValue(constant.0) },
                        value & mask
                    );
                    assert_eq!(constant.0, ty.const_int(value & mask, false).0);
                }
            }
        }
        let wide = context
            .custom_width_int_type(std::num::NonZeroU32::new(128).unwrap())
            .unwrap();
        assert_eq!(wide.const_int(u64::MAX, true).0, wide.const_all_ones().0);
        assert_ne!(wide.const_int(u64::MAX, false).0, wide.const_all_ones().0);
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
        // LLVM 23 requires the input to fit the integer width. In particular,
        // negative pattern bounds and folded constants arrive as u64 bit
        // patterns; passing their high bits through creates noncanonical APInts
        // in release LLVM builds and can make SimplifyCFG allocate indefinitely.
        let bits = unsafe { LLVMGetIntTypeWidth(self.0) };
        let (value, sign) = if bits < u64::BITS {
            (value & ((1u64 << bits) - 1), false)
        } else {
            (value, sign)
        };
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

unsafe fn large_aggregate(data: &TargetData, ty: LLVMTypeRef, limit: u64) -> bool {
    unsafe {
        matches!(
            LLVMGetTypeKind(ty),
            LLVMTypeKind::LLVMStructTypeKind | LLVMTypeKind::LLVMArrayTypeKind
        ) && LLVMABISizeOfType(data.0, ty) > limit
    }
}
unsafe fn large_aggregate_load(data: &TargetData, value: LLVMValueRef, limit: u64) -> bool {
    unsafe {
        !LLVMIsALoadInst(value).is_null()
            && LLVMGetVolatile(value) == 0
            && LLVMGetOrdering(value) == LLVMAtomicOrdering::LLVMAtomicOrderingNotAtomic
            && large_aggregate(data, LLVMTypeOf(value), limit)
    }
}
// The alloca a pointer addresses through constant-offset GEPs, if any.
unsafe fn underlying_alloca(mut pointer: LLVMValueRef) -> Option<LLVMValueRef> {
    unsafe {
        while !LLVMIsAGetElementPtrInst(pointer).is_null() {
            pointer = LLVMGetOperand(pointer, 0);
        }
        (!LLVMIsAAllocaInst(pointer).is_null()).then_some(pointer)
    }
}
// Whether `later`, in the same block as `earlier`, sees the memory at
// `source` that `earlier` saw. Instructions in between may write only to
// allocas other than the one holding `source`.
unsafe fn no_writes_between(
    earlier: LLVMValueRef,
    later: LLVMValueRef,
    source: LLVMValueRef,
) -> bool {
    unsafe {
        if LLVMGetInstructionParent(earlier) != LLVMGetInstructionParent(later) {
            return false;
        }
        let source = underlying_alloca(source);
        let elsewhere = |pointer: LLVMValueRef| {
            source.is_some() && underlying_alloca(pointer).is_some_and(|a| Some(a) != source)
        };
        let mut next = LLVMGetNextInstruction(earlier);
        while !next.is_null() && next != later {
            let safe = match LLVMGetInstructionOpcode(next) {
                LLVMOpcode::LLVMGetElementPtr
                | LLVMOpcode::LLVMBitCast
                | LLVMOpcode::LLVMAlloca
                | LLVMOpcode::LLVMExtractValue
                | LLVMOpcode::LLVMICmp
                | LLVMOpcode::LLVMLoad => true,
                LLVMOpcode::LLVMStore => elsewhere(LLVMGetOperand(next, 1)),
                LLVMOpcode::LLVMCall => {
                    // memset and memmove/memcpy write only their first operand.
                    let callee = LLVMGetCalledValue(next);
                    let mut length = 0;
                    let name = LLVMGetValueName2(callee, &mut length);
                    let name = std::slice::from_raw_parts(name.cast::<u8>(), length);
                    (name.starts_with(b"llvm.memset")
                        || name.starts_with(b"llvm.memmove")
                        || name.starts_with(b"llvm.memcpy"))
                        && elsewhere(LLVMGetOperand(next, 0))
                }
                _ => false,
            };
            if !safe {
                return false;
            }
            next = LLVMGetNextInstruction(next);
        }
        next == later
    }
}
// Stores a constant aggregate without whole-aggregate operations: zero as
// memset, undefined parts not at all, and other parts field by field.
unsafe fn store_constant(
    builder: LLVMBuilderRef,
    data: &TargetData,
    pointer: LLVMValueRef,
    value: LLVMValueRef,
    limit: u64,
) {
    unsafe {
        let ty = LLVMTypeOf(value);
        if LLVMIsUndef(value) != 0 || !LLVMIsAPoisonValue(value).is_null() {
            return;
        }
        if !large_aggregate(data, ty, limit) {
            LLVMBuildStore(builder, value, pointer);
            return;
        }
        let context = LLVMGetTypeContext(ty);
        if LLVMIsNull(value) != 0 {
            let size = LLVMConstInt(
                LLVMIntTypeInContext(context, data.get_pointer_byte_size() * 8),
                LLVMABISizeOfType(data.0, ty),
                0,
            );
            LLVMBuildMemSet(
                builder,
                pointer,
                LLVMConstInt(LLVMInt8TypeInContext(context), 0, 0),
                size,
                1,
            );
            return;
        }
        let count = if LLVMGetTypeKind(ty) == LLVMTypeKind::LLVMStructTypeKind {
            LLVMCountStructElementTypes(ty) as u64
        } else {
            LLVMGetArrayLength2(ty)
        };
        let i32_type = LLVMInt32TypeInContext(context);
        for index in 0..count {
            let element = LLVMGetAggregateElement(value, index as u32);
            if element.is_null() {
                // Not decomposable; keep the plain store.
                LLVMBuildStore(builder, value, pointer);
                return;
            }
            let mut indices = [
                LLVMConstInt(i32_type, 0, 0),
                LLVMConstInt(i32_type, index, 0),
            ];
            let field = LLVMBuildInBoundsGEP2(
                builder,
                ty,
                pointer,
                indices.as_mut_ptr(),
                2,
                c"field".as_ptr(),
            );
            store_constant(builder, data, field, element, limit);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Value<'ctx>(LLVMValueRef, PhantomData<&'ctx Context>);
impl<'ctx> Value<'ctx> {
    fn optional(raw: LLVMValueRef) -> Option<Self> {
        (!raw.is_null()).then_some(Self(raw, PhantomData))
    }
    pub(super) fn loaded_pointer(self, builder: &Builder<'ctx>) -> Option<Self> {
        unsafe {
            if LLVMIsALoadInst(self.0).is_null()
                || LLVMGetVolatile(self.0) != 0
                || LLVMGetOrdering(self.0) != LLVMAtomicOrdering::LLVMAtomicOrderingNotAtomic
            {
                return None;
            }
            // Reloading is only equivalent while the source is unchanged.
            // Cleanup calls, stores, and control flow must keep the SSA value
            // captured by the original load, even when it is a large record.
            if LLVMGetInstructionParent(self.0) != LLVMGetInsertBlock(builder.0) {
                return None;
            }
            let mut next = LLVMGetNextInstruction(self.0);
            while !next.is_null() {
                if !matches!(
                    LLVMGetInstructionOpcode(next),
                    LLVMOpcode::LLVMGetElementPtr
                        | LLVMOpcode::LLVMBitCast
                        | LLVMOpcode::LLVMAlloca
                ) {
                    return None;
                }
                next = LLVMGetNextInstruction(next);
            }
            Some(Value(LLVMGetOperand(self.0, 0), PhantomData))
        }
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
    /// The initializer of a defined global variable; none for a declaration.
    pub(super) fn get_initializer(self) -> Option<Self> {
        Self::optional(unsafe { LLVMGetInitializer(self.0) })
    }
    pub(super) fn is_global_constant(self) -> bool {
        unsafe { LLVMIsGlobalConstant(self.0) != 0 }
    }
    /// Whether the global's value type holds a pointer anywhere, so its
    /// initializer may need relocations.
    pub(super) fn value_type_has_pointer(self) -> bool {
        unsafe fn has_pointer(ty: LLVMTypeRef) -> bool {
            unsafe {
                match LLVMGetTypeKind(ty) {
                    LLVMTypeKind::LLVMPointerTypeKind => true,
                    LLVMTypeKind::LLVMArrayTypeKind | LLVMTypeKind::LLVMVectorTypeKind => {
                        has_pointer(LLVMGetElementType(ty))
                    }
                    LLVMTypeKind::LLVMStructTypeKind => (0..LLVMCountStructElementTypes(ty))
                        .any(|i| has_pointer(LLVMStructGetTypeAtIndex(ty, i))),
                    _ => false,
                }
            }
        }
        unsafe { has_pointer(LLVMGlobalGetValueType(self.0)) }
    }
    pub(super) fn is_thread_local(self) -> bool {
        unsafe { LLVMIsThreadLocal(self.0) != 0 }
    }
    pub(super) fn has_section(self) -> bool {
        let section = unsafe { LLVMGetSection(self.0) };
        !section.is_null() && unsafe { *section } != 0
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
    /// The block's address (`blockaddress`), a target of `indirectbr`.
    pub(super) fn address(self, function: Value<'ctx>) -> Value<'ctx> {
        Value(unsafe { LLVMBlockAddress(function.0, self.0) }, PhantomData)
    }
}

pub struct Module<'ctx>(LLVMModuleRef, PhantomData<&'ctx Context>);
impl<'ctx> Module<'ctx> {
    /// Parse textual LLVM IR, such as an embedded board runtime.
    pub(super) fn parse_ir(context: &'ctx Context, name: &str, text: &str) -> Result<Self> {
        let name = cstring(name);
        unsafe {
            let buffer = LLVMCreateMemoryBufferWithMemoryRangeCopy(
                text.as_ptr().cast(),
                text.len(),
                name.as_ptr(),
            );
            let mut module = ptr::null_mut();
            let mut msg = ptr::null_mut();
            let failed = ir_reader::LLVMParseIRInContext2(context.0, buffer, &mut module, &mut msg);
            LLVMDisposeMemoryBuffer(buffer);
            let msg = message(msg);
            if failed != 0 {
                Err(error(format!(
                    "invalid LLVM IR in {}: {msg}",
                    name.to_string_lossy()
                )))
            } else {
                Ok(Self(module, PhantomData))
            }
        }
    }
    /// Errors LLVM reported while emitting this module's code.
    fn emission_errors(&self) -> Result<()> {
        let errors = Context::take_errors(unsafe { LLVMGetModuleContext(self.0) });
        if errors.is_empty() {
            Ok(())
        } else {
            Err(error(errors.join("\n")))
        }
    }
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
    pub(super) fn globals(&self) -> impl Iterator<Item = Value<'ctx>> {
        let first = Value::optional(unsafe { LLVMGetFirstGlobal(self.0) });
        std::iter::successors(first, |v| {
            Value::optional(unsafe { LLVMGetNextGlobal(v.0) })
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
    /// Rewrites whole-value operations on aggregates larger than `limit`
    /// bytes. Instruction selection and scalar replacement split such values
    /// into one operation per element, costing minutes and gigabytes of
    /// compiler memory, and megabytes of stack at -O0. Constant stores become
    /// memset or field stores. A loaded value's store becomes a memmove when
    /// nothing in between can write its source; otherwise the load becomes a
    /// memcpy into a stack copy that the stores read. A field extraction
    /// becomes a field load, and a load left without uses is erased. Loads
    /// with other uses stay.
    pub(super) fn lower_large_aggregates(&self, data: &TargetData, limit: u64) {
        unsafe {
            let context = LLVMGetModuleContext(self.0);
            let builder = LLVMCreateBuilderInContext(context);
            let instructions = |keep: &dyn Fn(LLVMValueRef) -> bool| {
                let mut found = Vec::new();
                for function in self.functions() {
                    let mut block = LLVMGetFirstBasicBlock(function.0);
                    while !block.is_null() {
                        let mut instruction = LLVMGetFirstInstruction(block);
                        while !instruction.is_null() {
                            if keep(instruction) {
                                found.push(instruction);
                            }
                            instruction = LLVMGetNextInstruction(instruction);
                        }
                        block = LLVMGetNextBasicBlock(block);
                    }
                }
                found
            };
            let constant_stores = instructions(&|store| {
                !LLVMIsAStoreInst(store).is_null()
                    && LLVMGetVolatile(store) == 0
                    && LLVMIsConstant(LLVMGetOperand(store, 0)) != 0
                    && large_aggregate(data, LLVMTypeOf(LLVMGetOperand(store, 0)), limit)
            });
            for store in constant_stores {
                LLVMPositionBuilderBefore(builder, store);
                store_constant(
                    builder,
                    data,
                    LLVMGetOperand(store, 1),
                    LLVMGetOperand(store, 0),
                    limit,
                );
                LLVMInstructionEraseFromParent(store);
            }
            let mut work = instructions(&|load| large_aggregate_load(data, load, limit));
            let i32_type = LLVMInt32TypeInContext(context);
            while let Some(load) = work.pop() {
                let source = LLVMGetOperand(load, 0);
                let mut users = Vec::new();
                let mut use_ = LLVMGetFirstUse(load);
                while !use_.is_null() {
                    users.push(LLVMGetUser(use_));
                    use_ = LLVMGetNextUse(use_);
                }
                let movable = |user: LLVMValueRef| {
                    if !LLVMIsAExtractValueInst(user).is_null() {
                        return LLVMGetOperand(user, 0) == load;
                    }
                    !LLVMIsAStoreInst(user).is_null()
                        && LLVMGetVolatile(user) == 0
                        && LLVMGetOperand(user, 0) == load
                        && LLVMGetOperand(user, 1) != load
                };
                if !users.iter().all(|&user| movable(user)) {
                    continue;
                }
                let ty = LLVMTypeOf(load);
                let size = LLVMConstInt(
                    LLVMIntTypeInContext(context, data.get_pointer_byte_size() * 8),
                    LLVMABISizeOfType(data.0, ty),
                    0,
                );
                // When something between the load and a store may write the
                // source, such as the drops after a Result is built, copy the
                // value aside where it was loaded and read the copy instead.
                // Left as a load, scalar replacement would split it into one
                // operation per byte.
                let direct = users.iter().all(|&user| {
                    !LLVMIsAExtractValueInst(user).is_null()
                        || no_writes_between(load, user, source)
                });
                let source = if direct {
                    source
                } else {
                    let function = LLVMGetBasicBlockParent(LLVMGetInstructionParent(load));
                    let entry = LLVMGetEntryBasicBlock(function);
                    let first = LLVMGetFirstInstruction(entry);
                    if first.is_null() {
                        LLVMPositionBuilderAtEnd(builder, entry);
                    } else {
                        LLVMPositionBuilderBefore(builder, first);
                    }
                    let snapshot = LLVMBuildAlloca(builder, ty, c"snapshot".as_ptr());
                    LLVMPositionBuilderBefore(builder, load);
                    LLVMBuildMemCpy(builder, snapshot, 1, source, 1, size);
                    snapshot
                };
                for user in users {
                    if LLVMIsAStoreInst(user).is_null() {
                        // Read the field where the whole value was read.
                        LLVMPositionBuilderBefore(builder, load);
                        let count = LLVMGetNumIndices(user) as usize;
                        let indices = std::slice::from_raw_parts(LLVMGetIndices(user), count);
                        let mut gep = vec![LLVMConstInt(i32_type, 0, 0)];
                        gep.extend(indices.iter().map(|&i| LLVMConstInt(i32_type, i as u64, 0)));
                        let field = LLVMBuildInBoundsGEP2(
                            builder,
                            ty,
                            source,
                            gep.as_mut_ptr(),
                            gep.len() as u32,
                            c"field".as_ptr(),
                        );
                        let narrow =
                            LLVMBuildLoad2(builder, LLVMTypeOf(user), field, c"field".as_ptr());
                        LLVMReplaceAllUsesWith(user, narrow);
                        LLVMInstructionEraseFromParent(user);
                        if large_aggregate_load(data, narrow, limit) {
                            work.push(narrow);
                        }
                    } else {
                        LLVMPositionBuilderBefore(builder, user);
                        LLVMBuildMemMove(builder, LLVMGetOperand(user, 1), 1, source, 1, size);
                        LLVMInstructionEraseFromParent(user);
                    }
                }
                LLVMInstructionEraseFromParent(load);
            }
            LLVMDisposeBuilder(builder);
        }
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
        // ARMv6-M has no exclusive accesses, but its aligned loads and stores
        // of up to 32 bits are single-copy atomic. Without this feature LLVM
        // calls libatomic even for those; with it they become `ldr`/`str`
        // and `dmb`. Codegen rejects the read-modify-write operations, which
        // would otherwise become `__sync` calls (see `atomic_support`).
        let mut features = options.features.clone();
        let triple_text = triple.to_string_lossy();
        if triple_text.starts_with("thumbv6m") && !features.contains("atomics-32") {
            if !features.is_empty() {
                features.push(',');
            }
            features.push_str("+atomics-32");
        }
        let features =
            CString::new(features).map_err(|_| error("target features contain a NUL byte"))?;
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
        if failed != 0 {
            return Err(error(msg));
        }
        module.emission_errors()
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
            return Err(error(msg));
        }
        let buffer = MemoryBuffer(buffer);
        module.emission_errors()?;
        Ok(buffer)
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
            let name = unsafe { LLVMGetStructName(ty.0) };
            let name = if name.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(name) }
                    .to_string_lossy()
                    .into_owned()
            };
            return Err(error(format!(
                "LLVM struct field index {index} is out of bounds for `{name}`"
            )));
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
    /// Call through a function pointer of type `ty`.
    pub(super) fn build_indirect_call(
        &self,
        ty: LlvmType<'ctx>,
        function: Value<'ctx>,
        args: &[Value<'ctx>],
        name: &str,
    ) -> Result<Value<'ctx>> {
        self.positioned()?;
        let mut args: Vec<_> = args.iter().map(|v| v.0).collect();
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
    pub(super) fn build_indirect_br(
        &self,
        address: Value<'ctx>,
        destinations: &[BasicBlock<'ctx>],
    ) -> Result<Value<'ctx>> {
        self.positioned()?;
        let branch = unsafe { LLVMBuildIndirectBr(self.0, address.0, destinations.len() as u32) };
        for block in destinations {
            unsafe { LLVMAddDestination(branch, block.0) }
        }
        Ok(Value(branch, PhantomData))
    }
    pub(super) fn build_phi(&self, ty: LlvmType<'ctx>, name: &str) -> Result<Value<'ctx>> {
        self.positioned()?;
        Ok(Value(
            unsafe { LLVMBuildPhi(self.0, ty.0, cstring(name).as_ptr()) },
            PhantomData,
        ))
    }
    pub(super) fn build_fence(&self, order: LLVMAtomicOrdering) -> Result<Value<'ctx>> {
        self.positioned()?;
        Ok(Value(
            unsafe { LLVMBuildFence(self.0, order, 0, c"".as_ptr()) },
            PhantomData,
        ))
    }
    /// Calls side-effecting inline assembly of type `ty`; the constraints must
    /// declare every clobber, including `~{memory}` for compiler barriers.
    pub(super) fn build_inline_asm(
        &self,
        ty: LlvmType<'ctx>,
        assembly: &str,
        constraints: &str,
        args: &[Value<'ctx>],
        name: &str,
    ) -> Result<Value<'ctx>> {
        self.build_inline_asm_with(ty, assembly, constraints, args, name, true)
    }
    /// Inline assembly that LLVM may remove or merge unless `side_effects`.
    pub(super) fn build_inline_asm_with(
        &self,
        ty: LlvmType<'ctx>,
        assembly: &str,
        constraints: &str,
        args: &[Value<'ctx>],
        name: &str,
        side_effects: bool,
    ) -> Result<Value<'ctx>> {
        let asm = unsafe {
            LLVMGetInlineAsm(
                ty.0,
                assembly.as_ptr().cast_mut().cast(),
                assembly.len(),
                constraints.as_ptr().cast_mut().cast(),
                constraints.len(),
                side_effects as LLVMBool,
                0,
                LLVMInlineAsmDialect::LLVMInlineAsmDialectATT,
                0,
            )
        };
        self.positioned()?;
        let mut args: Vec<_> = args.iter().map(|v| v.0).collect();
        let returns =
            unsafe { LLVMGetTypeKind(LLVMGetReturnType(ty.0)) } != LLVMTypeKind::LLVMVoidTypeKind;
        Ok(Value(
            unsafe {
                LLVMBuildCall2(
                    self.0,
                    ty.0,
                    asm,
                    args.as_mut_ptr(),
                    args.len() as u32,
                    cstring(if returns { name } else { "" }).as_ptr(),
                )
            },
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

/// Rewriting a generated function into a resumable state machine: its entry
/// block gains edges to the resume points, and its stack slots move into the
/// suspended frame. See `Codegen::async_frames`.
impl<'ctx> Value<'ctx> {
    pub(super) fn basic_blocks(self) -> Vec<BasicBlock<'ctx>> {
        let mut blocks = Vec::new();
        let mut block = unsafe { LLVMGetFirstBasicBlock(self.0) };
        while !block.is_null() {
            blocks.push(BasicBlock(block, PhantomData));
            block = unsafe { LLVMGetNextBasicBlock(block) };
        }
        blocks
    }
    pub(super) fn is_alloca(self) -> bool {
        unsafe { !LLVMIsAAllocaInst(self.0).is_null() }
    }
    pub(super) fn allocated_type(self) -> LlvmType<'ctx> {
        LlvmType(unsafe { LLVMGetAllocatedType(self.0) }, PhantomData)
    }
    pub(super) fn replace_all_uses_with(self, other: Self) {
        unsafe { LLVMReplaceAllUsesWith(self.0, other.0) }
    }
    pub(super) fn erase_instruction(self) {
        unsafe { LLVMInstructionEraseFromParent(self.0) }
    }
    /// Demote every instruction result that is used where its definition no
    /// longer dominates the use: through memory from `slot(type)`, stored
    /// right after the definition and loaded before each such use. In a
    /// state machine such a use is reached through a resume point, so the
    /// value comes from an earlier call; its slot must persist between them.
    pub(super) fn repair_dominance(
        self,
        builder: &Builder<'ctx>,
        mut slot: impl FnMut(LlvmType<'ctx>) -> Result<Value<'ctx>>,
    ) -> Result<()> {
        let blocks = self.basic_blocks();
        let index: std::collections::HashMap<LLVMBasicBlockRef, usize> =
            blocks.iter().enumerate().map(|(i, b)| (b.0, i)).collect();
        let successors: Vec<Vec<usize>> = blocks
            .iter()
            .map(|block| unsafe {
                let terminator = LLVMGetBasicBlockTerminator(block.0);
                if terminator.is_null() {
                    return vec![];
                }
                (0..LLVMGetNumSuccessors(terminator))
                    .map(|i| index[&LLVMGetSuccessor(terminator, i)])
                    .collect()
            })
            .collect();
        // Reverse postorder from the entry; unreachable blocks keep None.
        let mut order = Vec::new();
        let mut visited = vec![false; blocks.len()];
        let mut stack = vec![(0usize, 0usize)];
        visited[0] = true;
        while let Some((block, next)) = stack.pop() {
            if next < successors[block].len() {
                stack.push((block, next + 1));
                let successor = successors[block][next];
                if !visited[successor] {
                    visited[successor] = true;
                    stack.push((successor, 0));
                }
            } else {
                order.push(block);
            }
        }
        order.reverse();
        let mut rank = vec![usize::MAX; blocks.len()];
        for (position, block) in order.iter().enumerate() {
            rank[*block] = position;
        }
        let mut predecessors = vec![vec![]; blocks.len()];
        for (block, targets) in successors.iter().enumerate() {
            if rank[block] != usize::MAX {
                for target in targets {
                    predecessors[*target].push(block);
                }
            }
        }
        // Cooper, Harvey, and Kennedy's iterative dominator computation.
        let mut idom = vec![usize::MAX; blocks.len()];
        idom[0] = 0;
        let mut changed = true;
        while changed {
            changed = false;
            for &block in order.iter().skip(1) {
                let mut new = usize::MAX;
                for &p in &predecessors[block] {
                    if idom[p] == usize::MAX {
                        continue;
                    }
                    new = if new == usize::MAX {
                        p
                    } else {
                        let (mut a, mut b) = (p, new);
                        while a != b {
                            while rank[a] > rank[b] {
                                a = idom[a];
                            }
                            while rank[b] > rank[a] {
                                b = idom[b];
                            }
                        }
                        a
                    };
                }
                if new != idom[block] {
                    idom[block] = new;
                    changed = true;
                }
            }
        }
        let dominates = |a: usize, mut b: usize| loop {
            if a == b {
                return true;
            }
            if b == 0 || idom[b] == usize::MAX {
                return false;
            }
            b = idom[b];
        };
        let reachable = |block: usize| rank[block] != usize::MAX;
        // Collect first: demotion inserts instructions.
        let mut definitions = Vec::new();
        for (number, block) in blocks.iter().enumerate() {
            if !reachable(number) {
                continue;
            }
            let mut instruction = unsafe { LLVMGetFirstInstruction(block.0) };
            while !instruction.is_null() {
                definitions.push((instruction, number));
                instruction = unsafe { LLVMGetNextInstruction(instruction) };
            }
        }
        for (definition, home) in definitions {
            let ty = unsafe { LLVMTypeOf(definition) };
            if unsafe { LLVMGetTypeKind(ty) } == LLVMTypeKind::LLVMVoidTypeKind {
                continue;
            }
            // Operands (user, operand index, block of the use) not dominated.
            let mut stale = Vec::new();
            let mut user_use = unsafe { LLVMGetFirstUse(definition) };
            let mut users = Vec::new();
            while !user_use.is_null() {
                let user = unsafe { LLVMGetUser(user_use) };
                if !users.contains(&user) {
                    users.push(user);
                }
                user_use = unsafe { LLVMGetNextUse(user_use) };
            }
            for user in users {
                if unsafe { LLVMIsAInstruction(user) }.is_null() {
                    continue;
                }
                let phi = unsafe { !LLVMIsAPHINode(user).is_null() };
                let parent = unsafe { LLVMGetInstructionParent(user) };
                for operand in 0..unsafe { LLVMGetNumOperands(user) } as u32 {
                    if unsafe { LLVMGetOperand(user, operand) } != definition {
                        continue;
                    }
                    let place = if phi {
                        unsafe { LLVMGetIncomingBlock(user, operand) }
                    } else {
                        parent
                    };
                    let Some(&block) = index.get(&place) else {
                        continue;
                    };
                    if reachable(block) && !dominates(home, block) {
                        stale.push((user, operand, phi, place));
                    }
                }
            }
            if stale.is_empty() {
                continue;
            }
            let memory = slot(LlvmType(ty, PhantomData))?;
            unsafe {
                // After the definition, and after every phi of its block.
                let mut after = LLVMGetNextInstruction(definition);
                while !after.is_null() && !LLVMIsAPHINode(after).is_null() {
                    after = LLVMGetNextInstruction(after);
                }
                if after.is_null() {
                    LLVMPositionBuilderAtEnd(builder.0, blocks[home].0);
                } else {
                    LLVMPositionBuilderBefore(builder.0, after);
                }
                LLVMBuildStore(builder.0, definition, memory.0);
                for (user, operand, phi, place) in stale {
                    if phi {
                        LLVMPositionBuilderBefore(builder.0, LLVMGetBasicBlockTerminator(place));
                    } else {
                        LLVMPositionBuilderBefore(builder.0, user);
                    }
                    let reloaded =
                        LLVMBuildLoad2(builder.0, ty, memory.0, cstring("resumed").as_ptr());
                    LLVMSetOperand(user, operand, reloaded);
                }
            }
        }
        Ok(())
    }
}
impl<'ctx> BasicBlock<'ctx> {
    pub(super) fn instructions(self) -> Vec<Value<'ctx>> {
        let mut instructions = Vec::new();
        let mut instruction = unsafe { LLVMGetFirstInstruction(self.0) };
        while !instruction.is_null() {
            instructions.push(Value(instruction, PhantomData));
            instruction = unsafe { LLVMGetNextInstruction(instruction) };
        }
        instructions
    }
}
impl<'ctx> Value<'ctx> {
    /// Which of `slots`, stack slots of this function, carry a value from
    /// one call to the next: those live where a `resume` block starts, that
    /// is, read on some path from it before being overwritten. `ignored`
    /// stores (first-call initialization) do not count. A slot whose address
    /// escapes counts as carried, unless it goes only to LLVM intrinsics or,
    /// as indirect call storage, to calls: `call_storage` tells whether a
    /// slot is a call's result storage, which the callee overwrites, or its
    /// argument storage, which the callee reads, and neither retains.
    pub(super) fn carried_slots(
        self,
        slots: &[Value<'ctx>],
        resumes: &[BasicBlock<'ctx>],
        ignored: &[Value<'ctx>],
        call_storage: impl Fn(Value<'ctx>) -> Option<bool>,
    ) -> Vec<bool> {
        let blocks = self.basic_blocks();
        let index: std::collections::HashMap<LLVMBasicBlockRef, usize> =
            blocks.iter().enumerate().map(|(i, b)| (b.0, i)).collect();
        let mut successors = vec![vec![]; blocks.len()];
        for (number, block) in blocks.iter().enumerate() {
            let terminator = unsafe { LLVMGetBasicBlockTerminator(block.0) };
            if terminator.is_null() {
                continue;
            }
            for i in 0..unsafe { LLVMGetNumSuccessors(terminator) } {
                successors[number].push(index[&unsafe { LLVMGetSuccessor(terminator, i) }]);
            }
        }
        // Instruction positions, to order the events within a block.
        let mut position = std::collections::HashMap::new();
        for block in &blocks {
            for (offset, instruction) in block.instructions().into_iter().enumerate() {
                position.insert(instruction.0, offset);
            }
        }
        let ignored: Vec<LLVMValueRef> = ignored.iter().map(|v| v.0).collect();
        #[derive(Clone, Copy, PartialEq)]
        enum Event {
            Read,
            Overwrite,
        }
        slots
            .iter()
            .map(|slot| {
                let storage = call_storage(*slot);
                let allocated = unsafe { LLVMGetAllocatedType(slot.0) };
                // The first event in each block that touches the slot.
                let mut first: Vec<Option<(usize, Event)>> = vec![None; blocks.len()];
                let mut note = |user: LLVMValueRef, event: Event| {
                    let block = index[&unsafe { LLVMGetInstructionParent(user) }];
                    let at = position[&user];
                    if first[block].is_none_or(|(earlier, _)| at < earlier) {
                        first[block] = Some((at, event));
                    }
                };
                let mut pending = vec![slot.0];
                while let Some(pointer) = pending.pop() {
                    let mut user_use = unsafe { LLVMGetFirstUse(pointer) };
                    while !user_use.is_null() {
                        let user = unsafe { LLVMGetUser(user_use) };
                        user_use = unsafe { LLVMGetNextUse(user_use) };
                        if unsafe { LLVMIsAInstruction(user) }.is_null() || ignored.contains(&user)
                        {
                            continue;
                        }
                        match unsafe { LLVMGetInstructionOpcode(user) } {
                            LLVMOpcode::LLVMLoad | LLVMOpcode::LLVMICmp => note(user, Event::Read),
                            LLVMOpcode::LLVMStore => {
                                if unsafe { LLVMGetOperand(user, 0) } == pointer {
                                    return true;
                                }
                                // Only a store of the whole slot replaces it.
                                let whole = pointer == slot.0
                                    && unsafe { LLVMTypeOf(LLVMGetOperand(user, 0)) } == allocated;
                                if whole {
                                    note(user, Event::Overwrite);
                                }
                            }
                            LLVMOpcode::LLVMGetElementPtr => {
                                note(user, Event::Read);
                                pending.push(user);
                            }
                            LLVMOpcode::LLVMCall => {
                                let callee = unsafe { LLVMGetCalledValue(user) };
                                let intrinsic = !callee.is_null()
                                    && unsafe { !LLVMIsAFunction(callee).is_null() }
                                    && unsafe { LLVMGetIntrinsicID(callee) } != 0;
                                match (intrinsic, storage) {
                                    (true, _) | (false, Some(false)) => note(user, Event::Read),
                                    (false, Some(true)) => note(user, Event::Overwrite),
                                    (false, None) => return true,
                                }
                            }
                            _ => return true,
                        }
                    }
                }
                // Backward liveness of the slot's contents to a fixed point.
                let mut live_in = vec![false; blocks.len()];
                let mut changed = true;
                while changed {
                    changed = false;
                    for block in (0..blocks.len()).rev() {
                        let live_out = successors[block].iter().any(|&s| live_in[s]);
                        let live = match first[block] {
                            Some((_, Event::Read)) => true,
                            Some((_, Event::Overwrite)) => false,
                            None => live_out,
                        };
                        if live != live_in[block] {
                            live_in[block] = live;
                            changed = true;
                        }
                    }
                }
                resumes.iter().any(|resume| live_in[index[&resume.0]])
            })
            .collect()
    }
}
impl<'ctx> Value<'ctx> {
    /// For each drop flag in `flags`, whether it may be set where the
    /// function suspends: at a `pending` block of `points`, on some path from
    /// `start` along which the last store to it was not `false`. A
    /// suspension continues at its `resume` block in the next call, so
    /// values flow along those edges too. Such a flag, and what it guards,
    /// must survive the suspension for a drop of the suspended state.
    pub(super) fn flags_set_at_suspension(
        self,
        flags: &[Value<'ctx>],
        start: BasicBlock<'ctx>,
        points: &[(BasicBlock<'ctx>, BasicBlock<'ctx>)],
    ) -> Vec<bool> {
        let blocks = self.basic_blocks();
        let index: std::collections::HashMap<LLVMBasicBlockRef, usize> =
            blocks.iter().enumerate().map(|(i, b)| (b.0, i)).collect();
        let entry = blocks[0].0;
        let mut successors = vec![vec![]; blocks.len()];
        for (number, block) in blocks.iter().enumerate() {
            // Later calls enter at the resume points, from their suspension.
            if block.0 == entry {
                continue;
            }
            let terminator = unsafe { LLVMGetBasicBlockTerminator(block.0) };
            if terminator.is_null() {
                continue;
            }
            for i in 0..unsafe { LLVMGetNumSuccessors(terminator) } {
                successors[number].push(index[&unsafe { LLVMGetSuccessor(terminator, i) }]);
            }
        }
        for (resume, pending) in points {
            successors[index[&pending.0]].push(index[&resume.0]);
        }
        flags
            .iter()
            .map(|flag| {
                // The last store to the flag in each block: Some(set).
                let mut last: Vec<Option<(usize, bool)>> = vec![None; blocks.len()];
                let mut user_use = unsafe { LLVMGetFirstUse(flag.0) };
                while !user_use.is_null() {
                    let user = unsafe { LLVMGetUser(user_use) };
                    user_use = unsafe { LLVMGetNextUse(user_use) };
                    if unsafe { LLVMIsAStoreInst(user) }.is_null()
                        || unsafe { LLVMGetOperand(user, 1) } != flag.0
                    {
                        continue;
                    }
                    let value = unsafe { LLVMGetOperand(user, 0) };
                    let clear = unsafe { !LLVMIsAConstantInt(value).is_null() }
                        && unsafe { LLVMConstIntGetZExtValue(value) } == 0;
                    let parent = unsafe { LLVMGetInstructionParent(user) };
                    let block = index[&parent];
                    let mut at = 0usize;
                    let mut cursor = unsafe { LLVMGetFirstInstruction(parent) };
                    while !cursor.is_null() && cursor != user {
                        at += 1;
                        cursor = unsafe { LLVMGetNextInstruction(cursor) };
                    }
                    if last[block].is_none_or(|(earlier, _)| at > earlier) {
                        last[block] = Some((at, !clear));
                    }
                }
                let mut set_in = vec![false; blocks.len()];
                let mut reached = vec![false; blocks.len()];
                reached[index[&start.0]] = true;
                let mut changed = true;
                while changed {
                    changed = false;
                    for block in 0..blocks.len() {
                        if !reached[block] {
                            continue;
                        }
                        let set_out = last[block].map_or(set_in[block], |(_, set)| set);
                        for &next in &successors[block] {
                            if !reached[next] || (set_out && !set_in[next]) {
                                reached[next] = true;
                                set_in[next] |= set_out;
                                changed = true;
                            }
                        }
                    }
                }
                points.iter().any(|(_, pending)| set_in[index[&pending.0]])
            })
            .collect()
    }
}
