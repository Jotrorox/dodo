//! Bindings and typed values of the AST checker. Ownership (initialization,
//! moves, loans, and Result obligations) is checked by `flow` afterwards.
use crate::ast::{Span, Type};

#[derive(Clone)]
pub(super) struct Value {
    pub(super) ty: Type,
}
#[derive(Clone)]
pub(super) struct Variable {
    pub(super) id: usize,
    pub(super) name: String,
    pub(super) ty: Type,
    pub(super) immutable: bool,
    pub(super) span: Span,
}
#[derive(Clone)]
pub(super) struct Place {
    pub(super) ty: Type,
    pub(super) mutable: bool,
    pub(super) direct: Option<usize>,
    /// Reached through a raw pointer, which carries no checked owner.
    pub(super) raw: bool,
    /// Inside a closure body: the captured binding this place is reached
    /// through, which decides whether the closure modifies its captures.
    pub(super) capture: Option<String>,
}
