//! Whole-local control-flow analysis, adopted as an additional check for the
//! supported subset. Borrow safety, Results, and native destruction remain with
//! the AST checker/backend. See docs/sema-flow-prototype.md.
mod lower;

use crate::ast::{Span, Type};
use std::collections::VecDeque;

pub(super) use lower::Adapter;
pub use lower::{lower, lower_for_target};

/// Stable whole-local identity. Projections live in `Destination`; they do not
/// have independent initialization or partial-move facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place(pub usize);
pub type BlockId = usize;

#[derive(Clone, Debug)]
pub struct Local {
    pub name: String,
    pub ty: Type,
    pub span: Span,
    pub parameter: bool,
}
#[derive(Clone, Debug)]
pub enum Operand {
    /// Abstract value with no remaining local reads, including results of pure
    /// scalar operations. Only its type matters; it does not prune branches.
    Constant(Type),
    Copy(Place),
    Move(Place),
}
/// An address is captured before the RHS executes. Its root must be initialized,
/// but a store through it never initializes (or moves) the whole root binding.
#[derive(Clone, Debug)]
pub struct Destination {
    pub root: Place,
    pub projections: Vec<Projection>,
    pub ty: Type,
}
#[derive(Clone, Debug)]
pub enum Projection {
    Field(String),
    Deref,
}
#[derive(Clone, Debug)]
pub enum Operation {
    Assign {
        target: Place,
        value: Operand,
    },
    Borrow {
        target: Place,
        source: Place,
        mutable: bool,
    },
    Call {
        function: String,
        args: Vec<Operand>,
        target: Place,
    },
    Capture {
        target: Place,
        destination: Destination,
    },
    Load {
        target: Place,
        address: Place,
    },
    Store {
        address: Place,
        value: Operand,
    },
    /// Conditional cleanup: kill the slot if initialized; no read obligation.
    /// Custom destructors and reference-containing aggregates are unsupported.
    Cleanup(Place),
}
#[derive(Clone, Debug)]
pub struct Instruction {
    pub operation: Operation,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub enum Terminator {
    Goto(BlockId),
    Branch {
        condition: Operand,
        yes: BlockId,
        no: BlockId,
    },
    Return(Option<Operand>),
    Unreachable,
}
#[derive(Clone, Debug)]
pub struct BasicBlock {
    pub instructions: Vec<Instruction>,
    pub terminator: Terminator,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct Body {
    pub locals: Vec<Local>,
    pub blocks: Vec<BasicBlock>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct UninitializedUse {
    pub name: String,
    pub span: Span,
    pub declaration: Span,
    /// One possible move reaching this use, selected in source order at joins.
    pub moved_at: Option<Span>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Availability {
    pub initialized: bool,
    pub maybe_initialized: bool,
    pub moved_at: Option<Span>,
}
impl Availability {
    fn initialized() -> Self {
        Self {
            initialized: true,
            maybe_initialized: true,
            moved_at: None,
        }
    }
    fn join(self, other: Self) -> Self {
        Self {
            initialized: self.initialized && other.initialized,
            maybe_initialized: self.maybe_initialized || other.maybe_initialized,
            moved_at: [self.moved_at, other.moved_at]
                .into_iter()
                .flatten()
                .min_by_key(|span| (span.start, span.end)),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupKind {
    Never,
    Always,
    Conditional,
}
#[derive(Debug)]
pub struct CleanupSite {
    pub block: BlockId,
    pub instruction: usize,
    pub place: Place,
    pub kind: CleanupKind,
}
#[derive(Debug)]
pub struct Initialization {
    /// None means unreachable; it is distinct from a reachable empty fact set.
    pub incoming: Vec<Option<Vec<Availability>>>,
    pub issues: Vec<UninitializedUse>,
    /// Abstract slot cleanup, not a native destructor/drop plan.
    pub cleanup: Vec<CleanupSite>,
}

impl Body {
    pub fn operand_type<'a>(&'a self, operand: &'a Operand) -> &'a Type {
        match operand {
            Operand::Constant(ty) => ty,
            Operand::Copy(place) | Operand::Move(place) => &self.locals[place.0].ty,
        }
    }

    /// Forward must/may analysis. Entry parameters are initialized; joins
    /// intersect definite facts and union possible facts/move origins from
    /// reachable predecessors. Assign generates, move/cleanup kill. Loops reach
    /// a fixed point before diagnostics and cleanup facts are collected.
    pub fn initialization(&self) -> Initialization {
        let mut incoming: Vec<Option<Vec<Availability>>> = vec![None; self.blocks.len()];
        incoming[0] = Some(
            self.locals
                .iter()
                .map(|local| {
                    if local.parameter {
                        Availability::initialized()
                    } else {
                        Availability::default()
                    }
                })
                .collect(),
        );
        let mut work = VecDeque::from([0]);
        while let Some(id) = work.pop_front() {
            let mut state = incoming[id].clone().unwrap();
            self.transfer(id, &mut state, &mut |_, _, _| {}, &mut |_, _, _| {});
            let successors = match self.blocks[id].terminator {
                Terminator::Goto(next) => vec![next],
                Terminator::Branch { yes, no, .. } => vec![yes, no],
                Terminator::Return(_) | Terminator::Unreachable => vec![],
            };
            for next in successors {
                let joined = match &incoming[next] {
                    None => state.clone(),
                    Some(old) => old.iter().zip(&state).map(|(a, b)| a.join(*b)).collect(),
                };
                if incoming[next].as_ref() != Some(&joined) {
                    incoming[next] = Some(joined);
                    work.push_back(next);
                }
            }
        }
        let mut issues = Vec::new();
        let mut cleanup = Vec::new();
        for (id, state) in incoming.iter().enumerate() {
            if let Some(state) = state {
                self.transfer(
                    id,
                    &mut state.clone(),
                    &mut |place, span, moved_at| {
                        issues.push(UninitializedUse {
                            name: self.locals[place.0].name.clone(),
                            declaration: self.locals[place.0].span,
                            span,
                            moved_at,
                        });
                    },
                    &mut |instruction, place, kind| {
                        cleanup.push(CleanupSite {
                            block: id,
                            instruction,
                            place,
                            kind,
                        });
                    },
                );
            }
        }
        // Block allocation order need not be source order (nested branches).
        issues.sort_by_key(|issue| (issue.span.start, issue.span.end, issue.declaration.start));
        Initialization {
            incoming,
            issues,
            cleanup,
        }
    }

    fn transfer(
        &self,
        id: BlockId,
        state: &mut [Availability],
        issue: &mut impl FnMut(Place, Span, Option<Span>),
        cleanup: &mut impl FnMut(usize, Place, CleanupKind),
    ) {
        let read = |place: Place,
                    state: &[Availability],
                    span,
                    issue: &mut dyn FnMut(Place, Span, Option<Span>)| {
            if !state[place.0].initialized {
                issue(place, span, state[place.0].moved_at);
            }
        };
        let operand = |value: &Operand,
                       state: &mut [Availability],
                       span,
                       issue: &mut dyn FnMut(Place, Span, Option<Span>)| {
            if let Operand::Copy(place) | Operand::Move(place) = value {
                read(*place, state, span, issue);
                if matches!(value, Operand::Move(_)) {
                    state[place.0] = Availability {
                        moved_at: Some(span),
                        ..Availability::default()
                    };
                }
            }
        };
        let block = &self.blocks[id];
        for (index, instruction) in block.instructions.iter().enumerate() {
            let span = instruction.span;
            match &instruction.operation {
                Operation::Assign { target, value } => {
                    debug_assert_eq!(&self.locals[target.0].ty, self.operand_type(value));
                    operand(value, state, span, issue);
                    state[target.0] = Availability::initialized();
                }
                Operation::Borrow {
                    target,
                    source,
                    mutable,
                } => {
                    debug_assert_eq!(
                        self.locals[target.0].ty,
                        Type::Ref(*mutable, Box::new(self.locals[source.0].ty.clone()))
                    );
                    read(*source, state, span, issue);
                    state[target.0] = Availability::initialized();
                }
                Operation::Call {
                    function,
                    args,
                    target,
                } => {
                    debug_assert!(!function.is_empty());
                    for arg in args {
                        operand(arg, state, span, issue);
                    }
                    state[target.0] = Availability::initialized();
                }
                Operation::Capture {
                    target,
                    destination,
                } => {
                    debug_assert_eq!(
                        self.locals[target.0].ty,
                        Type::Ref(true, Box::new(destination.ty.clone()))
                    );
                    read(destination.root, state, span, issue);
                    state[target.0] = Availability::initialized();
                }
                Operation::Load { target, address } => {
                    debug_assert!(matches!(&self.locals[address.0].ty, Type::Ref(_, inner)
                        if **inner == self.locals[target.0].ty));
                    read(*address, state, span, issue);
                    state[target.0] = Availability::initialized();
                }
                Operation::Store { address, value } => {
                    debug_assert!(matches!(&self.locals[address.0].ty, Type::Ref(_, inner)
                        if inner.as_ref() == self.operand_type(value)));
                    read(*address, state, span, issue);
                    operand(value, state, span, issue);
                }
                Operation::Cleanup(place) => {
                    let slot = state[place.0];
                    cleanup(
                        index,
                        *place,
                        if slot.initialized {
                            CleanupKind::Always
                        } else if slot.maybe_initialized {
                            CleanupKind::Conditional
                        } else {
                            CleanupKind::Never
                        },
                    );
                    state[place.0] = Availability::default();
                }
            }
        }
        match &block.terminator {
            Terminator::Branch { condition, .. } => operand(condition, state, block.span, issue),
            Terminator::Return(Some(value)) => operand(value, state, block.span, issue),
            _ => (),
        }
    }
}

impl UninitializedUse {
    pub(crate) fn diagnostic(&self) -> crate::diagnostic::Diagnostic {
        let mut diagnostic = crate::diagnostic::Diagnostic::new(
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
