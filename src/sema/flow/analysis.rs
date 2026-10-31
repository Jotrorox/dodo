//! Forward ownership analysis to a fixed point, then one checking pass that
//! uses backward liveness to decide which loans are still in use.
//!
//! Loans carry their source places through references and aggregates. A
//! binding holds the loans of every value assigned to it; joins union them.
//! An access conflicts with an incompatible loan held by a binding or
//! temporary that is used later, including by its destructor.
use super::*;
use crate::ast::Type;
use crate::sema::Context;
use std::collections::{HashSet, VecDeque};

/// Loan roots: 0 is program-lifetime storage, a local's root is its index
/// plus one, and parameters also have caller-owned roots for what they borrow.
const EXTERNAL: usize = usize::MAX / 2;
const STORED: usize = usize::MAX / 4;
/// Longer projections are truncated to a (more conservative) prefix, which
/// keeps repeated projection in loops finite.
const MAX_FIELDS: usize = 8;

fn local_root(place: Place) -> usize {
    place.0 + 1
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Loan {
    /// Element-region provenance, distinct from ordinary owner dependencies.
    stored: bool,
    /// Keeps a referenced source live without treating it as the storage of
    /// the aggregate that holds it.
    dependency: bool,
    root: usize,
    fields: Vec<String>,
    /// SplitMut witnesses: different sides of one split are disjoint.
    partitions: Vec<(usize, bool)>,
    mutable: bool,
    origin: Span,
    /// Bindings this loan was read through; accessing through a reference
    /// does not conflict with that reference itself.
    via: Vec<usize>,
    external: Option<String>,
}

impl Loan {
    fn same(&self, other: &Self) -> bool {
        self.root == other.root
            && self.fields == other.fields
            && self.partitions == other.partitions
            && self.mutable == other.mutable
            && self.dependency == other.dependency
            && self.stored == other.stored
            && self.external == other.external
            && self.via == other.via
    }
    fn push_field(&mut self, field: &str) {
        if self.fields.len() < MAX_FIELDS {
            self.fields.push(field.into());
        }
    }
}

fn static_loan(span: Span) -> Loan {
    Loan {
        stored: false,
        dependency: false,
        root: 0,
        fields: vec![],
        partitions: vec![],
        mutable: false,
        origin: span,
        via: vec![],
        external: Some("static".into()),
    }
}

fn overlaps(a: &Loan, b: &Loan) -> bool {
    a.root == b.root
        && a.fields
            .iter()
            .zip(&b.fields)
            .all(|(a, b)| a == b || a == "[]" || b == "[]")
        && !a.partitions.iter().any(|(id, side)| {
            b.partitions
                .iter()
                .any(|(other, opposite)| id == other && side != opposite)
        })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Access {
    Read,
    Write,
    Borrow(bool),
    Move,
}

fn incompatible(access: Access, mutable: bool) -> bool {
    match access {
        Access::Read | Access::Borrow(false) => mutable,
        Access::Write | Access::Move | Access::Borrow(true) => true,
    }
}

fn exclusive(access: Access) -> bool {
    matches!(access, Access::Write | Access::Move | Access::Borrow(true))
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Slot {
    initialized: bool,
    maybe_initialized: bool,
    moved_at: Option<Span>,
    deps: Vec<Loan>,
    /// A Result obligation that has not been handled on some path.
    pending: bool,
}

impl Slot {
    fn initialized(deps: Vec<Loan>) -> Self {
        Self {
            initialized: true,
            maybe_initialized: true,
            moved_at: None,
            deps,
            pending: false,
        }
    }
    fn join(&mut self, other: &Self) -> bool {
        let before = (
            self.initialized,
            self.maybe_initialized,
            self.moved_at,
            self.pending,
            self.deps.len(),
        );
        self.initialized &= other.initialized;
        self.maybe_initialized |= other.maybe_initialized;
        self.moved_at = [self.moved_at, other.moved_at]
            .into_iter()
            .flatten()
            .min_by_key(|span| (span.start, span.end));
        self.pending |= other.pending;
        extend(&mut self.deps, other.deps.iter().cloned());
        before
            != (
                self.initialized,
                self.maybe_initialized,
                self.moved_at,
                self.pending,
                self.deps.len(),
            )
    }
}

fn extend(deps: &mut Vec<Loan>, incoming: impl IntoIterator<Item = Loan>) {
    for loan in incoming {
        if !deps.iter().any(|existing| existing.same(&loan)) {
            deps.push(loan);
        }
    }
}

fn dedup(incoming: Vec<Loan>) -> Vec<Loan> {
    let mut deps = Vec::with_capacity(incoming.len());
    extend(&mut deps, incoming);
    deps
}

/// A fixed-size set of locals.
#[derive(Clone, PartialEq, Eq)]
struct Set(Vec<u64>);
impl Set {
    fn new(size: usize) -> Self {
        Self(vec![0; size.div_ceil(64)])
    }
    fn insert(&mut self, place: Place) {
        self.0[place.0 / 64] |= 1 << (place.0 % 64);
    }
    fn remove(&mut self, place: Place) {
        self.0[place.0 / 64] &= !(1 << (place.0 % 64));
    }
    fn contains(&self, place: Place) -> bool {
        self.0[place.0 / 64] & (1 << (place.0 % 64)) != 0
    }
    fn union(&mut self, other: &Self) -> bool {
        let mut changed = false;
        for (a, b) in self.0.iter_mut().zip(&other.0) {
            let next = *a | *b;
            changed |= next != *a;
            *a = next;
        }
        changed
    }
}

fn successors(terminator: &Terminator) -> Vec<BlockId> {
    match terminator {
        Terminator::Goto(next) => vec![*next],
        Terminator::Branch { targets, .. } => targets.clone(),
        Terminator::Return(_) | Terminator::Unreachable => vec![],
    }
}

fn path_uses(path: &Path, uses: &mut Vec<Place>) {
    if let Some(root) = path.root_local() {
        uses.push(root);
    }
    uses.extend(path.indices());
}

struct Issue {
    key: usize,
    diagnostic: Diagnostic,
}

struct Analysis<'a> {
    context: &'a Context,
    body: &'a Body,
    carries: Vec<bool>,
    drops: Vec<bool>,
    results: Vec<bool>,
    /// Temporaries that never hold loans for conflicts: arguments whose
    /// loans their reservation represents, and dereferenced references
    /// (accessing through a reference does not conflict with it).
    transparent: Set,
    /// Live locals after each instruction, by block.
    live: Vec<Vec<Set>>,
    issues: Vec<Issue>,
    uninitialized: Vec<(usize, UninitializedUse)>,
    /// Order of the instruction being checked.
    order: usize,
}

pub(super) fn analyze(context: &Context, body: &Body) -> Findings {
    let mut analysis = Analysis::new(context, body);
    let incoming = analysis.fixed_point();
    for (id, state) in incoming.iter().enumerate() {
        if let Some(state) = state {
            analysis.block(id, &mut state.clone(), true);
        }
    }
    analysis.findings()
}

impl<'a> Analysis<'a> {
    fn new(context: &'a Context, body: &'a Body) -> Self {
        let mut transparent = Set::new(body.locals.len());
        for block in &body.blocks {
            for instruction in &block.instructions {
                match &instruction.operation {
                    Operation::Reserve { source, .. } => transparent.insert(*source),
                    Operation::Read { path, .. }
                    | Operation::Borrow { path, .. }
                    | Operation::Write { path, .. }
                    | Operation::Reservation { path, .. }
                    | Operation::Store { path, .. } => {
                        if let Some(root) = path.root_local()
                            && body.locals[root.0].kind == LocalKind::Temporary
                            && !path.projections.is_empty()
                        {
                            transparent.insert(root);
                        }
                    }
                    _ => (),
                }
            }
        }
        let mut analysis = Self {
            context,
            body,
            carries: body
                .locals
                .iter()
                .map(|local| context.carries_borrow(&local.ty))
                .collect(),
            drops: body
                .locals
                .iter()
                .map(|local| context.has_drop(&local.ty))
                .collect(),
            results: body
                .locals
                .iter()
                .map(|local| context.contains_result(&local.ty))
                .collect(),
            transparent,
            live: vec![],
            issues: vec![],
            uninitialized: vec![],
            order: 0,
        };
        analysis.live = analysis.liveness();
        analysis
    }

    fn findings(mut self) -> Findings {
        for (order, issue) in &self.uninitialized {
            let diagnostic = self.uninitialized_diagnostic(issue);
            self.issues.push(Issue {
                key: *order,
                diagnostic,
            });
        }
        self.issues.sort_by_key(|issue| issue.key);
        let mut seen = HashSet::new();
        let diagnostics = self
            .issues
            .into_iter()
            .filter(|issue| {
                seen.insert((
                    issue.diagnostic.span.start,
                    issue.diagnostic.span.end,
                    issue.diagnostic.message.clone(),
                ))
            })
            .map(|issue| issue.diagnostic)
            .collect();
        let mut uninitialized: Vec<_> = self
            .uninitialized
            .into_iter()
            .map(|(_, issue)| issue)
            .collect();
        uninitialized
            .sort_by_key(|issue| (issue.span.start, issue.span.end, issue.declaration.start));
        Findings {
            uninitialized,
            diagnostics,
        }
    }

    // ---- liveness -------------------------------------------------------

    fn uses(&self, operation: &Operation) -> Vec<Place> {
        let mut uses = vec![];
        match operation {
            Operation::Read { path, .. }
            | Operation::Borrow { path, .. }
            | Operation::Reservation { path, .. } => path_uses(path, &mut uses),
            Operation::Write { path, read } => {
                if *read || !path.projections.is_empty() {
                    path_uses(path, &mut uses);
                } else {
                    uses.extend(path.indices());
                }
            }
            Operation::Store {
                path,
                value,
                reservation,
            } => {
                path_uses(path, &mut uses);
                uses.push(*value);
                uses.push(*reservation);
            }
            Operation::Combine { sources, .. } => uses.extend(sources),
            Operation::Alias { source, .. }
            | Operation::Project { source, .. }
            | Operation::Split { source, .. }
            | Operation::Reserve { source, .. } => uses.push(*source),
            Operation::Demote { place }
            | Operation::Transitive { place }
            | Operation::Filter { place, .. } => uses.push(*place),
            Operation::Call {
                args, reservations, ..
            } => {
                uses.extend(args);
                uses.extend(reservations);
            }
            Operation::Deposit { owner, sources } => {
                uses.push(*owner);
                uses.extend(sources);
            }
            Operation::Assign { value, .. } => uses.push(*value),
            Operation::Cleanup(place) => {
                if self.drops[place.0] {
                    uses.push(*place);
                }
            }
            Operation::Release(place) => uses.push(*place),
            Operation::Require(place) => uses.push(*place),
            Operation::Constant { .. }
            | Operation::Static { .. }
            | Operation::Handled(_)
            | Operation::ScopeEnd { .. }
            | Operation::Exit { .. }
            | Operation::Yield { .. }
            | Operation::IterationEscape { .. }
            | Operation::SplitEscape { .. } => (),
        }
        uses
    }

    fn defs(operation: &Operation) -> Vec<Place> {
        match operation {
            Operation::Read { target, .. }
            | Operation::Constant { target }
            | Operation::Static { target }
            | Operation::Borrow { target, .. }
            | Operation::Combine { target, .. }
            | Operation::Alias { target, .. }
            | Operation::Project { target, .. }
            | Operation::Reserve { target, .. }
            | Operation::Call { target, .. }
            | Operation::Reservation { target, .. }
            | Operation::Assign { target, .. } => vec![*target],
            Operation::Split { bindings, .. } => bindings.iter().map(|(p, _)| *p).collect(),
            Operation::Cleanup(place) | Operation::Release(place) => vec![*place],
            _ => vec![],
        }
    }

    fn terminator_uses(terminator: &Terminator) -> Vec<Place> {
        match terminator {
            Terminator::Branch {
                condition: Some(place),
                ..
            }
            | Terminator::Return(Some(place)) => vec![*place],
            _ => vec![],
        }
    }

    fn liveness(&self) -> Vec<Vec<Set>> {
        let size = self.body.locals.len();
        let blocks = &self.body.blocks;
        let mut live_in = vec![Set::new(size); blocks.len()];
        let mut changed = true;
        while changed {
            changed = false;
            for id in (0..blocks.len()).rev() {
                let mut live = Set::new(size);
                for next in successors(&blocks[id].terminator) {
                    live.union(&live_in[next]);
                }
                for place in Self::terminator_uses(&blocks[id].terminator) {
                    live.insert(place);
                }
                for instruction in blocks[id].instructions.iter().rev() {
                    for place in Self::defs(&instruction.operation) {
                        live.remove(place);
                    }
                    for place in self.uses(&instruction.operation) {
                        live.insert(place);
                    }
                }
                changed |= live_in[id].union(&live);
            }
        }
        blocks
            .iter()
            .map(|block| {
                let mut live = Set::new(size);
                for next in successors(&block.terminator) {
                    live.union(&live_in[next]);
                }
                for place in Self::terminator_uses(&block.terminator) {
                    live.insert(place);
                }
                let mut after = vec![live.clone(); block.instructions.len()];
                for (index, instruction) in block.instructions.iter().enumerate().rev() {
                    after[index] = live.clone();
                    for place in Self::defs(&instruction.operation) {
                        live.remove(place);
                    }
                    for place in self.uses(&instruction.operation) {
                        live.insert(place);
                    }
                }
                after
            })
            .collect()
    }

    // ---- fixed point ----------------------------------------------------

    fn entry(&self) -> Vec<Slot> {
        self.body
            .locals
            .iter()
            .enumerate()
            .map(|(index, local)| match local.kind {
                LocalKind::Parameter { .. } => {
                    let mut slot = Slot::initialized(self.parameter_loans(Place(index)));
                    slot.pending = self.results[index];
                    slot
                }
                _ => Slot::default(),
            })
            .collect()
    }

    /// A parameter's borrows come from its caller.
    fn parameter_loans(&self, place: Place) -> Vec<Loan> {
        let local = &self.body.locals[place.0];
        let context = self.context;
        let ty = &local.ty;
        let stored = context.contains_storage_witness(ty);
        let borrowed = matches!(ty, Type::Ref(..) | Type::Slice(..));
        let mut deps = if self.carries[place.0] {
            vec![Loan {
                stored: stored && !borrowed,
                dependency: false,
                root: EXTERNAL + place.0,
                fields: vec![],
                partitions: vec![],
                mutable: context.carries_mutable_borrow(ty),
                origin: local.span,
                via: vec![],
                external: Some(local.name.clone()),
            }]
        } else {
            vec![]
        };
        if let Type::Ref(_, inner) | Type::Slice(_, inner) = ty
            && stored
            && context.carries_borrow(inner)
        {
            deps.push(Loan {
                stored: true,
                dependency: true,
                root: STORED + place.0,
                fields: vec![],
                partitions: vec![],
                mutable: context.carries_mutable_borrow(inner),
                origin: local.span,
                via: vec![],
                external: Some(format!("{}.stored", local.name)),
            });
        }
        deps
    }

    fn fixed_point(&mut self) -> Vec<Option<Vec<Slot>>> {
        let blocks = &self.body.blocks;
        let mut incoming: Vec<Option<Vec<Slot>>> = vec![None; blocks.len()];
        incoming[0] = Some(self.entry());
        let mut work = VecDeque::from([0]);
        let mut queued = vec![false; blocks.len()];
        queued[0] = true;
        while let Some(id) = work.pop_front() {
            queued[id] = false;
            let mut state = incoming[id].clone().unwrap();
            self.block(id, &mut state, false);
            for next in successors(&blocks[id].terminator) {
                let changed = match &mut incoming[next] {
                    None => {
                        incoming[next] = Some(state.clone());
                        true
                    }
                    Some(old) => {
                        let mut changed = false;
                        for (slot, other) in old.iter_mut().zip(&state) {
                            changed |= slot.join(other);
                        }
                        changed
                    }
                };
                if changed && !queued[next] {
                    queued[next] = true;
                    work.push_back(next);
                }
            }
        }
        incoming
    }

    // ---- transfer -------------------------------------------------------

    fn block(&mut self, id: BlockId, state: &mut [Slot], check: bool) {
        let block = &self.body.blocks[id];
        for (index, instruction) in block.instructions.iter().enumerate() {
            let at = (id, index);
            self.order = instruction.order;
            self.instruction(instruction, at, state, check);
        }
        self.order = block
            .instructions
            .last()
            .map_or(self.order, |instruction| instruction.order);
        if check {
            match &block.terminator {
                Terminator::Branch {
                    condition: Some(place),
                    ..
                }
                | Terminator::Return(Some(place)) => {
                    self.read(*place, block.span, state);
                }
                _ => (),
            }
        }
    }

    fn report(&mut self, check: bool, _span: usize, diagnostic: Diagnostic) {
        if check {
            self.issues.push(Issue {
                key: self.order,
                diagnostic,
            });
        }
    }

    /// Reading a binding requires it to be initialized on every path.
    fn require(&mut self, place: Place, span: Span, state: &[Slot], check: bool) {
        if check && !state[place.0].initialized {
            let local = &self.body.locals[place.0];
            let issue = UninitializedUse {
                name: local.name.clone(),
                span,
                declaration: local.span,
                moved_at: state[place.0].moved_at,
            };
            if !self.uninitialized.iter().any(|(_, known)| *known == issue) {
                self.uninitialized.push((self.order, issue));
            }
        }
    }

    fn read(&mut self, place: Place, span: Span, state: &[Slot]) {
        self.require(place, span, state, true);
    }

    fn is_temporary(&self, place: Place) -> bool {
        self.body.locals[place.0].kind == LocalKind::Temporary
    }

    fn take(&self, place: Place, state: &mut [Slot]) -> Vec<Loan> {
        let deps = std::mem::take(&mut state[place.0].deps);
        if self.is_temporary(place) {
            state[place.0] = Slot::default();
        } else {
            state[place.0].deps = deps.clone();
        }
        deps
    }

    fn require_path(&mut self, path: &Path, state: &[Slot], check: bool) {
        if let Some(root) = path.root_local()
            && !self.is_temporary(root)
        {
            self.require(root, path.root_span, state, check);
        }
    }

    /// The storage a place expression designates.
    fn loans_of(&self, path: &Path, state: &[Slot]) -> Vec<Loan> {
        let mut loans = match &path.root {
            Root::Local(place) if self.is_temporary(*place) => state[place.0].deps.clone(),
            Root::Local(place) => vec![Loan {
                stored: false,
                dependency: false,
                root: local_root(*place),
                fields: vec![],
                partitions: vec![],
                mutable: !matches!(
                    self.body.locals[place.0].kind,
                    LocalKind::Binding { immutable: true }
                ),
                origin: path.root_span,
                via: vec![],
                external: None,
            }],
            Root::Global(name) => {
                let mut loan = static_loan(path.root_span);
                loan.fields.push(format!("static {name}"));
                vec![loan]
            }
        };
        for (index, projection) in path.projections.iter().enumerate() {
            match projection {
                Projection::Deref => loans = self.provenance(path, index, state),
                Projection::Field { name, through_ref } => {
                    if *through_ref {
                        loans = self.provenance(path, index, state);
                    }
                    for loan in &mut loans {
                        if !loan.dependency {
                            loan.push_field(name);
                        }
                    }
                }
                Projection::Index { view, .. } => {
                    if *view {
                        loans = self.provenance(path, index, state);
                    }
                    for loan in &mut loans {
                        if !loan.dependency {
                            loan.push_field("[]");
                        }
                    }
                }
            }
        }
        loans
    }

    /// The loans of the value read from the first `length` projections.
    fn provenance(&self, path: &Path, length: usize, state: &[Slot]) -> Vec<Loan> {
        if length == 0 {
            return match &path.root {
                Root::Local(place) => {
                    let mut deps = state[place.0].deps.clone();
                    if !self.is_temporary(*place) {
                        for loan in &mut deps {
                            if !loan.via.contains(&place.0) {
                                loan.via.push(place.0);
                            }
                        }
                    }
                    deps
                }
                Root::Global(_) => vec![static_loan(path.root_span)],
            };
        }
        let mut deps = self.provenance(path, length - 1, state);
        if let Projection::Field {
            name,
            through_ref: true,
        } = &path.projections[length - 1]
        {
            // Project fields of an externally borrowed struct; locally
            // assembled aggregates retain their original sources.
            for loan in &mut deps {
                if loan.external.is_some() && loan.root != 0 && !loan.dependency {
                    loan.push_field(name);
                }
            }
        }
        deps
    }

    fn instruction(
        &mut self,
        instruction: &Instruction,
        at: (BlockId, usize),
        state: &mut [Slot],
        check: bool,
    ) {
        let span = instruction.span;
        match &instruction.operation {
            Operation::Read {
                target,
                path,
                consume,
            } => {
                self.require_path(path, state, check);
                let moving = *consume && !self.body.locals[target.0].ty.is_copy();
                let access = if moving { Access::Move } else { Access::Read };
                if check {
                    for loan in self.loans_of(path, state) {
                        self.conflict(&loan, access, span, instruction, at, state, None);
                    }
                }
                let deps = if self.carries[target.0] {
                    self.provenance(path, path.projections.len(), state)
                } else {
                    vec![]
                };
                if let Some(place) = path.whole_local() {
                    if self.is_temporary(place) {
                        state[place.0] = Slot::default();
                    } else if moving {
                        state[place.0] = Slot {
                            moved_at: Some(path.root_span),
                            ..Slot::default()
                        };
                    }
                }
                state[target.0] = Slot::initialized(dedup(deps));
            }
            Operation::Require(place) => self.require(*place, span, state, check),
            Operation::Constant { target } => state[target.0] = Slot::initialized(vec![]),
            Operation::Static { target } => {
                state[target.0] = Slot::initialized(vec![static_loan(span)]);
            }
            Operation::Borrow {
                target,
                path,
                mutable,
                carried,
            } => {
                self.require_path(path, state, check);
                let mut deps = self.loans_of(path, state);
                if *carried {
                    deps.extend(
                        self.provenance(path, path.projections.len(), state)
                            .into_iter()
                            .map(|mut loan| {
                                loan.dependency = true;
                                loan
                            }),
                    );
                }
                for loan in &mut deps {
                    if check {
                        self.conflict(
                            loan,
                            Access::Borrow(*mutable),
                            span,
                            instruction,
                            at,
                            state,
                            None,
                        );
                    }
                    loan.mutable = *mutable && (!loan.dependency || loan.mutable);
                    loan.origin = span;
                }
                state[target.0] = Slot::initialized(dedup(deps));
            }
            Operation::Combine { target, sources } => {
                let mut deps = vec![];
                for source in sources {
                    extend(&mut deps, self.take(*source, state));
                }
                state[target.0] = Slot::initialized(deps);
            }
            Operation::Alias { target, source } => {
                // Accessing through the alias does not conflict with its source.
                let mut deps = state[source.0].deps.clone();
                for loan in &mut deps {
                    if !loan.via.contains(&source.0) {
                        loan.via.push(source.0);
                    }
                }
                state[target.0] = Slot::initialized(dedup(deps));
            }
            Operation::Demote { place } => {
                let deps = std::mem::take(&mut state[place.0].deps);
                state[place.0].deps = dedup(
                    deps.into_iter()
                        .map(|mut loan| {
                            loan.mutable = false;
                            loan
                        })
                        .collect(),
                );
            }
            Operation::Transitive { place } => {
                let mut deps = state[place.0].deps.clone();
                let mut visited = HashSet::new();
                let mut cursor = 0;
                while cursor < deps.len() {
                    let root = deps[cursor].root;
                    cursor += 1;
                    if visited.insert(root)
                        && let Some(local) = self.root_local(root)
                    {
                        let incoming = state[local.0].deps.clone();
                        extend(&mut deps, incoming);
                    }
                }
                state[place.0].deps = deps;
            }
            Operation::Filter {
                place,
                shared,
                stored,
            } => {
                let deps = std::mem::take(&mut state[place.0].deps);
                state[place.0].deps = dedup(
                    deps.into_iter()
                        .filter(|loan| !*stored || loan.stored)
                        .map(|mut loan| {
                            loan.mutable &= !*shared;
                            loan
                        })
                        .collect(),
                );
            }
            Operation::Project {
                target,
                source,
                paths,
            } => {
                let deps = if !self.carries[target.0] {
                    vec![]
                } else if paths.is_empty() {
                    state[source.0].deps.clone()
                } else {
                    paths
                        .iter()
                        .flat_map(|fields| {
                            state[source.0].deps.iter().cloned().map(move |mut loan| {
                                if !loan.dependency {
                                    for field in fields {
                                        loan.push_field(field);
                                    }
                                }
                                loan
                            })
                        })
                        .collect()
                };
                let mut slot = Slot::initialized(dedup(deps));
                slot.pending = self
                    .context
                    .contains_result(super::super::dereferenced(&self.body.locals[target.0].ty));
                state[target.0] = slot;
            }
            Operation::Split {
                source,
                bindings,
                partition,
            } => {
                let deps = state[source.0].deps.clone();
                if check {
                    for loan in &deps {
                        self.conflict(
                            loan,
                            Access::Borrow(true),
                            span,
                            instruction,
                            at,
                            state,
                            Some(*source),
                        );
                    }
                }
                for (binding, side) in bindings {
                    let mut deps = deps.clone();
                    if let Some(side) = side {
                        for loan in &mut deps {
                            if !loan.dependency {
                                loan.partitions.push((*partition, *side));
                            }
                        }
                    }
                    state[binding.0] = Slot::initialized(dedup(deps));
                }
                state[source.0] = Slot::default();
            }
            Operation::Reserve {
                target,
                source,
                mutable,
            } => {
                let mut reserved = vec![];
                for mut loan in state[source.0].deps.clone() {
                    loan.mutable = *mutable && (!loan.dependency || loan.mutable);
                    if check {
                        self.conflict(
                            &loan,
                            Access::Borrow(*mutable),
                            span,
                            instruction,
                            at,
                            state,
                            Some(*source),
                        );
                    }
                    loan.origin = span;
                    reserved.push(loan);
                }
                state[target.0] = Slot::initialized(dedup(reserved));
            }
            Operation::Call {
                target,
                args,
                reservations,
                contract,
                borrowed,
            } => {
                let values: Vec<Vec<Loan>> =
                    args.iter().map(|arg| state[arg.0].deps.clone()).collect();
                if let Some((owner, sources)) = &contract.stores {
                    let incoming = sources
                        .iter()
                        .flat_map(|source| values[*source].clone())
                        .collect();
                    self.deposit(&values[*owner], incoming, span, state, check);
                }
                let mut deps = vec![];
                if *borrowed {
                    for index in &contract.from {
                        extend(&mut deps, values[*index].iter().cloned());
                    }
                    for index in &contract.stored {
                        extend(
                            &mut deps,
                            values[*index].iter().filter(|loan| loan.stored).cloned(),
                        );
                    }
                    if contract.static_ || (contract.fallback_static && deps.is_empty()) {
                        extend(&mut deps, [static_loan(span)]);
                    }
                }
                for place in args.iter().chain(reservations) {
                    if self.is_temporary(*place) {
                        state[place.0] = Slot::default();
                    }
                }
                state[target.0] = Slot::initialized(deps);
            }
            Operation::Deposit { owner, sources } => {
                let incoming = sources
                    .iter()
                    .flat_map(|source| state[source.0].deps.clone())
                    .collect();
                let owner = state[owner.0].deps.clone();
                self.deposit(&owner, incoming, span, state, check);
            }
            Operation::Write { path, read } => {
                if *read || !path.projections.is_empty() {
                    self.require_path(path, state, check);
                }
                if check {
                    for loan in self.loans_of(path, state) {
                        self.conflict(&loan, Access::Write, span, instruction, at, state, None);
                    }
                }
            }
            Operation::Reservation { target, path } => {
                self.require_path(path, state, check);
                let deps = self
                    .loans_of(path, state)
                    .into_iter()
                    .map(|mut loan| {
                        loan.mutable = false;
                        loan.origin = span;
                        loan
                    })
                    .collect();
                state[target.0] = Slot::initialized(dedup(deps));
            }
            Operation::Store {
                path,
                value,
                reservation,
            } => {
                let incoming = state[value.0].deps.clone();
                if self.carries[value.0] {
                    self.store(path, incoming, span, state, check);
                }
                state[value.0] = Slot::default();
                state[reservation.0] = Slot::default();
            }
            Operation::Assign { target, value } => {
                if check && state[target.0].initialized && state[target.0].pending {
                    self.report(
                        check,
                        span.start,
                        Diagnostic::new(span, "overwriting an unhandled Result"),
                    );
                }
                let deps = self.take(*value, state);
                let mut slot = Slot::initialized(deps);
                slot.pending = self.results[target.0];
                state[target.0] = slot;
            }
            Operation::Cleanup(place) | Operation::Release(place) => {
                state[place.0] = Slot::default();
            }
            Operation::Handled(place) => {
                state[place.0].pending = false;
                for loan in state[place.0].deps.clone() {
                    if !loan.stored
                        && loan.fields.is_empty()
                        && loan.root != 0
                        && loan.root <= self.body.locals.len()
                    {
                        state[loan.root - 1].pending = false;
                    }
                }
            }
            Operation::ScopeEnd { scope } => {
                if check {
                    self.scope_end(*scope, span, at, state);
                }
            }
            Operation::Exit { scope, returned } => {
                if check {
                    if let Some(returned) = returned {
                        let deps = state[returned.0].deps.clone();
                        self.returned_borrows(&deps, span);
                    }
                    self.pending_all(*scope, span, state);
                }
            }
            Operation::Yield { value, from, to } => {
                if check {
                    self.yield_escape(*value, *from, *to, span, state);
                }
            }
            Operation::IterationEscape { binding, scope } => {
                if check {
                    self.iteration_escape(*binding, *scope, span, at, state);
                }
            }
            Operation::SplitEscape { partitions, scope } => {
                if check {
                    self.split_escape(partitions, *scope, span, state);
                }
            }
        }
    }

    // ---- conflicts and labels -------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn conflict(
        &mut self,
        place: &Loan,
        access: Access,
        span: Span,
        instruction: &Instruction,
        at: (BlockId, usize),
        state: &[Slot],
        skip: Option<Place>,
    ) {
        if instruction.guard && exclusive(access) {
            self.report(
                true,
                span.start,
                Diagnostic::new(
                    span,
                    "match guards cannot move values, mutate storage, or borrow mutably",
                ),
            );
            return;
        }
        let access = if place.dependency {
            match access {
                Access::Read | Access::Borrow(false) => Access::Borrow(false),
                _ => Access::Borrow(place.mutable),
            }
        } else {
            access
        };
        let defined = Self::defs(&instruction.operation);
        let live = &self.live[at.0][at.1];
        // Bindings first, in declaration order; then expression temporaries.
        let holders = (0..self.body.locals.len())
            .map(Place)
            .filter(|p| !self.is_temporary(*p))
            .chain(
                (0..self.body.locals.len())
                    .map(Place)
                    .filter(|p| self.is_temporary(*p)),
            );
        for holder in holders {
            if !live.contains(holder)
                || !state[holder.0].maybe_initialized
                || self.transparent.contains(holder)
                || Some(holder) == skip
                || defined.contains(&holder)
                || place.via.contains(&holder.0)
            {
                continue;
            }
            let Some(loan) = state[holder.0]
                .deps
                .iter()
                .find(|loan| overlaps(place, loan) && incompatible(access, loan.mutable))
            else {
                continue;
            };
            let kind = if loan.mutable { "mutable" } else { "shared" };
            let diagnostic = if self.is_temporary(holder) {
                Diagnostic::new(span, "overlapping borrows within the same expression")
                    .primary_label(self.access_label(place, access))
                    .label(
                        loan.origin,
                        format!("{kind} borrow begins here and remains live for this expression"),
                    )
            } else {
                let diagnostic = Diagnostic::new(
                    span,
                    format!(
                        "access conflicts with a live {kind} borrow held by `{}`",
                        self.body.locals[holder.0].name
                    ),
                )
                .primary_label(self.access_label(place, access))
                .label(loan.origin, format!("{kind} borrow begins here"));
                self.label_live_borrow(diagnostic, holder, span, at).note(
                    "end the borrow's uses before this access, or borrow disjoint struct fields",
                )
            };
            let diagnostic = match &instruction.note {
                Some(note) => diagnostic.note(note.clone()),
                None => diagnostic,
            };
            self.report(true, span.start, diagnostic);
            return;
        }
    }

    fn access_label(&self, place: &Loan, access: Access) -> String {
        let name = if (1..=self.body.locals.len()).contains(&place.root) {
            format!("`{}`", self.body.locals[place.root - 1].name)
        } else if let Some(name) = place.fields.first().and_then(|f| f.strip_prefix("static ")) {
            format!("static `{}`", crate::sema::source_spelling(name))
        } else {
            place
                .external
                .clone()
                .unwrap_or_else(|| "this value".into())
        };
        let operation = match access {
            Access::Read => "read",
            Access::Write => "modify",
            Access::Move => "move",
            Access::Borrow(false) => "borrow",
            Access::Borrow(true) => "mutably borrow",
        };
        format!("cannot {operation} {name} while this borrow is live")
    }

    /// The next use of `holder` after `at`, in breadth-first program order.
    fn next_use(&self, holder: Place, at: (BlockId, usize)) -> Option<(Span, bool, bool)> {
        let blocks = &self.body.blocks;
        let mut work = VecDeque::from([(at.0, at.1 + 1)]);
        let mut visited = HashSet::new();
        while let Some((id, start)) = work.pop_front() {
            if !visited.insert((id, start)) {
                continue;
            }
            let block = &blocks[id];
            for instruction in &block.instructions[start.min(block.instructions.len())..] {
                if self.uses(&instruction.operation).contains(&holder) {
                    return Some(match &instruction.operation {
                        Operation::Cleanup(_) => (instruction.span, true, false),
                        Operation::Alias { .. } => (instruction.span, false, true),
                        Operation::Read { path, .. }
                        | Operation::Borrow { path, .. }
                        | Operation::Write { path, .. }
                        | Operation::Reservation { path, .. }
                        | Operation::Store { path, .. }
                            if path.root_local() == Some(holder) =>
                        {
                            (path.root_span, false, false)
                        }
                        _ => (instruction.span, false, false),
                    });
                }
            }
            if Self::terminator_uses(&block.terminator).contains(&holder) {
                return Some((block.span, false, false));
            }
            for next in successors(&block.terminator) {
                work.push_back((next, 0));
            }
        }
        None
    }

    fn label_live_borrow(
        &self,
        diagnostic: Diagnostic,
        holder: Place,
        access: Span,
        at: (BlockId, usize),
    ) -> Diagnostic {
        let local = &self.body.locals[holder.0];
        match self.next_use(holder, at) {
            Some((_, true, _)) => diagnostic.label(
                local.span,
                format!(
                    "borrow may be used when `{}` is destroyed at scope exit",
                    local.name
                ),
            ),
            Some((span, _, true)) => {
                diagnostic.label(span, "borrow remains live for this iteration")
            }
            Some((span, ..)) if span.start >= access.end => {
                diagnostic.label(span, "borrow is used here")
            }
            Some((span, ..)) if span.start < access.start => {
                diagnostic.label(span, "borrow is used here on a later loop iteration")
            }
            Some((span, ..)) => diagnostic.label(span, "borrow is used here in the same statement"),
            None => diagnostic.label(local.span, format!("borrow is held by `{}`", local.name)),
        }
    }

    // ---- escapes, destruction, and obligations --------------------------

    /// Scopes from the outermost to `scope`.
    fn chain(&self, scope: usize) -> Vec<usize> {
        let mut chain = vec![scope];
        let mut cursor = scope;
        while let Some(parent) = self.body.scopes[cursor].parent {
            chain.push(parent);
            cursor = parent;
        }
        chain.reverse();
        chain
    }

    fn label_return_contract(&self, diagnostic: Diagnostic) -> Diagnostic {
        diagnostic.label(
            self.body.return_contract,
            format!(
                "{}return contract allows borrows from({})",
                if self.body.inferred_contract {
                    "inferred "
                } else {
                    ""
                },
                self.body.from.join(", ")
            ),
        )
    }

    /// Every loan in a returned value must come from the return contract.
    fn returned_borrows(&mut self, deps: &[Loan], span: Span) {
        for loan in deps {
            let from = &self.body.from;
            let diagnostic = match &loan.external {
                Some(source)
                    if from.contains(source)
                        || source == "static"
                        || source
                            .strip_suffix(".stored")
                            .is_some_and(|s| from.iter().any(|f| f == s)) =>
                {
                    continue;
                }
                Some(source) => {
                    let source_span = self
                        .body
                        .locals
                        .iter()
                        .find(|local| {
                            matches!(local.kind, LocalKind::Parameter { .. })
                                && &local.name == source
                        })
                        .map_or(loan.origin, |local| local.span);
                    Diagnostic::new(
                        span,
                        format!(
                            "returned borrow depends on `{source}`, outside the return contract"
                        ),
                    )
                    .primary_label(format!("returned borrow comes from `{source}`"))
                    .label(
                        source_span,
                        format!("borrowed source `{source}` is declared here"),
                    )
                }
                None => {
                    let mut diagnostic =
                        Diagnostic::new(span, "cannot return a borrow of a local value")
                            .primary_label("local value does not live long enough to be returned")
                            .label(loan.origin, "local borrow begins here");
                    if let Some(source) = self.root_local(loan.root) {
                        let source = &self.body.locals[source.0];
                        diagnostic = diagnostic.label(
                            source.span,
                            format!("local value `{}` is declared here", source.name),
                        );
                    }
                    diagnostic
                }
            };
            let diagnostic = self.label_return_contract(diagnostic);
            self.report(true, span.start, diagnostic);
            return;
        }
    }

    fn root_local(&self, root: usize) -> Option<Place> {
        (1..=self.body.locals.len())
            .contains(&root)
            .then(|| Place(root - 1))
    }

    fn drop_order(&mut self, scope: usize, span: Span, state: &[Slot]) -> bool {
        for scope in self.chain(scope) {
            let locals = &self.body.scopes[scope].locals;
            for (position, variable) in locals.iter().enumerate() {
                if !state[variable.0].maybe_initialized || !self.drops[variable.0] {
                    continue;
                }
                for loan in &state[variable.0].deps {
                    if loan.external.is_some() {
                        continue;
                    }
                    let Some(source) = locals[position + 1..]
                        .iter()
                        .find(|source| local_root(**source) == loan.root)
                    else {
                        continue;
                    };
                    let variable_local = &self.body.locals[variable.0];
                    let source_local = &self.body.locals[source.0];
                    let diagnostic = Diagnostic::new(
                        span,
                        format!(
                            "destructor of `{}` would run after its borrowed source is destroyed",
                            variable_local.name
                        ),
                    )
                    .primary_label("borrowed source is destroyed before its borrower")
                    .label(loan.origin, "borrow begins here")
                    .label(
                        variable_local.span,
                        format!(
                            "`{}` is declared first and will be destroyed last",
                            variable_local.name
                        ),
                    )
                    .label(
                        source_local.span,
                        format!(
                            "borrowed source `{}` is declared later and will be destroyed first",
                            source_local.name
                        ),
                    )
                    .note(
                        "declare the borrowed source before the value whose destructor accesses it",
                    );
                    self.report(true, span.start, diagnostic);
                    return true;
                }
            }
        }
        false
    }

    fn pending_all(&mut self, scope: usize, span: Span, state: &[Slot]) {
        if self.drop_order(scope, span, state) {
            return;
        }
        for scope in self.chain(scope) {
            for variable in &self.body.scopes[scope].locals {
                if state[variable.0].initialized && state[variable.0].pending {
                    let diagnostic = Diagnostic::new(
                        span,
                        format!(
                            "Result `{}` is left unhandled on this exit",
                            self.body.locals[variable.0].name
                        ),
                    );
                    self.report(true, span.start, diagnostic);
                    return;
                }
            }
        }
    }

    fn scope_end(&mut self, scope: usize, span: Span, at: (BlockId, usize), state: &[Slot]) {
        if self.drop_order(scope, span, state) {
            return;
        }
        for variable in &self.body.scopes[scope].locals {
            if state[variable.0].initialized && state[variable.0].pending {
                let local = &self.body.locals[variable.0];
                let diagnostic = Diagnostic::new(
                    local.span,
                    format!("Result `{}` is never handled", local.name),
                )
                .note("match both variants, return the Result, propagate it with `?`, or unwrap it with `!`");
                self.report(true, span.start, diagnostic);
                return;
            }
        }
        let departing: HashSet<usize> = self.body.scopes[scope]
            .locals
            .iter()
            .map(|place| local_root(*place))
            .collect();
        let live = &self.live[at.0][at.1];
        let chain = self.chain(scope);
        for outer in &chain[..chain.len() - 1] {
            for variable in &self.body.scopes[*outer].locals {
                if !state[variable.0].maybe_initialized
                    || (!live.contains(*variable) && !self.drops[variable.0])
                {
                    continue;
                }
                if let Some(loan) = state[variable.0]
                    .deps
                    .iter()
                    .find(|loan| departing.contains(&loan.root) && loan.external.is_none())
                {
                    let diagnostic = Diagnostic::new(
                        span,
                        format!(
                            "borrow in `{}` outlives its source in this scope",
                            self.body.locals[variable.0].name
                        ),
                    )
                    .primary_label("borrowed source cannot outlive this scope")
                    .label(loan.origin, "borrow begins here");
                    let diagnostic = self.label_live_borrow(diagnostic, *variable, span, at);
                    self.report(true, span.start, diagnostic);
                    return;
                }
            }
        }
    }

    /// Locals of the scopes from `from` up to, but not including, `to`.
    fn departing(&self, from: usize, to: usize) -> Vec<Place> {
        let mut locals = vec![];
        let mut cursor = Some(from);
        while let Some(scope) = cursor {
            if scope == to {
                break;
            }
            locals.extend(&self.body.scopes[scope].locals);
            cursor = self.body.scopes[scope].parent;
        }
        locals
    }

    fn yield_escape(&mut self, value: Place, from: usize, to: usize, span: Span, state: &[Slot]) {
        let departing = self.departing(from, to);
        let roots: HashSet<usize> = departing.iter().map(|p| local_root(*p)).collect();
        if let Some(loan) = state[value.0]
            .deps
            .iter()
            .find(|loan| roots.contains(&loan.root) && loan.external.is_none())
        {
            let mut diagnostic = Diagnostic::new(
                span,
                "value block cannot yield a borrow of its local storage",
            )
            .primary_label("local storage does not live beyond this value block")
            .label(loan.origin, "local borrow begins here");
            if let Some(source) = self.root_local(loan.root) {
                let source = &self.body.locals[source.0];
                diagnostic = diagnostic.label(
                    source.span,
                    format!("local value `{}` is declared here", source.name),
                );
            }
            self.report(true, span.start, diagnostic);
            return;
        }
        if departing
            .iter()
            .any(|p| state[p.0].initialized && state[p.0].pending)
        {
            self.report(
                true,
                span.start,
                Diagnostic::new(
                    span,
                    "a Result must be handled before leaving its value block",
                ),
            );
        }
    }

    fn iteration_escape(
        &mut self,
        binding: Place,
        scope: usize,
        span: Span,
        at: (BlockId, usize),
        state: &[Slot],
    ) {
        for outer in self.chain(scope) {
            for variable in &self.body.scopes[outer].locals {
                if !state[variable.0].maybe_initialized {
                    continue;
                }
                if let Some(loan) = state[variable.0]
                    .deps
                    .iter()
                    .find(|loan| loan.via.contains(&binding.0))
                {
                    let diagnostic = Diagnostic::new(
                        span,
                        "mutable foreach element borrow cannot escape its iteration",
                    )
                    .primary_label("element borrow escapes this iteration")
                    .label(loan.origin, "mutable element borrow begins here")
                    .label(
                        self.body.locals[variable.0].span,
                        format!(
                            "borrow is stored in outer binding `{}`",
                            self.body.locals[variable.0].name
                        ),
                    );
                    let diagnostic = self.label_live_borrow(diagnostic, *variable, span, at);
                    self.report(true, span.start, diagnostic);
                    return;
                }
            }
        }
    }

    fn split_escape(&mut self, partitions: &[usize], scope: usize, span: Span, state: &[Slot]) {
        for outer in self.chain(scope) {
            for variable in &self.body.scopes[outer].locals {
                if !state[variable.0].maybe_initialized {
                    continue;
                }
                if let Some(loan) = state[variable.0].deps.iter().find(|loan| {
                    loan.partitions
                        .iter()
                        .any(|(id, _)| partitions.contains(id))
                }) {
                    let diagnostic =
                        Diagnostic::new(span, "split view cannot escape its loop iteration")
                            .label(loan.origin, "source borrow begins here")
                            .label(
                                self.body.locals[variable.0].span,
                                format!(
                                    "view stored in outer binding `{}`",
                                    self.body.locals[variable.0].name
                                ),
                            );
                    self.report(true, span.start, diagnostic);
                    return;
                }
            }
        }
    }

    // ---- storage effects -------------------------------------------------

    fn capture(&self, root: usize) -> Option<&str> {
        let index = root.checked_sub(EXTERNAL)?;
        let local = self.body.locals.get(index)?;
        matches!(local.kind, LocalKind::Parameter { capture: true }).then_some(local.name.as_str())
    }

    /// A projected store may narrow an owned aggregate's lifetime; it never
    /// erases the owner's existing dependencies.
    fn store(
        &mut self,
        path: &Path,
        incoming: Vec<Loan>,
        span: Span,
        state: &mut [Slot],
        check: bool,
    ) {
        for loan in self.loans_of(path, state) {
            if let Some(name) = self.capture(loan.root) {
                let diagnostic = Diagnostic::new(
                    span,
                    format!("a closure cannot store a borrow in captured `{name}`"),
                )
                .note("captured bindings outlive the closure's own borrows; return the value from the closure instead");
                self.report(check, span.start, diagnostic);
                return;
            }
            if !loan.via.is_empty() || loan.external.is_some() {
                let diagnostic = Diagnostic::new(
                    span,
                    "replacing borrow-carrying fields through a reference is not supported in 0.1",
                )
                .note("replace the owned aggregate so its complete borrow dependencies can be checked");
                self.report(check, span.start, diagnostic);
                return;
            }
            if let Some(owner) = self.root_local(loan.root) {
                extend(&mut state[owner.0].deps, incoming.iter().cloned());
            }
        }
    }

    /// Update the original owner, not a temporary reborrow. An external root
    /// has no local binding: every incoming source must be covered by this
    /// body's declared effect, including effects forwarded through helpers.
    fn deposit(
        &mut self,
        owner: &[Loan],
        incoming: Vec<Loan>,
        span: Span,
        state: &mut [Slot],
        check: bool,
    ) {
        for target in owner.iter().filter(|loan| !loan.dependency) {
            if let Some(external) = &target.external {
                let stores = &self.body.stores;
                for loan in &incoming {
                    let allowed = loan.external.as_ref().is_some_and(|source| {
                        source == "static"
                            || source == &format!("{external}.stored")
                            || (stores.first() == Some(external)
                                && stores[1..]
                                    .iter()
                                    .any(|s| source == s || source == &format!("{s}.stored")))
                    });
                    if !allowed {
                        let diagnostic = Diagnostic::new(
                            span,
                            "stored borrow source is outside the declared stores(target, source...) effect",
                        )
                        .label(loan.origin, "this source would escape into caller storage")
                        .note("forward parameter dependencies with stores; local storage cannot escape through a mutation effect");
                        self.report(check, span.start, diagnostic);
                        return;
                    }
                }
            } else if self.root_local(target.root).is_none() && !incoming.is_empty() {
                self.report(
                    check,
                    span.start,
                    Diagnostic::new(span, "cannot identify the owner of this stored borrow"),
                );
                return;
            }
            let mut retained = vec![];
            for loan in &incoming {
                if loan.root == target.root && loan.external.is_none() {
                    self.report(
                        check,
                        span.start,
                        Diagnostic::new(
                            span,
                            "a stored borrow cannot depend on its own container storage",
                        ),
                    );
                    return;
                }
                let mut loan = loan.clone();
                loan.dependency = true;
                loan.stored = true;
                retained.push(loan);
            }
            // Reborrows made before this call must observe the new region union.
            for (index, slot) in state.iter_mut().enumerate() {
                if local_root(Place(index)) == target.root
                    || slot
                        .deps
                        .iter()
                        .any(|d| !d.dependency && d.root == target.root)
                {
                    extend(&mut slot.deps, retained.iter().cloned());
                }
            }
        }
    }

    // ---- initialization diagnostics -------------------------------------

    fn uninitialized_diagnostic(&self, issue: &UninitializedUse) -> Diagnostic {
        // A move that reaches its own (or an earlier) use came around a loop.
        if let Some(moved_at) = issue.moved_at
            && moved_at.start >= issue.span.start
            && let Some(loop_span) = self
                .body
                .loops
                .iter()
                .filter(|span| {
                    span.start <= issue.span.start
                        && moved_at.end <= span.end
                        && issue.declaration.start < span.start
                })
                .min_by_key(|span| span.end - span.start)
        {
            return Diagnostic::new(
                moved_at,
                format!(
                    "`{}` is moved in a loop and may be used again on the next iteration",
                    issue.name
                ),
            )
            .primary_label(format!("value `{}` is moved here", issue.name))
            .label(
                *loop_span,
                "this loop may repeat the move on a later iteration",
            )
            .label(
                issue.declaration,
                format!("binding `{}` is declared outside the loop", issue.name),
            )
            .note("reinitialize the binding before continuing the loop");
        }
        issue.diagnostic()
    }
}
