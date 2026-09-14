//! Test-only experiment: whole-local definite initialization, not a safety checker.
//! No production module imports this representation. See docs/sema-flow-prototype.md.
mod lower;

use dodoc::ast::{Span, Type};
use std::collections::VecDeque;

pub use lower::lower;

/// A place's type is stored once in `Body::locals`. Projections are unsupported.
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
    /// Only the type matters to this analysis; constants do not prune branches.
    Constant(Type),
    Copy(Place),
    Move(Place),
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
}
#[derive(Debug)]
pub struct Initialization {
    /// None means unreachable; it is distinct from a reachable empty fact set.
    pub incoming: Vec<Option<Vec<bool>>>,
    pub issues: Vec<UninitializedUse>,
}

impl Body {
    pub fn operand_type<'a>(&'a self, operand: &'a Operand) -> &'a Type {
        match operand {
            Operand::Constant(ty) => ty,
            Operand::Copy(place) | Operand::Move(place) => &self.locals[place.0].ty,
        }
    }

    /// Forward must analysis. Entry parameters are initialized; join intersects
    /// reachable predecessors. Assign generates, move/cleanup kill. Loops reach
    /// a fixed point before diagnostics are collected in stable block order.
    pub fn initialization(&self) -> Initialization {
        let mut incoming: Vec<Option<Vec<bool>>> = vec![None; self.blocks.len()];
        incoming[0] = Some(self.locals.iter().map(|local| local.parameter).collect());
        let mut work = VecDeque::from([0]);
        while let Some(id) = work.pop_front() {
            let mut state = incoming[id].clone().unwrap();
            self.transfer(id, &mut state, &mut |_, _| {});
            let successors = match self.blocks[id].terminator {
                Terminator::Goto(next) => vec![next],
                Terminator::Branch { yes, no, .. } => vec![yes, no],
                Terminator::Return(_) | Terminator::Unreachable => vec![],
            };
            for next in successors {
                let joined = match &incoming[next] {
                    None => state.clone(),
                    Some(old) => old.iter().zip(&state).map(|(a, b)| *a && *b).collect(),
                };
                if incoming[next].as_ref() != Some(&joined) {
                    incoming[next] = Some(joined);
                    work.push_back(next);
                }
            }
        }
        let mut issues = Vec::new();
        for (id, state) in incoming.iter().enumerate() {
            if let Some(state) = state {
                self.transfer(id, &mut state.clone(), &mut |place, span| {
                    issues.push(UninitializedUse {
                        name: self.locals[place.0].name.clone(),
                        declaration: self.locals[place.0].span,
                        span,
                    });
                });
            }
        }
        Initialization { incoming, issues }
    }

    fn transfer(&self, id: BlockId, state: &mut [bool], issue: &mut impl FnMut(Place, Span)) {
        let read = |place: Place, state: &[bool], span, issue: &mut dyn FnMut(Place, Span)| {
            if !state[place.0] {
                issue(place, span);
            }
        };
        let operand =
            |value: &Operand, state: &mut [bool], span, issue: &mut dyn FnMut(Place, Span)| {
                if let Operand::Copy(place) | Operand::Move(place) = value {
                    read(*place, state, span, issue);
                    if matches!(value, Operand::Move(_)) {
                        state[place.0] = false;
                    }
                }
            };
        let block = &self.blocks[id];
        for instruction in &block.instructions {
            let span = instruction.span;
            match &instruction.operation {
                Operation::Assign { target, value } => {
                    debug_assert_eq!(&self.locals[target.0].ty, self.operand_type(value));
                    operand(value, state, span, issue);
                    state[target.0] = true;
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
                    state[target.0] = true;
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
                    state[target.0] = true;
                }
                Operation::Cleanup(place) => state[place.0] = false,
            }
        }
        match &block.terminator {
            Terminator::Branch { condition, .. } => operand(condition, state, block.span, issue),
            Terminator::Return(Some(value)) => operand(value, state, block.span, issue),
            _ => (),
        }
    }
}
