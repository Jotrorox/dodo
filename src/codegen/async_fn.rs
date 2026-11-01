//! `async fn` state machines.
//!
//! An async fn's future is a struct `{ parameters..., state, home, frame }`.
//! Its `poll` is generated from the body function (`AsyncPart::Body`) as an
//! ordinary function first: locals are stack slots, and each `await` stores
//! its future in a slot, records a resume point, and returns Pending while
//! the future is. Then the function is rewritten into a state machine:
//!
//! 1. The entry block ends in a switch on `state` that jumps to the body's
//!    start, to a resume point, or to a finished future's Ready.
//! 2. Values that a resume point can reach without passing their definition
//!    are demoted through slots, which the next step makes persistent.
//! 3. Every stack slot becomes a field of the frame, so locals survive from
//!    one poll to the next. Drop flags start cleared on the first poll only.
//!
//! The frame holds pointers into itself (a local borrowed across an await),
//! so a started future must not move: `home` records its address on the
//! first poll, and polling it elsewhere traps.
use super::*;

/// The `state` field: not started, finished with its output stored, finished
/// with nothing left, or suspended at await `state - SUSPENDED`.
const START: u64 = 0;
const READY: u64 = 1;
const FINISHED: u64 = 2;
const SUSPENDED: u64 = 3;

/// A poll function being generated.
pub(super) struct Frame<'ctx> {
    state: Value<'ctx>,
    start: BasicBlock<'ctx>,
    resumes: Vec<BasicBlock<'ctx>>,
    /// Where each await returns Pending, matching `resumes`.
    pendings: Vec<BasicBlock<'ctx>>,
    /// First-poll clearing of drop flags: flag and store.
    zero_inits: Vec<(Value<'ctx>, Value<'ctx>)>,
    output: Option<Value<'ctx>>,
    poll: Type,
    /// Bindings with drop flags: storage, type, flag.
    owned: Vec<(Value<'ctx>, Type, Value<'ctx>)>,
    /// Parameters, which stay in the future's fields: type and drop flag.
    params: Vec<(Type, Option<Value<'ctx>>)>,
}

/// What `output` and `drop` need to know about a generated frame.
pub(super) struct FrameLayout<'ctx> {
    frame: LlvmType<'ctx>,
    output: Option<u32>,
    owned: Vec<(u32, Type, u32)>,
    params: Vec<(Type, Option<u32>)>,
}

/// The structs and enums whose values contain an async fn's future, directly
/// or in a field, element, or payload.
pub(super) fn future_holders(program: &Program) -> HashSet<String> {
    fn names(ty: &Type, holders: &HashSet<String>) -> bool {
        match ty {
            Type::Named(name) => holders.contains(name),
            Type::Array(_, inner) | Type::Option(inner) | Type::MaybeUninit(inner) => {
                names(inner, holders)
            }
            Type::Result(value, error) => names(value, holders) || names(error, holders),
            _ => false,
        }
    }
    let mut holders: HashSet<String> = program
        .structs
        .iter()
        .filter(|s| s.async_future)
        .map(|s| s.name.clone())
        .collect();
    if holders.is_empty() {
        return holders;
    }
    loop {
        let mut added = Vec::new();
        for s in &program.structs {
            if !holders.contains(&s.name) && s.fields.iter().any(|f| names(&f.ty, &holders)) {
                added.push(s.name.clone());
            }
        }
        for e in &program.enums {
            if !holders.contains(&e.name)
                && e.variants
                    .iter()
                    .any(|v| v.fields.iter().any(|f| names(&f.ty, &holders)))
            {
                added.push(e.name.clone());
            }
        }
        if added.is_empty() {
            return holders;
        }
        holders.extend(added);
    }
}

