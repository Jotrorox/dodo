use crate::ast::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    fn label(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub span: Span,
    // Messages are immutable. A boxed str keeps diagnostics compact as they
    // propagate through recursive parser results, even with severity metadata.
    pub message: Box<str>,
    pub notes: Vec<String>,
}
impl Diagnostic {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            span,
            message: message.into().into_boxed_str(),
            notes: vec![],
        }
    }
    pub fn warning(span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            ..Self::new(span, message)
        }
    }
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
    pub fn render(&self, path: &str, source: &str) -> String {
        let start = self.span.start.min(source.len());
        let start = (0..=start)
            .rev()
            .find(|&i| source.is_char_boundary(i))
            .unwrap_or(0);
        let line = source[..start].bytes().filter(|b| *b == b'\n').count() + 1;
        let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
        let column = source[line_start..start].chars().count() + 1;
        let text = source[line_start..].lines().next().unwrap_or("");
        let width = self
            .span
            .end
            .saturating_sub(start)
            .max(1)
            .min(text.len().saturating_sub(start - line_start).max(1));
        let mut result = format!(
            "{}: {}\n --> {path}:{line}:{column}\n  |\n{line:>3} | {text}\n  | {}{}\n",
            self.severity.label(),
            self.message,
            " ".repeat(column - 1),
            "^".repeat(width)
        );
        for note in &self.notes {
            result.push_str(&format!("  = note: {note}\n"));
        }
        result
    }
}
impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for Diagnostic {}
