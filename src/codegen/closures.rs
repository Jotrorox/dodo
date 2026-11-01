//! Function values. A function pointer `fn(..)` is the address of code using
//! the value calling convention: an optional result slot, then parameters,
//! with values larger than 1024 bytes passed through memory. A closure
//! reference `&fn(..)` pairs such code, which takes an environment pointer
//! before its parameters, with that environment: an array of the captured
//! bindings' addresses, stored in the creating function's frame.
use super::*;

impl<'a, 'ctx> Codegen<'a, 'ctx> {
    /// Which of a value-convention signature's result and parameters pass
    /// through memory.
    pub(super) fn value_convention(
        &self,
        params: &[Type],
        ret: &Type,
    ) -> Result<(bool, Vec<bool>)> {
        let large = |ty: &Type| -> Result<bool> { self.large_value(ty) };
        Ok((
            large(ret)?,
            params.iter().map(large).collect::<Result<Vec<_>>>()?,
        ))
    }
    pub(super) fn value_fn_type(
        &self,
        params: &[Type],
        ret: &Type,
        environment: bool,
    ) -> Result<LlvmType<'ctx>> {
        let (indirect_return, indirect_params) = self.value_convention(params, ret)?;
        let pointer = self.context.ptr_type(0);
        let mut types = Vec::new();
        if indirect_return {
            types.push(pointer);
        }
        if environment {
            types.push(pointer);
        }
        for (param, indirect) in params.iter().zip(indirect_params) {
            types.push(if indirect { pointer } else { self.ty(param)? });
        }
        Ok(if *ret == Type::Void || indirect_return {
            self.context.void_type().fn_type(&types, false)
        } else {
            self.ty(ret)?.fn_type(&types, false)
        })
    }
    pub(super) fn closure_value(
        &self,
        code: Value<'ctx>,
        environment: Value<'ctx>,
    ) -> Result<Value<'ctx>> {
        let pair = self
            .context
            .struct_type(&[self.code_ptr_type(), self.context.ptr_type(0)], false);
        let value = self
            .builder
            .build_insert_value(pair.get_undef(), code, 0, "closure.code")?;
        self.builder
            .build_insert_value(value, environment, 1, "closure.environment")
    }
    /// A call through an `extern "C" fn` pointer, lowered like a call to a
    /// C declaration with the pointer type's signature.
    fn invoke_c(
        &mut self,
        code: Value<'ctx>,
        params: &[Type],
        args: &[Expr],
        ret: &Type,
    ) -> Result<Value<'ctx>> {
        let what = Type::ExternFn(params.to_vec(), Box::new(ret.clone()));
        let lowering = self.c_lowering(&format!("`{what}`"), params, ret)?;
        let fn_type = match &lowering {
            Some(lowering) => self.c_fn_type(params, ret, lowering)?,
            None => self.value_fn_type(params, ret, false)?,
        };
        self.c_call(code, fn_type, (params, ret), lowering.as_ref(), args)
    }
    /// The function-pointer entry point of a named function or closure body.
    /// Functions already using the value convention are their own entry point.
    pub(super) fn fn_pointer(&mut self, name: &str) -> Result<Value<'ctx>> {
        if let Some(pointer) = self.fn_pointers.get(name) {
            return Ok(*pointer);
        }
        let function = self
            .program
            .functions
            .iter()
            .find(|f| f.name == name)
            .ok_or_else(|| error(format!("unknown function value {name}")))?;
        if function.extern_ {
            let target = self.functions[name];
            self.fn_pointers.insert(name.to_owned(), target);
            return Ok(target);
        }
        let captures = function.environment.unwrap_or(0);
        let params: Vec<Type> = function.params[captures..]
            .iter()
            .map(|p| p.ty.clone())
            .collect();
        let ret = function.ret.clone();
        let target = self.functions[name];
        let (target_return, target_params) = self.indirect[name].clone();
        let (slot_return, slot_params) = self.value_convention(&params, &ret)?;
        if function.environment.is_none()
            && slot_return == target_return
            && slot_params == target_params
        {
            self.fn_pointers.insert(name.to_owned(), target);
            return Ok(target);
        }
        let thunk = self.module.add_function(
            &format!("dodo.{}.fnptr.{name}", self.program.package),
            self.value_fn_type(&params, &ret, false)?,
            Some(LLVMInternalLinkage),
        );
        let builder = self.context.create_builder();
        builder.position_at_end(self.context.append_basic_block(thunk, "entry"));
        let mut incoming = thunk.get_param_iter();
        let slot_storage = if slot_return { incoming.next() } else { None };
        let target_storage = if target_return {
            Some(builder.build_alloca(self.ty(&ret)?, "result")?)
        } else {
            None
        };
        let mut args: Vec<Value<'ctx>> = target_storage.into_iter().collect();
        if function.environment.is_some() {
            args.push(self.context.ptr_type(0).const_zero());
        }
        for (index, (value, ty)) in incoming.zip(&params).enumerate() {
            args.push(match (slot_params[index], target_params[index]) {
                (true, false) => builder.build_load(self.ty(ty)?, value, "argument")?,
                (false, true) => {
                    let storage = builder.build_alloca(self.ty(ty)?, "argument")?;
                    builder.build_store(storage, value)?;
                    storage
                }
                _ => value,
            });
        }
        let call = match self.foreign_signatures.get(name) {
            Some(&ty) => builder.build_indirect_call(ty, target, &args, "result")?,
            None => builder.build_call(target, &args, "result")?,
        };
        if ret == Type::Void {
            builder.build_return(None)?;
        } else {
            let result = match target_storage {
                Some(storage) => builder.build_load(self.ty(&ret)?, storage, "result")?,
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
        self.fn_pointers.insert(name.to_owned(), thunk);
        Ok(thunk)
    }
    /// Closure code that calls the function pointer stored as its environment.
    pub(super) fn trampoline(&mut self, params: &[Type], ret: &Type) -> Result<Value<'ctx>> {
        let key = Type::Fn(params.to_vec(), Box::new(ret.clone())).to_string();
        if let Some(trampoline) = self.trampolines.get(&key) {
            return Ok(*trampoline);
        }
        let trampoline = self.module.add_function(
            &format!(
                "dodo.{}.trampoline.{}",
                self.program.package,
                self.trampolines.len()
            ),
            self.value_fn_type(params, ret, true)?,
            Some(LLVMInternalLinkage),
        );
        let builder = self.context.create_builder();
        builder.position_at_end(self.context.append_basic_block(trampoline, "entry"));
        let (indirect_return, _) = self.value_convention(params, ret)?;
        let mut incoming = trampoline.get_param_iter();
        let mut args = Vec::new();
        if indirect_return {
            args.push(incoming.next().unwrap());
        }
        let target = self.data_as_code(&builder, incoming.next().unwrap())?;
        args.extend(incoming);
        let call = builder.build_indirect_call(
            self.value_fn_type(params, ret, false)?,
            target,
            &args,
            "result",
        )?;
        if *ret == Type::Void || indirect_return {
            builder.build_return(None)?;
        } else {
            builder.build_return(Some(&call))?;
        }
        self.trampolines.insert(key, trampoline);
        Ok(trampoline)
    }
    pub(super) fn closure(&mut self, closure: &Closure, ty: &Type) -> Result<Value<'ctx>> {
        if closure.captures.is_empty() {
            return if matches!(ty, Type::Fn(..)) {
                self.fn_pointer(&closure.function)
            } else {
                let code = self.functions[&closure.function];
                self.closure_value(code, self.context.ptr_type(0).const_zero())
            };
        }
        let pointer = self.context.ptr_type(0);
        let layout = self
            .context
            .struct_type(&vec![pointer; closure.captures.len()], false);
        let environment = self.alloca(layout, "closure.environment")?;
        for (index, capture) in closure.captures.iter().enumerate() {
            let binding = self.binding(&capture.name).ok_or_else(|| {
                error(format!("captured binding {} has no storage", capture.name))
            })?;
            let slot =
                self.builder
                    .build_struct_gep(layout, environment, index as u32, "capture.slot")?;
            self.builder.build_store(slot, binding.ptr)?;
        }
        let code = self.functions[&closure.function];
        self.closure_value(code, environment)
    }
    pub(super) fn invoke(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        ret: &Type,
    ) -> Result<Value<'ctx>> {
        let value = self.expr(callee)?;
        let (params, _) = callee
            .ty
            .callable_signature()
            .ok_or_else(|| error(format!("cannot call a value of type {}", callee.ty)))?;
        let params = params.to_vec();
        if matches!(callee.ty, Type::ExternFn(..)) {
            return self.invoke_c(value, &params, args, ret);
        }
        let closure = callee.ty.closure_signature().is_some();
        let (indirect_return, indirect_params) = self.value_convention(&params, ret)?;
        let storage = if indirect_return {
            Some(self.alloca(self.ty(ret)?, CALL_RESULT)?)
        } else {
            None
        };
        let mut values: Vec<Value<'ctx>> = storage.into_iter().collect();
        let code = if closure {
            values.push(
                self.builder
                    .build_extract_value(value, 1, "closure.environment")?,
            );
            self.builder.build_extract_value(value, 0, "closure.code")?
        } else {
            value
        };
        let mut pending = Vec::new();
        for (e, indirect) in args.iter().zip(&indirect_params) {
            let value = self.stage(e, &mut pending)?;
            if *indirect {
                let argument = self.alloca(self.ty(&e.ty)?, CALL_ARGUMENT)?;
                self.store_value(argument, value)?;
                values.push(argument);
            } else {
                values.push(value);
            }
        }
        self.transfer(&pending)?;
        let call = self.builder.build_indirect_call(
            self.value_fn_type(&params, ret, closure)?,
            code,
            &values,
            "call",
        )?;
        if let Some(storage) = storage {
            self.load(storage, ret)
        } else {
            Ok(call.basic().unwrap_or(self.context.i8_type().const_zero()))
        }
    }
}