impl<'a, 'ctx> Codegen<'a, 'ctx> {
    pub(super) fn is_future(&self, name: &str) -> bool {
        self.program
            .structs
            .iter()
            .any(|s| s.async_future && s.name == name)
    }
    /// Whether a value of `ty` contains an async fn's future, whose size is
    /// known only once its state machine is generated. Such values always
    /// pass through memory, so signatures never need their size.
    pub(super) fn holds_future(&self, ty: &Type) -> bool {
        match ty {
            Type::Named(name) => self.future_holders.contains(name),
            Type::Array(_, inner) | Type::Option(inner) | Type::MaybeUninit(inner) => {
                self.holds_future(inner)
            }
            Type::Result(value, error) => self.holds_future(value) || self.holds_future(error),
            _ => false,
        }
    }
    /// Whether a value passes through memory in Dodo's calling convention.
    pub(super) fn large_value(&self, ty: &Type) -> Result<bool> {
        Ok(*ty != Type::Void
            && (self.holds_future(ty) || self.data.get_abi_size(&self.ty(ty)?) > 1024))
    }
    /// The futures a value of `ty` contains, directly or in its parts.
    fn futures_in(&self, ty: &Type, found: &mut Vec<String>, visiting: &mut Vec<String>) {
        match ty {
            Type::Named(name) => {
                if visiting.contains(name) {
                    return;
                }
                visiting.push(name.clone());
                if let Some(s) = self.program.structs.iter().find(|s| s.name == *name) {
                    if s.async_future {
                        if !found.contains(name) {
                            found.push(name.clone());
                        }
                    } else {
                        for field in &s.fields {
                            self.futures_in(&field.ty, found, visiting);
                        }
                    }
                } else if let Some(e) = self.program.enums.iter().find(|e| e.name == *name) {
                    for variant in &e.variants {
                        for field in &variant.fields {
                            self.futures_in(&field.ty, found, visiting);
                        }
                    }
                }
                visiting.pop();
            }
            Type::Array(_, inner) | Type::Option(inner) | Type::MaybeUninit(inner) => {
                self.futures_in(inner, found, visiting)
            }
            Type::Result(value, error) => {
                self.futures_in(value, found, visiting);
                self.futures_in(error, found, visiting);
            }
            _ => {}
        }
    }
    /// The futures whose layout a future's frame needs: those its parameters
    /// and the values of its body hold.
    fn frame_dependencies(&self, future: &Struct) -> Vec<String> {
        let mut found = Vec::new();
        for ty in &self.frame_types(future) {
            self.futures_in(ty, &mut found, &mut Vec::new());
        }
        found
    }
    /// The types of a future's parameters and of every value in its body.
    fn frame_types(&self, future: &Struct) -> Vec<Type> {
        fn block_types(block: &Block, types: &mut Vec<Type>) {
            for statement in block {
                stmt_types(statement, types);
            }
        }
        fn stmt_types(statement: &Stmt, types: &mut Vec<Type>) {
            match &statement.kind {
                StmtKind::Let { ty, value, .. } => {
                    types.push(ty.clone());
                    if let Some(value) = value {
                        expr_types(value, types);
                    }
                }
                StmtKind::LetPattern {
                    ty,
                    value,
                    else_block,
                    ..
                } => {
                    types.push(ty.clone());
                    expr_types(value, types);
                    if let Some(block) = else_block {
                        block_types(block, types);
                    }
                }
                StmtKind::Assign { target, value, .. } => {
                    expr_types(target, types);
                    expr_types(value, types);
                }
                StmtKind::Expr(value) | StmtKind::Yield(value) => expr_types(value, types),
                StmtKind::Return(value) => {
                    if let Some(value) = value {
                        expr_types(value, types);
                    }
                }
                StmtKind::If {
                    condition,
                    then_block,
                    else_block,
                } => {
                    expr_types(condition, types);
                    block_types(then_block, types);
                    block_types(else_block, types);
                }
                StmtKind::IfLet {
                    value,
                    then_block,
                    else_block,
                    ..
                } => {
                    expr_types(value, types);
                    block_types(then_block, types);
                    block_types(else_block, types);
                }
                StmtKind::For {
                    init,
                    condition,
                    step,
                    body,
                } => {
                    if let Some(init) = init {
                        stmt_types(init, types);
                    }
                    if let Some(condition) = condition {
                        expr_types(condition, types);
                    }
                    if let Some(step) = step {
                        stmt_types(step, types);
                    }
                    block_types(body, types);
                }
                StmtKind::ForEach { iterable, body, .. } => {
                    expr_types(iterable, types);
                    block_types(body, types);
                }
                StmtKind::Match { value, arms } => {
                    expr_types(value, types);
                    for arm in arms {
                        if let Some(guard) = &arm.guard {
                            expr_types(guard, types);
                        }
                        block_types(&arm.body, types);
                    }
                }
                StmtKind::Block(block) | StmtKind::Unsafe(block) => block_types(block, types),
                StmtKind::Break | StmtKind::Continue => {}
            }
        }
        fn expr_types(expression: &Expr, types: &mut Vec<Type>) {
            types.push(expression.ty.clone());
            match &expression.kind {
                ExprKind::Array(_, items) => items.iter().for_each(|e| expr_types(e, types)),
                ExprKind::Repeat(inner, _)
                | ExprKind::Constant(inner, _)
                | ExprKind::Unary(_, inner)
                | ExprKind::Field(inner, _)
                | ExprKind::Cast(inner, _)
                | ExprKind::Try(inner)
                | ExprKind::Unwrap(inner)
                | ExprKind::Await { value: inner, .. } => expr_types(inner, types),
                ExprKind::ValueBlock(block) => block_types(block, types),
                ExprKind::Range(a, b) | ExprKind::Binary(_, a, b) | ExprKind::Index(a, b) => {
                    expr_types(a, types);
                    expr_types(b, types);
                }
                ExprKind::Slice {
                    base, start, end, ..
                } => {
                    expr_types(base, types);
                    for e in start.iter().chain(end) {
                        expr_types(e, types);
                    }
                }
                ExprKind::Struct(_, fields) => {
                    fields.iter().for_each(|(_, e)| expr_types(e, types))
                }
                ExprKind::Call { args, .. } => args.iter().for_each(|e| expr_types(e, types)),
                ExprKind::MethodCall { receiver, args, .. }
                | ExprKind::Invoke {
                    callee: receiver,
                    args,
                } => {
                    expr_types(receiver, types);
                    args.iter().for_each(|e| expr_types(e, types));
                }
                ExprKind::Asm(asm) => asm.inputs().for_each(|e| expr_types(e, types)),
                ExprKind::Int(..)
                | ExprKind::Float(..)
                | ExprKind::Bool(_)
                | ExprKind::String(..)
                | ExprKind::Name(_)
                | ExprKind::Closure(_) => {}
            }
        }
        let mut types: Vec<Type> = future.fields.iter().map(|f| f.ty.clone()).collect();
        if let Some(body) = self
            .future_function(&future.name, "body")
            .and_then(|f| f.body.as_ref())
        {
            block_types(body, &mut types);
        }
        types.sort_by_key(|t| t.to_string());
        types.dedup();
        types
    }
    fn future_function(&self, future: &str, part: &str) -> Option<&'a Function> {
        let program: &'a Program = self.program;
        program
            .functions
            .iter()
            .find(|f| f.name == format!("{future}.{part}"))
    }
    /// Generate every future's state machine, after all declarations and
    /// before the bodies that use futures by value: a future's size is that
    /// of its frame. Futures a frame holds by value come first; a future that
    /// holds itself would be infinitely large.
    pub(super) fn async_frames(&mut self) -> Result<()> {
        let program: &'a Program = self.program;
        let futures: Vec<&Struct> = program
            .structs
            .iter()
            .filter(|s| s.async_future && s.generics.is_empty())
            .collect();
        if futures.is_empty() {
            return Ok(());
        }
        let mut order: Vec<&Struct> = Vec::new();
        let mut visiting: Vec<String> = Vec::new();
        fn visit<'p>(
            cg: &Codegen<'p, '_>,
            future: &'p Struct,
            futures: &[&'p Struct],
            order: &mut Vec<&'p Struct>,
            visiting: &mut Vec<String>,
        ) -> Result<()> {
            if order.iter().any(|s| s.name == future.name) {
                return Ok(());
            }
            if visiting.contains(&future.name) {
                let name = crate::ast::source_names(&future.name);
                return Err(error(format!(
                    "the future `{name}` holds itself, so it would be infinitely large; an async fn cannot await itself directly or through others"
                )));
            }
            visiting.push(future.name.clone());
            for dependency in cg.frame_dependencies(future) {
                if dependency == future.name {
                    let name = crate::ast::source_names(&future.name);
                    return Err(error(format!(
                        "the future `{name}` holds itself, so it would be infinitely large; an async fn cannot await itself directly or through others"
                    )));
                }
                if let Some(next) = futures.iter().find(|s| s.name == dependency) {
                    visit(cg, next, futures, order, visiting)?;
                }
            }
            visiting.pop();
            order.push(future);
            Ok(())
        }
        for future in &futures {
            visit(self, future, &futures, &mut order, &mut visiting)?;
        }
        let mut layouts = HashMap::new();
        for future in &order {
            let layout = self.poll_function(future)?;
            layouts.insert(future.name.clone(), layout);
        }
        self.frame = None;
        // Layouts that hold futures waited for their frames.
        for name in program
            .structs
            .iter()
            .filter(|s| s.generics.is_empty())
            .map(|s| &s.name)
            .chain(
                program
                    .enums
                    .iter()
                    .filter(|e| e.generics.is_empty())
                    .map(|e| &e.name),
            )
        {
            self.define_layout(&Type::Named(name.clone()))?;
        }
        for future in &order {
            let layout = &layouts[&future.name];
            if self.future_function(&future.name, "output").is_some() {
                self.output_function(future, layout)?;
            }
            self.drop_function(future, layout)?;
        }
        Ok(())
    }
    /// The future's struct type, with an opaque frame until its poll exists.
    fn begin_layout(&mut self, future: &Struct) -> Result<LlvmType<'ctx>> {
        // Values of the body that hold futures: theirs are complete by now.
        for ty in self.frame_types(future) {
            if !matches!(&ty, Type::Named(name) if *name == future.name) {
                self.define_layout(&ty)?;
            }
        }
        let layout = self.structs[&future.name];
        let frame = self
            .context
            .opaque_struct_type(&format!("{}.frame", future.name));
        let mut fields = future
            .fields
            .iter()
            .map(|f| self.ty(&f.ty))
            .collect::<Result<Vec<_>>>()?;
        fields.push(self.context.i32_type());
        fields.push(self.context.ptr_type(0));
        fields.push(frame);
        layout.set_body(&fields, false);
        self.frames.insert(future.name.clone(), frame);
        Ok(layout)
    }
    fn poll_function(&mut self, future: &Struct) -> Result<FrameLayout<'ctx>> {
        let layout = self.begin_layout(future)?;
        let frame_ty = self.frames[&future.name];
        let poll = self
            .future_function(&future.name, "poll")
            .ok_or_else(|| error("async future without poll"))?;
        let body = self
            .future_function(&future.name, "body")
            .ok_or_else(|| error("async future without a body"))?;
        let function = self.functions[&poll.name];
        let count = future.fields.len() as u32;
        self.function = Some(function);
        self.return_type = body.ret.clone();
        self.return_storage = None;
        self.c_result = None;
        self.scopes = vec![vec![]];
        self.loops.clear();
        self.debug_function(poll, function)?;
        self.location(body.span);
        let entry = self.context.append_basic_block(function, "entry");
        let start = self.context.append_basic_block(function, "async.start");
        self.builder.position_at_end(entry);
        let this = function.get_nth_param(0).unwrap();
        let cx = function.get_nth_param(1).unwrap();
        let state = self
            .builder
            .build_struct_gep(layout, this, count, "async.state")?;
        let home = self
            .builder
            .build_struct_gep(layout, this, count + 1, "async.home")?;
        let mut fields = Vec::new();
        for index in 0..count {
            fields.push(
                self.builder
                    .build_struct_gep(layout, this, index, "async.parameter")?,
            );
        }
        self.builder.position_at_end(start);
        self.frame = Some(Frame {
            state,
            start,
            resumes: vec![],
            pendings: vec![],
            zero_inits: vec![],
            output: None,
            poll: poll.ret.clone(),
            owned: vec![],
            params: vec![],
        });
        if body.ret != Type::Void {
            let slot = self.alloca(self.ty(&body.ret)?, "async.output")?;
            self.frame.as_mut().unwrap().output = Some(slot);
        }
        // The context changes from poll to poll: store it on every entry.
        let cx_ty = poll.params[1].ty.clone();
        let cx_binding = self.bind("$cx", &cx_ty, None)?;
        {
            let entry_builder = self.context.create_builder();
            entry_builder.position_at_end(entry);
            entry_builder.build_store(cx_binding.ptr, cx)?;
        }
        self.builder.build_store(home, this)?;
        // Parameters live in the future's fields from construction on.
        for (index, parameter) in body.params.iter().enumerate() {
            let live = if self.needs_drop(&parameter.ty) {
                let flag = self.alloca(self.context.bool_type(), "initialized")?;
                self.zero_on_start(flag)?;
                self.builder
                    .build_store(flag, self.context.bool_type().const_int(1, false))?;
                Some(flag)
            } else {
                None
            };
            self.scopes.last_mut().unwrap().push(Binding {
                name: parameter.name.clone(),
                ty: parameter.ty.clone(),
                ptr: fields[index],
                live,
            });
            self.debug_variable(&parameter.name, &parameter.ty, fields[index])?;
            self.frame
                .as_mut()
                .unwrap()
                .params
                .push((parameter.ty.clone(), live));
        }
        self.block(body.body.as_ref().unwrap())?;
        if !self.terminated() {
            self.cleanup_to(0)?;
            if body.ret == Type::Void {
                self.frame_return(None)?;
            } else {
                self.builder.build_unreachable()?;
            }
        }
        self.scopes.clear();
        // Dispatch on the state.
        let frame = self.frame.take().unwrap();
        self.builder.position_at_end(entry);
        let i32_ty = self.context.i32_type();
        let current = self.builder.build_load(i32_ty, state, "state")?;
        let done = self.context.append_basic_block(function, "async.done");
        let invalid = self.context.append_basic_block(function, "async.invalid");
        let mut cases = vec![
            (i32_ty.const_int(START, false), start),
            (i32_ty.const_int(READY, false), done),
            (i32_ty.const_int(FINISHED, false), done),
        ];
        let mut checks = Vec::new();
        for (index, resume) in frame.resumes.iter().enumerate() {
            let check = self.context.append_basic_block(function, "async.moved");
            cases.push((i32_ty.const_int(SUSPENDED + index as u64, false), check));
            checks.push((check, *resume));
        }
        self.builder.build_switch(current, invalid, &cases)?;
        self.builder.position_at_end(invalid);
        self.builder.build_unreachable()?;
        self.builder.position_at_end(done);
        let ready = self.poll_value(&frame.poll, "Ready")?;
        self.builder.build_return(Some(&ready))?;
        for (check, resume) in checks {
            self.builder.position_at_end(check);
            let address = self
                .builder
                .build_load(self.context.ptr_type(0), home, "async.home")?;
            let same = self.builder.build_int_compare(
                IntPredicate::LLVMIntEQ,
                address,
                this,
                "async.unmoved",
            )?;
            self.guard(
                same,
                "async future moved",
                "a suspended future was moved; keep it in place until it finishes",
            )?;
            self.builder.build_unconditional_branch(resume)?;
        }
        // Values crossing a resume point go through slots, then the slots
        // that carry a value from one poll to the next move into the frame.
        {
            let repair = self.context.create_builder();
            function.repair_dominance(&repair, |ty| self.alloca(ty, "resumed.slot"))?;
        }
        let all: Vec<Value<'ctx>> = entry
            .instructions()
            .into_iter()
            .filter(|i| i.is_alloca())
            .collect();
        let first_poll: Vec<Value<'ctx>> = frame.zero_inits.iter().map(|(_, s)| *s).collect();
        // Indirect call storage: the callee overwrites a result and reads an
        // argument, and retains neither.
        let carried = function.carried_slots(&all, &frame.resumes, &first_poll, |slot| {
            let name = slot.get_name().to_string_lossy();
            if name.starts_with(CALL_RESULT) {
                Some(true)
            } else if name.starts_with(CALL_ARGUMENT) {
                Some(false)
            } else {
                None
            }
        });
        let mut carried = carried;
        // A suspended future's drop reads the flags that may be set at a
        // suspension, and the bindings they guard.
        let points: Vec<_> = frame
            .resumes
            .iter()
            .copied()
            .zip(frame.pendings.iter().copied())
            .collect();
        let guarded: Vec<(Option<Value<'ctx>>, Value<'ctx>)> = frame
            .owned
            .iter()
            .map(|(slot, _, flag)| (Some(*slot), *flag))
            .chain(
                frame
                    .params
                    .iter()
                    .filter_map(|(_, flag)| flag.map(|f| (None, f))),
            )
            .collect();
        let flags: Vec<Value<'ctx>> = guarded.iter().map(|(_, flag)| *flag).collect();
        let set = function.flags_set_at_suspension(&flags, frame.start, &points);
        for ((slot, flag), set) in guarded.iter().zip(set) {
            if set {
                for value in slot.iter().chain([flag]) {
                    if let Some(index) = all.iter().position(|s| s == value) {
                        carried[index] = true;
                    }
                }
            }
        }
        let rewrite = self.context.create_builder();
        let mut slots = Vec::new();
        for (slot, carried) in all.iter().zip(carried) {
            // `output` reads the result after the poll that stored it.
            if carried || Some(*slot) == frame.output {
                slots.push(*slot);
            } else {
                // A slot on the stack starts every poll with its flag clear.
                for (flag, store) in &frame.zero_inits {
                    if flag == slot {
                        store.erase_instruction();
                        rewrite.position_before(&entry.get_terminator().unwrap());
                        rewrite.build_store(*flag, self.context.bool_type().const_zero())?;
                    }
                }
            }
        }
        let types: Vec<LlvmType<'ctx>> = slots.iter().map(|s| s.allocated_type()).collect();
        frame_ty.set_body(&types, false);
        rewrite.position_before(&entry.get_first_instruction().unwrap());
        let frame_ptr = rewrite.build_struct_gep(layout, this, count + 2, "async.frame")?;
        let position =
            |value: Value<'ctx>| slots.iter().position(|s| *s == value).map(|i| i as u32);
        let result = FrameLayout {
            frame: frame_ty,
            output: frame.output.and_then(position),
            owned: frame
                .owned
                .iter()
                .filter_map(|(slot, ty, flag)| {
                    Some((position(*slot)?, ty.clone(), position(*flag)?))
                })
                .collect(),
            params: frame
                .params
                .iter()
                .map(|(ty, flag)| (ty.clone(), flag.and_then(position)))
                .collect(),
        };
        for (index, slot) in slots.iter().enumerate() {
            rewrite.position_before(slot);
            let field =
                rewrite.build_struct_gep(frame_ty, frame_ptr, index as u32, "async.slot")?;
            slot.replace_all_uses_with(field);
            slot.erase_instruction();
        }
        Ok(result)
    }
    /// Clear a drop flag on the first poll only: later polls find it in the
    /// frame as the previous poll left it.
    pub(super) fn zero_on_start(&mut self, flag: Value<'ctx>) -> Result<()> {
        let start = self.frame.as_ref().unwrap().start;
        let builder = self.context.create_builder();
        if let Some(first) = start.get_first_instruction() {
            builder.position_before(&first);
        } else {
            builder.position_at_end(start);
        }
        let store = builder.build_store(flag, self.context.bool_type().const_zero())?;
        self.frame.as_mut().unwrap().zero_inits.push((flag, store));
        Ok(())
    }
    /// Track a binding that owns a value, for dropping a suspended future.
    pub(super) fn frame_owns(&mut self, ptr: Value<'ctx>, ty: &Type, flag: Value<'ctx>) {
        if let Some(frame) = &mut self.frame {
            frame.owned.push((ptr, ty.clone(), flag));
        }
    }
    fn poll_value(&self, poll: &Type, variant: &str) -> Result<Value<'ctx>> {
        let tag = self.variant_tag(poll, variant)?;
        self.tagged_value(
            self.ty(poll)?,
            self.context.i32_type().const_int(tag, false),
            None,
        )
    }
    /// Finish the body: keep its result for `output` and report Ready.
    pub(super) fn frame_return(&mut self, value: Option<Value<'ctx>>) -> Result<()> {
        let frame = self.frame.as_ref().unwrap();
        let (state, output, poll) = (frame.state, frame.output, frame.poll.clone());
        let next = match (value, output) {
            (Some(value), Some(slot)) => {
                self.store_value(slot, value)?;
                READY
            }
            _ => FINISHED,
        };
        self.builder
            .build_store(state, self.context.i32_type().const_int(next, false))?;
        let ready = self.poll_value(&poll, "Ready")?;
        self.builder.build_return(Some(&ready))?;
        Ok(())
    }
    /// `await future`: keep the future in the frame and poll it until it is
    /// ready, returning Pending from this poll meanwhile.
    pub(super) fn await_expr(
        &mut self,
        value: &Expr,
        poll: &str,
        output: Option<&str>,
        ty: &Type,
    ) -> Result<Value<'ctx>> {
        let Some(frame) = &self.frame else {
            return Err(error("`await` outside an async fn"));
        };
        let (state, index, poll_ty) = (frame.state, frame.resumes.len(), frame.poll.clone());
        let future = self.expr(value)?;
        let name = format!("$await{index}");
        let slot = self.bind(&name, &value.ty, Some(future))?;
        self.builder.build_store(
            state,
            self.context
                .i32_type()
                .const_int(SUSPENDED + index as u64, false),
        )?;
        let resume = self.bb("async.resume");
        self.frame.as_mut().unwrap().resumes.push(resume);
        self.builder.build_unconditional_branch(resume)?;
        self.builder.position_at_end(resume);
        let span = value.span;
        let reference = |name: &str, ty: &Type| Expr {
            kind: ExprKind::Unary(
                UnaryOp::BorrowMut,
                Box::new(Expr {
                    kind: ExprKind::Name(name.to_owned()),
                    span,
                    ty: ty.clone(),
                }),
            ),
            span,
            ty: Type::Ref(true, Box::new(ty.clone())),
        };
        let cx = self.binding("$cx").unwrap();
        let call = Expr {
            kind: ExprKind::Call {
                name: poll.to_owned(),
                type_args: vec![],
                args: vec![
                    reference(&name, &value.ty),
                    Expr {
                        kind: ExprKind::Name("$cx".to_owned()),
                        span,
                        ty: cx.ty.clone(),
                    },
                ],
            },
            span,
            ty: poll_ty.clone(),
        };
        let result = self.expr(&call)?;
        let tag = self.builder.build_extract_value(result, 0, "poll.tag")?;
        let ready_tag = self.variant_tag(&poll_ty, "Ready")?;
        let ready = self.builder.build_int_compare(
            IntPredicate::LLVMIntEQ,
            tag,
            self.context.i32_type().const_int(ready_tag, false),
            "poll.ready",
        )?;
        let pending = self.bb("async.pending");
        let resumed = self.bb("async.ready");
        self.builder
            .build_conditional_branch(ready, resumed, pending)?;
        self.builder.position_at_end(pending);
        self.frame.as_mut().unwrap().pendings.push(pending);
        let waiting = self.poll_value(&poll_ty, "Pending")?;
        self.builder.build_return(Some(&waiting))?;
        self.builder.position_at_end(resumed);
        let result = match output {
            Some(output) => {
                let call = Expr {
                    kind: ExprKind::Call {
                        name: output.to_owned(),
                        type_args: vec![],
                        args: vec![reference(&name, &value.ty)],
                    },
                    span,
                    ty: ty.clone(),
                };
                self.expr(&call)?
            }
            None => self.context.i8_type().const_zero(),
        };
        self.drop_binding(&slot)?;
        Ok(result)
    }
    /// `output(&mut self) -> T`: move the result out, once.
    fn output_function(&mut self, future: &Struct, frame: &FrameLayout<'ctx>) -> Result<()> {
        let output = self.future_function(&future.name, "output").unwrap();
        let function = self.functions[&output.name];
        let (indirect, _) = self.indirect[&output.name].clone();
        self.function = Some(function);
        self.return_type = output.ret.clone();
        self.return_storage = if indirect {
            function.get_nth_param(0)
        } else {
            None
        };
        self.c_result = None;
        self.scopes = vec![vec![]];
        self.debug_function(output, function)?;
        self.location(output.span);
        self.builder
            .position_at_end(self.context.append_basic_block(function, "entry"));
        let this = function.get_nth_param(u32::from(indirect)).unwrap();
        let layout = self.structs[&future.name];
        let count = future.fields.len() as u32;
        let state = self
            .builder
            .build_struct_gep(layout, this, count, "async.state")?;
        let current = self
            .builder
            .build_load(self.context.i32_type(), state, "state")?;
        let ready = self.builder.build_int_compare(
            IntPredicate::LLVMIntEQ,
            current,
            self.context.i32_type().const_int(READY, false),
            "async.ready",
        )?;
        self.guard(
            ready,
            "async output",
            "a future's output was taken before it was ready, or twice",
        )?;
        let frame_ptr = self
            .builder
            .build_struct_gep(layout, this, count + 2, "async.frame")?;
        let slot = self.builder.build_struct_gep(
            frame.frame,
            frame_ptr,
            frame
                .output
                .ok_or_else(|| error("async output without a slot"))?,
            "async.output",
        )?;
        let value = self.load(slot, &output.ret)?;
        self.builder
            .build_store(state, self.context.i32_type().const_int(FINISHED, false))?;
        self.return_value(value)?;
        self.scopes.clear();
        Ok(())
    }
    /// `drop(&mut self)`: what the future still owns depends on its state:
    /// the arguments before it starts, its result once ready, and the live
    /// bindings while suspended.
    fn drop_function(&mut self, future: &Struct, frame: &FrameLayout<'ctx>) -> Result<()> {
        let drop = self.future_function(&future.name, "drop").unwrap();
        let function = self.functions[&drop.name];
        self.function = Some(function);
        self.return_type = Type::Void;
        self.return_storage = None;
        self.c_result = None;
        self.scopes = vec![vec![]];
        self.debug_function(drop, function)?;
        self.location(drop.span);
        self.builder
            .position_at_end(self.context.append_basic_block(function, "entry"));
        let this = function.get_nth_param(0).unwrap();
        let layout = self.structs[&future.name];
        let count = future.fields.len() as u32;
        let i32_ty = self.context.i32_type();
        let state = self
            .builder
            .build_struct_gep(layout, this, count, "async.state")?;
        let current = self.builder.build_load(i32_ty, state, "state")?;
        let frame_ptr = self
            .builder
            .build_struct_gep(layout, this, count + 2, "async.frame")?;
        let unstarted = self.bb("drop.unstarted");
        let ready = self.bb("drop.ready");
        let suspended = self.bb("drop.suspended");
        let end = self.bb("drop.end");
        self.builder.build_switch(
            current,
            suspended,
            &[
                (i32_ty.const_int(START, false), unstarted),
                (i32_ty.const_int(READY, false), ready),
                (i32_ty.const_int(FINISHED, false), end),
            ],
        )?;
        self.builder.position_at_end(unstarted);
        for (index, field) in future.fields.iter().enumerate().rev() {
            let ptr = self
                .builder
                .build_struct_gep(layout, this, index as u32, "drop.argument")?;
            self.drop_ptr(ptr, &field.ty)?;
        }
        self.builder.build_unconditional_branch(end)?;
        self.builder.position_at_end(ready);
        if let Some(slot) = frame.output
            && let Some(output) = self.future_function(&future.name, "output")
            && self.needs_drop(&output.ret)
        {
            let ptr = self
                .builder
                .build_struct_gep(frame.frame, frame_ptr, slot, "drop.output")?;
            self.drop_ptr(ptr, &output.ret)?;
        }
        self.builder.build_unconditional_branch(end)?;
        self.builder.position_at_end(suspended);
        let owned: Vec<(Value<'ctx>, Type, Value<'ctx>)> = {
            let mut owned = Vec::new();
            for (slot, ty, flag) in frame.owned.iter().rev() {
                let ptr =
                    self.builder
                        .build_struct_gep(frame.frame, frame_ptr, *slot, "drop.binding")?;
                let flag =
                    self.builder
                        .build_struct_gep(frame.frame, frame_ptr, *flag, "drop.flag")?;
                owned.push((ptr, ty.clone(), flag));
            }
            for (index, (ty, flag)) in frame.params.iter().enumerate().rev() {
                if let Some(flag) = flag {
                    let ptr = self.builder.build_struct_gep(
                        layout,
                        this,
                        index as u32,
                        "drop.argument",
                    )?;
                    let flag = self.builder.build_struct_gep(
                        frame.frame,
                        frame_ptr,
                        *flag,
                        "drop.flag",
                    )?;
                    owned.push((ptr, ty.clone(), flag));
                }
            }
            owned
        };
        for (ptr, ty, flag) in owned {
            let binding = Binding {
                name: String::new(),
                ty,
                ptr,
                live: Some(flag),
            };
            self.drop_binding(&binding)?;
        }
        self.builder
            .build_store(state, i32_ty.const_int(FINISHED, false))?;
        self.builder.build_unconditional_branch(end)?;
        self.builder.position_at_end(end);
        self.builder.build_return(None)?;
        self.scopes.clear();
        Ok(())
    }
}
