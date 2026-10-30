//! Control-flow ownership checking for every checked body: initialization,
//! moves, loans, escapes, destructor order, and Result obligations.
//!
//! The AST checker resolves names and types, desugars method calls and range
//! loops, and lifts closures. Each successfully checked body is then lowered
//! to a graph of basic blocks and analyzed to a fixed point. There is no
//! fallback: every construct the backend accepts has a lowering here.
//! See docs/sema-flow.md.
mod analysis;
mod lower;
mod patterns;

use crate::ast::{Function, Program, Span, Type};
use crate::diagnostic::Diagnostic;

pub(super) use lower::lower;

/// Stable whole-local identity. Shadowed bindings and expression temporaries
/// have distinct places; projections live in `Path`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Place(pub usize);
pub type BlockId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalKind {
    /// A parameter; `capture` marks a closure environment reference.
    Parameter {
        capture: bool,
    },
    Binding {
        immutable: bool,
    },
    /// Single-use expression results. A loan held by a temporary is live for
    /// the remainder of its full expression.
    Temporary,
}

#[derive(Clone, Debug)]
pub struct Local {
    pub name: String,
    pub ty: Type,
    pub span: Span,
    pub kind: LocalKind,
    /// The local's scope, for escape and destruction-order checks.
    pub scope: usize,
}

#[derive(Clone, Debug)]
pub enum Root {
    Local(Place),
    /// A global constant or static, by qualified name.
    Global(String),
}

#[derive(Clone, Debug)]
pub enum Projection {
    /// An explicit `*reference`.
    Deref,
    /// `through_ref`: the base is a reference, dereferenced implicitly.
    Field { name: String, through_ref: bool },
    /// `view`: the base is a slice or a reference, so its loans are the
    /// base's provenance rather than its own storage.
    Index { index: Place, view: bool },
}

/// A checked place expression: a root and its projections.
#[derive(Clone, Debug)]
pub struct Path {
    pub root: Root,
    /// The root binding's own source span, for use labels.
    pub root_span: Span,
    pub projections: Vec<Projection>,
}

impl Path {
    pub fn local(place: Place, span: Span) -> Self {
        Self {
            root: Root::Local(place),
            root_span: span,
            projections: vec![],
        }
    }
    fn root_local(&self) -> Option<Place> {
        match self.root {
            Root::Local(place) => Some(place),
            Root::Global(_) => None,
        }
    }
    fn whole_local(&self) -> Option<Place> {
        self.projections
            .is_empty()
            .then(|| self.root_local())
            .flatten()
    }
    fn indices(&self) -> impl Iterator<Item = Place> + '_ {
        self.projections.iter().filter_map(|p| match p {
            Projection::Index { index, .. } => Some(*index),
            _ => None,
        })
    }
}

/// How the result of a call depends on its arguments' loans.
#[derive(Clone, Debug, Default)]
pub struct Contract {
    /// Arguments whose complete dependencies flow into the result.
    pub from: Vec<usize>,
    /// Arguments whose stored-element dependencies flow into the result.
    pub stored: Vec<usize>,
    /// The result may borrow program-lifetime storage.
    pub static_: bool,
    /// A borrowed result without argument loans borrows static storage.
    pub fallback_static: bool,
    /// `stores(target, sources...)`: deposit source loans into the target.
    pub stores: Option<(usize, Vec<usize>)>,
}

#[derive(Clone, Debug)]
pub enum Operation {
    /// Read a place, moving it when `consume` (only whole locals move).
    Read {
        target: Place,
        path: Path,
        consume: bool,
    },
    /// Require a binding to be initialized: the base of an index is taken
    /// before the index is evaluated.
    Require(Place),
    /// A value with no loans: literals, scalar results, function values.
    Constant { target: Place },
    /// A value borrowing program-lifetime storage, such as a string literal.
    Static { target: Place },
    /// `&path` or `&mut path`. `carried`: the place itself carries borrows,
    /// which the new reference keeps as dependencies.
    Borrow {
        target: Place,
        path: Path,
        mutable: bool,
        carried: bool,
    },
    /// Combine (and consume) temporaries: aggregates, variants, closures.
    Combine { target: Place, sources: Vec<Place> },
    /// Share a value's loans without consuming or accessing it.
    Alias { target: Place, source: Place },
    /// A shared reborrow of an exclusive value.
    Demote { place: Place },
    /// Add the loans held by every local this value borrows, recursively:
    /// copying a reference out of a collection keeps what it references.
    Transitive { place: Place },
    /// Keep only shared loans (`view`) or stored-element loans.
    Filter {
        place: Place,
        shared: bool,
        stored: bool,
    },
    /// Bind a pattern payload. Each path projects the subject's own loans;
    /// no paths keep the subject's complete dependencies.
    Project {
        target: Place,
        source: Place,
        paths: Vec<Vec<String>>,
    },
    /// Open a SplitMut pair: sides receive disjoint partition witnesses.
    Split {
        source: Place,
        bindings: Vec<(Place, Option<bool>)>,
        partition: usize,
    },
    /// Reborrow an argument for the rest of the call expression. The
    /// reservation stands in for the argument's own loans until the call.
    Reserve {
        target: Place,
        source: Place,
        mutable: bool,
    },
    /// A call or invocation. Arguments and reservations are consumed.
    Call {
        target: Place,
        args: Vec<Place>,
        reservations: Vec<Place>,
        contract: Contract,
        /// The result type carries borrows.
        borrowed: bool,
    },
    /// Deposit `sources` into the storage regions owned through `owner`.
    Deposit { owner: Place, sources: Vec<Place> },
    /// Check that `path` may be written. `read` also requires initialization.
    Write { path: Path, read: bool },
    /// Reserve a captured destination until its store.
    Reservation { target: Place, path: Path },
    /// Store through a projected destination, releasing its reservation.
    Store {
        path: Path,
        value: Place,
        reservation: Place,
    },
    /// Replace a whole binding.
    Assign { target: Place, value: Place },
    /// Scope exit: kill the slot. Destructors read their loans here.
    Cleanup(Place),
    /// Release a reservation: its last use.
    Release(Place),
    /// A matched Result is handled, including through a whole borrow.
    Handled(Place),
    /// A scope is about to end: destruction order, unhandled Results, and
    /// loans of outer bindings into departing storage.
    ScopeEnd { scope: usize },
    /// A return or error propagation leaves every enclosing scope.
    Exit {
        scope: usize,
        returned: Option<Place>,
    },
    /// A value block yields `value` from scope `from`, leaving every scope
    /// up to (not including) the block's enclosing scope `to`.
    Yield {
        value: Place,
        from: usize,
        to: usize,
    },
    /// A mutable element borrow cannot outlive its iteration.
    IterationEscape { binding: Place, scope: usize },
    /// A split view cannot be stored outside the loop that opened it.
    SplitEscape {
        partitions: Vec<usize>,
        scope: usize,
    },
}

