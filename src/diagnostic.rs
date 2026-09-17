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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelStyle {
    Primary,
    Secondary,
}

/// A source location that helps explain a diagnostic. Spans use the same byte
/// offsets as the diagnostic's primary span, including package source offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticLabel {
    pub span: Span,
    pub message: String,
    pub style: LabelStyle,
}

/// A direct local binding affected by an operation. These spans use the same
/// (possibly package-wide) byte offsets as the diagnostic's primary span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticBinding {
    pub name: String,
    pub declaration: Span,
    pub usage: Span,
}

/// Machine-readable meaning, independent of the diagnostic's displayed text.
/// Payloads are boxed to keep recursive parser results compact. Names may be
/// qualified; binding metadata is absent for indirect storage such as fields
/// and dereferences. Unresolved receivers have a name only for a simple path.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum DiagnosticKind {
    #[default]
    Unclassified,
    ImmutableAssignment(Option<Box<DiagnosticBinding>>),
    ImmutableBorrow(Option<Box<DiagnosticBinding>>),
    UnknownFunction(Box<str>),
    UnknownBinding(Box<str>),
    UnknownType(Box<str>),
    UnknownStruct(Box<str>),
    UnknownVariant(Box<str>),
    UnresolvedReceiver(Option<Box<str>>),
}

impl DiagnosticKind {
    /// Stable public codes for editor and library consumers. Unclassified
    /// diagnostics intentionally have no code until their meaning is modeled.
    pub fn code(&self) -> Option<&'static str> {
        Some(match self {
            Self::Unclassified => return None,
            Self::ImmutableAssignment(_) => "immutable-assignment",
            Self::ImmutableBorrow(_) => "immutable-borrow",
            Self::UnknownFunction(_) => "unknown-function",
            Self::UnknownBinding(_) => "unknown-binding",
            Self::UnknownType(_) => "unknown-type",
            Self::UnknownStruct(_) => "unknown-struct",
            Self::UnknownVariant(_) => "unknown-variant",
            Self::UnresolvedReceiver(_) => "unresolved-receiver",
        })
    }
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub kind: DiagnosticKind,
    pub span: Span,
    // A boxed message keeps recursive parser results compact alongside metadata.
    pub message: Box<str>,
    pub notes: Vec<String>,
    pub labels: Vec<DiagnosticLabel>,
}
impl Diagnostic {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            kind: DiagnosticKind::Unclassified,
            span,
            message: message.into().into_boxed_str(),
            notes: vec![],
            labels: vec![],
        }
    }
    pub fn warning(span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            ..Self::new(span, message)
        }
    }
    pub fn with_kind(mut self, kind: DiagnosticKind) -> Self {
        self.kind = kind;
        self
    }
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
    /// Explain a related source location, underlined with dashes.
    pub fn label(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(DiagnosticLabel {
            span,
            message: message.into(),
            style: LabelStyle::Secondary,
        });
        self
    }
    /// Explain the diagnostic's primary location, underlined with carets.
    pub fn primary_label(mut self, message: impl Into<String>) -> Self {
        self.labels
            .retain(|label| label.style != LabelStyle::Primary);
        self.labels.push(DiagnosticLabel {
            span: self.span,
            message: message.into(),
            style: LabelStyle::Primary,
        });
        self
    }
    pub fn render(&self, path: &str, source: &str) -> String {
        self.render_with_sources(&[(path, source, 0)])
    }

    /// Resolve every label independently so imported declarations keep their
    /// own paths and local line numbers. Sources are (path, text, byte offset).
    pub(crate) fn render_with_sources(&self, sources: &[(&str, &str, usize)]) -> String {
        let mut result = format!("{}: {}\n", self.severity.label(), self.message);
        let mut labels: Vec<_> = self.labels.iter().collect();
        let primary = DiagnosticLabel {
            span: self.span,
            message: String::new(),
            style: LabelStyle::Primary,
        };
        if !labels
            .iter()
            .any(|label| label.style == LabelStyle::Primary)
        {
            labels.push(&primary);
        }
        // Keep a borrow's beginning, conflicting action, and later use in
        // reading order even when the diagnostic was built primary-first.
        labels.sort_by_key(|label| (label.span.start, label.span.end));
        for label in labels {
            let source = sources.iter().find(|(_, text, offset)| {
                label.span.start >= *offset && label.span.start <= offset.saturating_add(text.len())
            });
            if let Some(&(path, text, offset)) = source.or_else(|| sources.first()) {
                let span = Span {
                    start: label.span.start.saturating_sub(offset),
                    end: label.span.end.saturating_sub(offset),
                };
                render_label(&mut result, path, text, span, label);
            }
        }
        for note in &self.notes {
            result.push_str(&format!("  = note: {note}\n"));
        }
        result
    }
}

