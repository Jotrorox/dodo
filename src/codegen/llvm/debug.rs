//! DWARF construction through LLVM's metadata and debug-record APIs.
use super::*;
use llvm_sys::debuginfo::*;
use std::cell::Cell;

#[derive(Clone, Copy)]
pub(in crate::codegen) struct Metadata<'ctx>(LLVMMetadataRef, PhantomData<&'ctx Context>);
pub(in crate::codegen) struct DebugBuilder<'ctx> {
    raw: LLVMDIBuilderRef,
    finalized: Cell<bool>,
    _context: PhantomData<&'ctx Context>,
}
impl Drop for DebugBuilder<'_> {
    fn drop(&mut self) {
        self.finalize();
        unsafe { LLVMDisposeDIBuilder(self.raw) }
    }
}
/// An unresolved temporary owns its node until replacement, including on error.
pub(in crate::codegen) struct Placeholder<'ctx>(Metadata<'ctx>);
impl<'ctx> Placeholder<'ctx> {
    pub(in crate::codegen) fn metadata(&self) -> Metadata<'ctx> {
        self.0
    }
    pub(in crate::codegen) fn replace_with(self, ty: Metadata<'ctx>) {
        // LLVMMetadataReplaceAllUsesWith consumes the temporary node.
        unsafe { LLVMMetadataReplaceAllUsesWith(self.0.0, ty.0) }
        std::mem::forget(self);
    }
}
impl Drop for Placeholder<'_> {
    fn drop(&mut self) {
        unsafe { LLVMDisposeTemporaryMDNode(self.0.0) }
    }
}
#[allow(clippy::too_many_arguments)]
impl<'ctx> DebugBuilder<'ctx> {
    pub(in crate::codegen) fn new(
        module: &Module<'ctx>,
        filename: &str,
        directory: &str,
        optimized: bool,
    ) -> Self {
        let builder = Self {
            raw: unsafe { LLVMCreateDIBuilder(module.0) },
            finalized: Cell::new(false),
            _context: PhantomData,
        };
        let file = builder.create_file(filename, directory);
        let producer = concat!("dodo ", env!("CARGO_PKG_VERSION"));
        unsafe {
            LLVMDIBuilderCreateCompileUnit(
                builder.raw,
                LLVMDWARFSourceLanguage::LLVMDWARFSourceLanguageC,
                file.0,
                producer.as_ptr().cast(),
                producer.len(),
                optimized.into(),
                c"".as_ptr(),
                0,
                0,
                c"".as_ptr(),
                0,
                LLVMDWARFEmissionKind::LLVMDWARFEmissionKindFull,
                0,
                0,
                0,
                c"".as_ptr(),
                0,
                c"".as_ptr(),
                0,
            );
            let context = LLVMGetModuleContext(module.0);
            for (name, value) in [
                ("Debug Info Version", LLVMDebugMetadataVersion()),
                ("Dwarf Version", 4),
            ] {
                LLVMAddModuleFlag(
                    module.0,
                    LLVMModuleFlagBehavior::LLVMModuleFlagBehaviorWarning,
                    name.as_ptr().cast(),
                    name.len(),
                    LLVMValueAsMetadata(LLVMConstInt(
                        LLVMInt32TypeInContext(context),
                        value.into(),
                        0,
                    )),
                );
            }
        }
        builder
    }
    pub(in crate::codegen) fn finalize(&self) {
        if !self.finalized.replace(true) {
            unsafe { LLVMDIBuilderFinalize(self.raw) }
        }
    }
    pub(in crate::codegen) fn create_file(
        &self,
        filename: &str,
        directory: &str,
    ) -> Metadata<'ctx> {
        Metadata(
            unsafe {
                LLVMDIBuilderCreateFile(
                    self.raw,
                    filename.as_ptr().cast(),
                    filename.len(),
                    directory.as_ptr().cast(),
                    directory.len(),
                )
            },
            PhantomData,
        )
    }
    pub(in crate::codegen) fn create_debug_location(
        &self,
        context: &'ctx Context,
        line: u32,
        column: u32,
        scope: Metadata<'ctx>,
        inlined: Option<Metadata<'ctx>>,
    ) -> Metadata<'ctx> {
        Metadata(
            unsafe {
                LLVMDIBuilderCreateDebugLocation(
                    context.0,
                    line,
                    column,
                    scope.0,
                    inlined.map_or(ptr::null_mut(), |m| m.0),
                )
            },
            PhantomData,
        )
    }
    pub(in crate::codegen) fn create_lexical_block(
        &self,
        scope: Metadata<'ctx>,
        file: Metadata<'ctx>,
        line: u32,
        column: u32,
    ) -> Metadata<'ctx> {
        Metadata(
            unsafe { LLVMDIBuilderCreateLexicalBlock(self.raw, scope.0, file.0, line, column) },
            PhantomData,
        )
    }
    pub(in crate::codegen) fn create_subroutine_type(
        &self,
        file: Metadata<'ctx>,
        ret: Option<Metadata<'ctx>>,
        params: &[Metadata<'ctx>],
        flags: LLVMDIFlags,
    ) -> Metadata<'ctx> {
        let mut types: Vec<_> = std::iter::once(ret.map_or(ptr::null_mut(), |m| m.0))
            .chain(params.iter().map(|m| m.0))
            .collect();
        Metadata(
            unsafe {
                LLVMDIBuilderCreateSubroutineType(
                    self.raw,
                    file.0,
                    types.as_mut_ptr(),
                    types.len() as u32,
                    flags,
                )
            },
            PhantomData,
        )
    }
    pub(in crate::codegen) fn create_function(
        &self,
        scope: Metadata<'ctx>,
        name: &str,
        linkage: Option<&str>,
        file: Metadata<'ctx>,
        line: u32,
        ty: Metadata<'ctx>,
        local: bool,
        definition: bool,
        scope_line: u32,
        flags: LLVMDIFlags,
        optimized: bool,
    ) -> Metadata<'ctx> {
        let linkage = linkage.unwrap_or(name);
        Metadata(
            unsafe {
                LLVMDIBuilderCreateFunction(
                    self.raw,
                    scope.0,
                    name.as_ptr().cast(),
                    name.len(),
                    linkage.as_ptr().cast(),
                    linkage.len(),
                    file.0,
                    line,
                    ty.0,
                    local.into(),
                    definition.into(),
                    scope_line,
                    flags,
                    optimized.into(),
                )
            },
            PhantomData,
        )
    }
    pub(in crate::codegen) fn create_parameter_variable(
        &self,
        scope: Metadata<'ctx>,
        name: &str,
        index: u32,
        file: Metadata<'ctx>,
        line: u32,
        ty: Metadata<'ctx>,
        preserve: bool,
        flags: LLVMDIFlags,
    ) -> Metadata<'ctx> {
        Metadata(
            unsafe {
                LLVMDIBuilderCreateParameterVariable(
                    self.raw,
                    scope.0,
                    name.as_ptr().cast(),
                    name.len(),
                    index,
                    file.0,
                    line,
                    ty.0,
                    preserve.into(),
                    flags,
                )
            },
            PhantomData,
        )
    }
    pub(in crate::codegen) fn create_auto_variable(
        &self,
        scope: Metadata<'ctx>,
        name: &str,
        file: Metadata<'ctx>,
        line: u32,
        ty: Metadata<'ctx>,
        preserve: bool,
        flags: LLVMDIFlags,
        align: u32,
    ) -> Metadata<'ctx> {
        Metadata(
            unsafe {
                LLVMDIBuilderCreateAutoVariable(
                    self.raw,
                    scope.0,
                    name.as_ptr().cast(),
                    name.len(),
                    file.0,
                    line,
                    ty.0,
                    preserve.into(),
                    flags,
                    align,
                )
            },
            PhantomData,
        )
    }
    pub(in crate::codegen) fn insert_declare(
        &self,
        value: Value<'ctx>,
        var: Metadata<'ctx>,
        location: Metadata<'ctx>,
        block: BasicBlock<'ctx>,
    ) {
        unsafe {
            let expression = LLVMDIBuilderCreateExpression(self.raw, ptr::null_mut(), 0);
            LLVMDIBuilderInsertDeclareRecordAtEnd(
                self.raw, value.0, var.0, expression, location.0, block.0,
            );
        }
    }
    pub(in crate::codegen) fn placeholder(&self, context: &'ctx Context) -> Placeholder<'ctx> {
        Placeholder(Metadata(
            unsafe { LLVMTemporaryMDNode(context.0, ptr::null_mut(), 0) },
            PhantomData,
        ))
    }
    pub(in crate::codegen) fn create_basic_type(
        &self,
        name: &str,
        size: u64,
        encoding: u32,
        flags: LLVMDIFlags,
    ) -> Result<Metadata<'ctx>> {
        Ok(Metadata(
            unsafe {
                LLVMDIBuilderCreateBasicType(
                    self.raw,
                    name.as_ptr().cast(),
                    name.len(),
                    size,
                    encoding,
                    flags,
                )
            },
            PhantomData,
        ))
    }
    pub(in crate::codegen) fn create_pointer_type(
        &self,
        name: &str,
        inner: Metadata<'ctx>,
        size: u64,
        align: u32,
        space: u32,
    ) -> Metadata<'ctx> {
        Metadata(
            unsafe {
                LLVMDIBuilderCreatePointerType(
                    self.raw,
                    inner.0,
                    size,
                    align,
                    space,
                    name.as_ptr().cast(),
                    name.len(),
                )
            },
            PhantomData,
        )
    }
    pub(in crate::codegen) fn create_array_type(
        &self,
        inner: Metadata<'ctx>,
        size: u64,
        align: u32,
        ranges: &[std::ops::Range<i64>],
    ) -> Metadata<'ctx> {
        let mut subscripts: Vec<_> = ranges
            .iter()
            .map(|r| unsafe {
                LLVMDIBuilderGetOrCreateSubrange(self.raw, r.start, r.end - r.start)
            })
            .collect();
        Metadata(
            unsafe {
                LLVMDIBuilderCreateArrayType(
                    self.raw,
                    size,
                    align,
                    inner.0,
                    subscripts.as_mut_ptr(),
                    subscripts.len() as u32,
                )
            },
            PhantomData,
        )
    }
    pub(in crate::codegen) fn create_typedef(
        &self,
        inner: Metadata<'ctx>,
        name: &str,
        file: Metadata<'ctx>,
        line: u32,
        scope: Metadata<'ctx>,
        align: u32,
    ) -> Metadata<'ctx> {
        Metadata(
            unsafe {
                LLVMDIBuilderCreateTypedef(
                    self.raw,
                    inner.0,
                    name.as_ptr().cast(),
                    name.len(),
                    file.0,
                    line,
                    scope.0,
                    align,
                )
            },
            PhantomData,
        )
    }
    pub(in crate::codegen) fn create_member_type(
        &self,
        scope: Metadata<'ctx>,
        name: &str,
        file: Metadata<'ctx>,
        line: u32,
        size: u64,
        align: u32,
        offset: u64,
        flags: LLVMDIFlags,
        inner: Metadata<'ctx>,
    ) -> Metadata<'ctx> {
        Metadata(
            unsafe {
                LLVMDIBuilderCreateMemberType(
                    self.raw,
                    scope.0,
                    name.as_ptr().cast(),
                    name.len(),
                    file.0,
                    line,
                    size,
                    align,
                    offset,
                    flags,
                    inner.0,
                )
            },
            PhantomData,
        )
    }
    pub(in crate::codegen) fn create_struct_type(
        &self,
        scope: Metadata<'ctx>,
        name: &str,
        file: Metadata<'ctx>,
        line: u32,
        size: u64,
        align: u32,
        flags: LLVMDIFlags,
        base: Option<Metadata<'ctx>>,
        members: &[Metadata<'ctx>],
        language: u32,
        vtable: Option<Metadata<'ctx>>,
        id: &str,
    ) -> Metadata<'ctx> {
        let mut members: Vec<_> = members.iter().map(|m| m.0).collect();
        Metadata(
            unsafe {
                LLVMDIBuilderCreateStructType(
                    self.raw,
                    scope.0,
                    name.as_ptr().cast(),
                    name.len(),
                    file.0,
                    line,
                    size,
                    align,
                    flags,
                    base.map_or(ptr::null_mut(), |m| m.0),
                    members.as_mut_ptr(),
                    members.len() as u32,
                    language,
                    vtable.map_or(ptr::null_mut(), |m| m.0),
                    id.as_ptr().cast(),
                    id.len(),
                )
            },
            PhantomData,
        )
    }
    pub(in crate::codegen) fn create_union_type(
        &self,
        scope: Metadata<'ctx>,
        name: &str,
        file: Metadata<'ctx>,
        line: u32,
        size: u64,
        align: u32,
        flags: LLVMDIFlags,
        members: &[Metadata<'ctx>],
        language: u32,
        id: &str,
    ) -> Metadata<'ctx> {
        let mut members: Vec<_> = members.iter().map(|m| m.0).collect();
        Metadata(
            unsafe {
                LLVMDIBuilderCreateUnionType(
                    self.raw,
                    scope.0,
                    name.as_ptr().cast(),
                    name.len(),
                    file.0,
                    line,
                    size,
                    align,
                    flags,
                    members.as_mut_ptr(),
                    members.len() as u32,
                    language,
                    id.as_ptr().cast(),
                    id.len(),
                )
            },
            PhantomData,
        )
    }
    pub(in crate::codegen) fn create_enumerator(
        &self,
        name: &str,
        value: i64,
        unsigned: bool,
    ) -> Metadata<'ctx> {
        Metadata(
            unsafe {
                LLVMDIBuilderCreateEnumerator(
                    self.raw,
                    name.as_ptr().cast(),
                    name.len(),
                    value,
                    unsigned.into(),
                )
            },
            PhantomData,
        )
    }
    pub(in crate::codegen) fn create_enumeration_type(
        &self,
        scope: Metadata<'ctx>,
        name: &str,
        file: Metadata<'ctx>,
        line: u32,
        size: u64,
        align: u32,
        variants: &[Metadata<'ctx>],
        inner: Metadata<'ctx>,
    ) -> Metadata<'ctx> {
        let mut variants: Vec<_> = variants.iter().map(|m| m.0).collect();
        Metadata(
            unsafe {
                LLVMDIBuilderCreateEnumerationType(
                    self.raw,
                    scope.0,
                    name.as_ptr().cast(),
                    name.len(),
                    file.0,
                    line,
                    size,
                    align,
                    variants.as_mut_ptr(),
                    variants.len() as u32,
                    inner.0,
                )
            },
            PhantomData,
        )
    }
}
impl<'ctx> Value<'ctx> {
    pub(in crate::codegen) fn set_subprogram(self, program: Metadata<'ctx>) {
        unsafe { LLVMSetSubprogram(self.0, program.0) }
    }
}
impl<'ctx> Builder<'ctx> {
    pub(in crate::codegen) fn set_current_debug_location(&self, location: Metadata<'ctx>) {
        unsafe { LLVMSetCurrentDebugLocation2(self.0, location.0) }
    }
    pub(in crate::codegen) fn unset_current_debug_location(&self) {
        unsafe { LLVMSetCurrentDebugLocation2(self.0, ptr::null_mut()) }
    }
}
