//! Failure paths stay available without debug metadata or a bundled runtime.
use super::*;
use inkwell::types::FunctionType;

impl<'a, 'ctx> Codegen<'a, 'ctx> {
    fn runtime_function(&self, name: &str, ty: FunctionType<'ctx>) -> Result<FunctionValue<'ctx>> {
        if let Some(function) = self.module.get_function(name) {
            if function.get_type() != ty {
                return Err(error(format!(
                    "runtime symbol `{name}` has an incompatible declaration"
                )));
            }
            Ok(function)
        } else if self.module.get_global(name).is_some() {
            Err(error(format!(
                "runtime symbol `{name}` conflicts with a global"
            )))
        } else {
            Ok(self.module.add_function(name, ty, None))
        }
    }
    pub(super) fn panic(&mut self, check: &str) -> Result<()> {
        let triple = self
            .module
            .get_triple()
            .as_str()
            .to_string_lossy()
            .into_owned();
        let strategy = match &self.panic {
            PanicStrategy::Auto => {
                if [
                    "linux",
                    "windows",
                    "darwin",
                    "freebsd",
                    "netbsd",
                    "openbsd",
                    "dragonfly",
                    "solaris",
                    "illumos",
                ]
                .iter()
                .any(|os| triple.split('-').any(|part| part.starts_with(os)))
                {
                    PanicStrategy::Hosted
                } else {
                    PanicStrategy::Trap
                }
            }
            strategy => strategy.clone(),
        };
        let (file, line, column) = self
            .sources
            .location(self.span)
            .map(|(i, line, column)| {
                (
                    self.sources.sources[i].path.display().to_string(),
                    line,
                    column,
                )
            })
            .unwrap_or_else(|| ("<unknown>".into(), 0, 0));
        let ptr = self.context.ptr_type(AddressSpace::default());
        let i32 = self.context.i32_type();
        match strategy {
            PanicStrategy::Hosted => {
                let message = format!("dodo: {check} check failed at {file}:{line}:{column}\n");
                let text = self
                    .builder
                    .build_global_string_ptr(&message, "panic.message")?;
                let windows = triple.contains("windows");
                let count = if windows { i32 } else { self.usize_type() };
                let write = self.runtime_function(
                    if windows { "_write" } else { "write" },
                    count.fn_type(&[i32.into(), ptr.into(), count.into()], false),
                )?;
                self.builder.build_call(
                    write,
                    &[
                        i32.const_int(2, false).into(),
                        text.as_pointer_value().into(),
                        count.const_int(message.len() as u64, false).into(),
                    ],
                    "",
                )?;
                let abort =
                    self.runtime_function("abort", self.context.void_type().fn_type(&[], false))?;
                self.builder.build_call(abort, &[], "")?;
            }
            PanicStrategy::Hook(name) => {
                if name.is_empty()
                    || !name.bytes().enumerate().all(|(i, b)| {
                        b == b'_' || b.is_ascii_alphabetic() || (i != 0 && b.is_ascii_digit())
                    })
                {
                    return Err(error("panic hook must be a C symbol name"));
                }
                let hook = self.runtime_function(
                    &name,
                    self.context
                        .void_type()
                        .fn_type(&[ptr.into(), ptr.into(), i32.into(), i32.into()], false),
                )?;
                hook.add_attribute(
                    AttributeLoc::Function,
                    self.context
                        .create_enum_attribute(Attribute::get_named_enum_kind_id("noreturn"), 0),
                );
                let message = self.builder.build_global_string_ptr(check, "panic.check")?;
                let file = self.builder.build_global_string_ptr(&file, "panic.file")?;
                self.builder.build_call(
                    hook,
                    &[
                        message.as_pointer_value().into(),
                        file.as_pointer_value().into(),
                        i32.const_int(line as u64, false).into(),
                        i32.const_int(column as u64, false).into(),
                    ],
                    "",
                )?;
                // Board code owns termination. A trap fallback can lower to abort
                // on some targets and would impose an unwanted runtime dependency.
                self.builder.build_unreachable()?;
                return Ok(());
            }
            PanicStrategy::Trap => {}
            PanicStrategy::Auto => unreachable!(),
        }
        // Also terminate if an interposed hosted abort returns.
        self.trap()
    }
}
