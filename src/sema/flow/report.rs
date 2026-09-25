//! Developer-only observability and comparison, not compiler checking flags.
use super::{Limitation, LimitationKind, UninitializedUse};
use crate::ast::{Program, Span};
use crate::diagnostic::Diagnostic;
use crate::sema::{CheckMode, check_program};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkipScope {
    /// Adapter creation failed: even supported-looking bodies are not analyzed.
    Program,
    Body,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BodyStatus {
    Analyzed,
    /// Initialization/move findings only, not a complete safety verdict.
    Diagnostics(Vec<UninitializedUse>),
    Skipped {
        scope: SkipScope,
        limitation: Limitation,
    },
    NoBody,
    Unavailable,
}

impl BodyStatus {
    pub(in crate::sema) fn diagnostic(&self) -> Option<Diagnostic> {
        match self {
            Self::Diagnostics(issues) => issues.first().map(UninitializedUse::diagnostic),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct BodyReport {
    /// Prepared name, including the specialization name for generic instances.
    pub name: String,
    pub span: Span,
    pub status: BodyStatus,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct CoverageReport {
    pub bodies: Vec<BodyReport>,
}

impl CoverageReport {
    /// Counts affected bodies, not unique declarations. Validation/lowering
    /// stops at the first limitation; these are first-blocker frequencies.
    pub fn skip_counts(&self) -> BTreeMap<(SkipScope, LimitationKind), usize> {
        let mut counts = BTreeMap::new();
        for body in &self.bodies {
            if let BodyStatus::Skipped { scope, limitation } = &body.status {
                *counts.entry((*scope, limitation.kind)).or_default() += 1;
            }
        }
        counts
    }
}

#[derive(Debug)]
pub struct CheckReport {
    pub diagnostics: Vec<Diagnostic>,
    /// None means the integration point was not reached (shared preparation
    /// failed), or flow was intentionally disabled in the AST comparison run.
    /// Some(empty) means it was reached but there were no functions.
    pub coverage: Option<CoverageReport>,
}

fn run(program: &mut Program, pointer_bits: u32, mode: CheckMode) -> CheckReport {
    let mut report = CheckReport {
        diagnostics: Vec::new(),
        coverage: None,
    };
    if let Err(error) = check_program(
        program,
        pointer_bits,
        true,
        &mut report.diagnostics,
        mode,
        Some(&mut report.coverage),
    ) {
        report.diagnostics.push(error);
    }
    report
}

/// Combined production recovery checking with internal flow coverage attached.
/// No fallback warnings are added to the user-facing diagnostics.
#[doc(hidden)]
pub fn check_with_report(program: &mut Program, pointer_bits: u32) -> CheckReport {
    run(program, pointer_bits, CheckMode::Combined)
}

#[derive(Debug)]
pub struct CheckerComparison {
    pub ast: CheckReport,
    pub flow: CheckReport,
    pub combined: CheckReport,
}

/// Test/developer helper: fresh clones undergo the same preparation,
/// instantiation and declaration checks. Only body-checker selection differs.
/// Flow-only diagnostics do NOT certify types, borrowing, or unsupported bodies.
/// Always runs all configurations; there is no public safety-disable option and
/// no unchecked, annotated program is returned for compilation.
#[doc(hidden)]
pub fn compare_checkers(program: &Program, pointer_bits: u32) -> CheckerComparison {
    CheckerComparison {
        ast: run(&mut program.clone(), pointer_bits, CheckMode::AstOnly),
        flow: run(&mut program.clone(), pointer_bits, CheckMode::FlowOnly),
        combined: run(&mut program.clone(), pointer_bits, CheckMode::Combined),
    }
}
