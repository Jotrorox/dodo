//! Ownership facts and pure access/join operations used by the AST checker.
//! Loan dependencies, move origins, and pending Results retain their existing semantics.
use crate::ast::{Span, Type};

#[derive(Clone, Debug)]
pub(super) struct Loan {
    // Element-region provenance is distinct from ordinary owner dependencies.
    pub(super) stored: bool,
    // A dependency keeps a referenced source live without treating the source
    // as the storage of its owning aggregate. Mutating a container does not
    // turn a shared allocator dependency into an exclusive allocator loan.
    pub(super) dependency: bool,
    pub(super) root: usize,
    pub(super) fields: Vec<String>,
    // Only consuming a SplitMut creates these witnesses. Different sides of
    // the same split are disjoint; an ancestor without that witness overlaps.
    pub(super) partitions: Vec<(usize, bool)>,
    pub(super) mutable: bool,
    pub(super) origin: Span,
    pub(super) via: Vec<usize>,
    pub(super) external: Option<String>,
}
#[derive(Clone)]
pub(super) struct Value {
    pub(super) ty: Type,
    pub(super) deps: Vec<Loan>,
}
#[derive(Clone)]
pub(super) struct Variable {
    pub(super) id: usize,
    pub(super) name: String,
    pub(super) ty: Type,
    pub(super) initialized: bool,
    pub(super) moved_at: Option<Span>,
    pub(super) immutable: bool,
    pub(super) deps: Vec<Loan>,
    pub(super) span: Span,
    pub(super) last_use: usize,
    pub(super) last_use_span: Option<Span>,
    pub(super) pending_result: bool,
}
#[derive(Clone)]
pub(super) struct Place {
    pub(super) ty: Type,
    pub(super) loans: Vec<Loan>,
    pub(super) mutable: bool,
    pub(super) direct: Option<usize>,
}
#[derive(Clone, Copy)]
pub(super) enum Access {
    Read,
    Write,
    Borrow(bool),
    Move,
}
pub(super) fn static_loan(span: Span) -> Loan {
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
pub(super) fn overlaps(a: &Loan, b: &Loan) -> bool {
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
pub(super) fn incompatible(access: Access, mutable: bool) -> bool {
    match access {
        Access::Read | Access::Borrow(false) => mutable,
        Access::Write | Access::Move | Access::Borrow(true) => true,
    }
}
pub(super) fn merge_states(mut a: Vec<Vec<Variable>>, b: Vec<Vec<Variable>>) -> Vec<Vec<Variable>> {
    for variable in a.iter_mut().flatten() {
        if let Some(other) = b.iter().flatten().find(|v| v.id == variable.id) {
            variable.initialized &= other.initialized;
            variable.moved_at = variable.moved_at.or(other.moved_at);
            variable.pending_result |= other.pending_result;
            variable.deps.extend(other.deps.clone());
            variable.deps.sort_by_key(|d| {
                (
                    d.root,
                    d.fields.clone(),
                    d.partitions.clone(),
                    d.mutable,
                    d.dependency,
                    d.stored,
                )
            });
            variable.deps.dedup_by(|a, b| {
                a.root == b.root
                    && a.fields == b.fields
                    && a.partitions == b.partitions
                    && a.mutable == b.mutable
                    && a.dependency == b.dependency
                    && a.stored == b.stored
                    && a.external == b.external
            });
        }
    }
    a
}