#[derive(Clone, Debug)]
pub struct Instruction {
    pub operation: Operation,
    pub span: Span,
    /// Evaluation order: the first finding in this order is reported.
    pub order: usize,
    /// Inside a match guard, which may not move, mutate, or borrow mutably.
    pub guard: bool,
    /// Explains a borrow the compiler inserted (closure captures).
    pub note: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Terminator {
    Goto(BlockId),
    /// Both successors stay possible: values never prune paths.
    Branch {
        condition: Option<Place>,
        targets: Vec<BlockId>,
    },
    Return(Option<Place>),
    Unreachable,
}

#[derive(Clone, Debug)]
pub struct BasicBlock {
    pub instructions: Vec<Instruction>,
    pub terminator: Terminator,
    pub span: Span,
}

#[derive(Clone, Debug, Default)]
pub struct Scope {
    pub parent: Option<usize>,
    /// Declaration order.
    pub locals: Vec<Place>,
}

#[derive(Clone, Debug)]
pub struct Body {
    pub name: String,
    pub locals: Vec<Local>,
    pub blocks: Vec<BasicBlock>,
    pub scopes: Vec<Scope>,
    /// Loop statement spans, for loop-carried move diagnostics.
    pub loops: Vec<Span>,
    /// Borrow sources the return contract allows.
    pub from: Vec<String>,
    /// The declared `stores(target, sources...)` effect.
    pub stores: Vec<String>,
    pub return_contract: Span,
    pub inferred_contract: bool,
}

/// One use of an uninitialized or moved binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UninitializedUse {
    pub name: String,
    pub span: Span,
    pub declaration: Span,
    /// One possible move reaching this use, selected in source order at joins.
    pub moved_at: Option<Span>,
}

/// The ownership findings for one body, in source order.
#[derive(Debug, Default)]
pub struct Findings {
    pub uninitialized: Vec<UninitializedUse>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Analyze one lowered body.
pub(super) fn analyze(context: &super::Context, body: &Body) -> Findings {
    analysis::analyze(context, body)
}

/// Developer report: every body's ownership findings, for differential tests.
#[derive(Debug)]
pub struct BodyReport {
    pub name: String,
    pub span: Span,
    pub uninitialized: Vec<UninitializedUse>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
pub struct CheckReport {
    /// What recovering production checking reports.
    pub diagnostics: Vec<Diagnostic>,
    /// None if shared declaration checks failed before bodies were analyzed.
    pub bodies: Option<Vec<BodyReport>>,
}

/// Recovering production checking with every analyzed body's findings attached.
#[doc(hidden)]
pub fn check_with_report(program: &mut Program, pointer_bits: u32) -> CheckReport {
    let mut report = CheckReport {
        diagnostics: Vec::new(),
        bodies: None,
    };
    if let Err(error) = super::check_program(
        program,
        pointer_bits,
        true,
        &mut report.diagnostics,
        Some(&mut report.bodies),
    ) {
        report.diagnostics.push(error);
    }
    report
}

pub(super) fn body_report(function: &Function, findings: &Findings) -> BodyReport {
    BodyReport {
        name: function.name.clone(),
        span: function.span,
        uninitialized: findings.uninitialized.clone(),
        diagnostics: findings.diagnostics.clone(),
    }
}

impl UninitializedUse {
    pub(crate) fn diagnostic(&self) -> Diagnostic {
        let mut diagnostic = Diagnostic::new(
            self.span,
            format!("`{}` is uninitialized or has been moved", self.name),
        )
        .label(
            self.declaration,
            format!("binding `{}` is declared here", self.name),
        );
        if let Some(span) = self.moved_at {
            diagnostic = diagnostic
                .primary_label(format!("cannot use `{}` after it was moved", self.name))
                .label(span, format!("value `{}` is moved here", self.name));
        } else {
            diagnostic = diagnostic
                .primary_label(format!("`{}` is not initialized on every path", self.name));
        }
        diagnostic
    }
}