fn render_label(
    result: &mut String,
    path: &str,
    source: &str,
    span: Span,
    label: &DiagnosticLabel,
) {
    // Diagnostics can point at EOF or originate in a partially edited buffer.
    // Snap malformed byte offsets outwards to whole UTF-8 characters.
    let mut start = span.start.min(source.len());
    while !source.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = span.end.min(source.len()).max(start);
    while !source.is_char_boundary(end) {
        end += 1;
    }
    let mut line = source[..start]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1;
    let mut line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
    let column = source[line_start..start].chars().count() + 1;
    let last_line = line
        + source[start..end]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count();
    let gutter = last_line.to_string().len().max(3);
    let blank = " ".repeat(gutter);
    let arrow = match label.style {
        LabelStyle::Primary => "-->",
        LabelStyle::Secondary => ":::",
    };
    let marker = match label.style {
        LabelStyle::Primary => "^",
        LabelStyle::Secondary => "-",
    };
    result.push_str(&format!(" {arrow} {path}:{line}:{column}\n{blank} |\n"));
    loop {
        let line_end = source[line_start..]
            .find('\n')
            .map_or(source.len(), |index| line_start + index);
        let display_end = if source[line_start..line_end].ends_with('\r') {
            line_end - 1
        } else {
            line_end
        };
        let highlight_start = start.max(line_start).min(display_end);
        let highlight_end = end.min(display_end).max(highlight_start);
        let prefix = expand_tabs(&source[line_start..highlight_start]);
        let marked = expand_tabs(&source[line_start..highlight_end]);
        let padding = prefix.chars().count();
        let width = marked.chars().count().saturating_sub(padding).max(1);
        let text = expand_tabs(&source[line_start..display_end]);
        let more = end > line_end.saturating_add(1) && line_end < source.len();
        let message = if more || label.message.is_empty() {
            String::new()
        } else {
            format!(" {}", label.message)
        };
        result.push_str(&format!(
            "{line:>gutter$} | {text}\n{blank} | {}{}{message}\n",
            " ".repeat(padding),
            marker.repeat(width),
        ));
        if !more {
            break;
        }
        line += 1;
        line_start = line_end + 1;
    }
}

fn expand_tabs(text: &str) -> String {
    let mut result = String::new();
    let mut column = 0;
    for character in text.chars() {
        if character == '\t' {
            let width = 4 - column % 4;
            result.push_str(&" ".repeat(width));
            column += width;
        } else {
            result.push(character);
            column += 1;
        }
    }
    result
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for Diagnostic {}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(source: &str, needle: &str) -> Span {
        let start = source.find(needle).unwrap();
        Span {
            start,
            end: start + needle.len(),
        }
    }

    #[test]
    fn renders_borrow_locations_in_source_order() {
        let source = "view := &value\nvalue = replacement\nconsume(view)\n";
        let target = source.find("value =").unwrap();
        let diagnostic = Diagnostic::new(
            Span {
                start: target,
                end: target + "value".len(),
            },
            "cannot modify borrowed value",
        )
        .primary_label("cannot modify value while this borrow is live")
        .label(span(source, "&value"), "shared borrow begins here")
        .label(Span { start: 43, end: 47 }, "borrow is used here")
        .note("finish using the borrow before assigning");
        let rendered = diagnostic.render("borrow.dodo", source);
        assert_eq!(
            rendered,
            concat!(
                "error: cannot modify borrowed value\n",
                " ::: borrow.dodo:1:9\n    |\n  1 | view := &value\n",
                "    |         ------ shared borrow begins here\n",
                " --> borrow.dodo:2:1\n    |\n  2 | value = replacement\n",
                "    | ^^^^^ cannot modify value while this borrow is live\n",
                " ::: borrow.dodo:3:9\n    |\n  3 | consume(view)\n",
                "    |         ---- borrow is used here\n",
                "  = note: finish using the borrow before assigning\n",
            )
        );
    }

    #[test]
    fn renders_each_label_on_a_shared_line() {
        let source = "take(value, &value)";
        let rendered = Diagnostic::new(span(source, "value,"), "conflicting arguments")
            .label(span(source, "&value"), "borrow begins here")
            .primary_label("value moves here")
            .render("call.dodo", source);
        assert!(rendered.contains("^^^^^^ value moves here"), "{rendered}");
        assert!(rendered.contains("------ borrow begins here"), "{rendered}");
    }

    #[test]
    fn unicode_and_tabs_use_character_widths_and_aligned_markers() {
        let source = "\tview := &café\r\n";
        let rendered = Diagnostic::new(span(source, "&café"), "borrow error")
            .primary_label("shared borrow")
            .render("unicode.dodo", source);
        assert!(rendered.contains("unicode.dodo:1:10"), "{rendered}");
        assert!(rendered.contains("  1 |     view := &café\n"), "{rendered}");
        assert!(
            rendered.contains("    |             ^^^^^ shared borrow\n"),
            "{rendered}"
        );
        assert!(!rendered.contains('\r'));
    }

    #[test]
    fn malformed_spans_clamp_to_complete_characters_and_eof() {
        let source = "é\n";
        let rendered = Diagnostic::new(Span { start: 1, end: 1 }, "inside character")
            .render("utf8.dodo", source);
        assert!(rendered.contains("utf8.dodo:1:1\n"), "{rendered}");
        assert!(rendered.ends_with("    | ^\n"), "{rendered}");
        let rendered = Diagnostic::new(
            Span {
                start: usize::MAX,
                end: 0,
            },
            "at eof",
        )
        .render("utf8.dodo", source);
        assert!(rendered.contains("utf8.dodo:2:1\n"), "{rendered}");
        assert!(rendered.contains("  2 | \n    | ^\n"), "{rendered}");
        let empty = Diagnostic::new(Span::default(), "empty").render("empty.dodo", "");
        assert!(empty.contains("empty.dodo:1:1\n"), "{empty}");
    }

    #[test]
    fn multiline_labels_show_the_entire_range_and_one_message() {
        let source = "return make(\n\tvalue,\n\tother\n)\nnext()\n";
        let rendered = Diagnostic::new(Span { start: 7, end: 30 }, "invalid return")
            .primary_label("borrowed value is returned here")
            .render("return.dodo", source);
        assert!(rendered.contains("  1 | return make("), "{rendered}");
        assert!(rendered.contains("  2 |     value,"), "{rendered}");
        assert!(rendered.contains("  3 |     other"), "{rendered}");
        assert!(rendered.contains("  4 | )"), "{rendered}");
        assert!(!rendered.contains("next()"), "{rendered}");
        assert_eq!(
            rendered.matches("borrowed value is returned here").count(),
            1
        );
    }

    #[test]
    fn replacing_primary_label_keeps_the_latest_explanation() {
        let diagnostic = Diagnostic::new(Span::default(), "error")
            .primary_label("old")
            .primary_label("new");
        assert_eq!(diagnostic.labels.len(), 1);
        assert_eq!(diagnostic.labels[0].message, "new");
    }

    #[test]
    fn warning_severity_is_preserved_with_source_labels() {
        let source = "value\nborrow\n";
        let diagnostic = Diagnostic::warning(span(source, "borrow"), "warning example")
            .primary_label("primary warning location")
            .label(span(source, "value"), "related location")
            .note("warning explanation");
        assert_eq!(diagnostic.severity, Severity::Warning);
        let rendered = diagnostic.render("warning.dodo", source);
        assert!(
            rendered.starts_with("warning: warning example\n"),
            "{rendered}"
        );
        assert!(rendered.contains("----- related location"), "{rendered}");
        assert!(
            rendered.contains("^^^^^^ primary warning location"),
            "{rendered}"
        );
        assert!(
            rendered.ends_with("  = note: warning explanation\n"),
            "{rendered}"
        );
        assert_eq!(
            Diagnostic::new(Span::default(), "error").severity,
            Severity::Error
        );
    }
}
