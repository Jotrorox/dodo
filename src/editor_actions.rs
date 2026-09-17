//! Diagnostic-backed edits. All edit offsets are local source bytes; the LSP
//! layer converts them to UTF-16 and attaches the open document's version.
use super::{local_span, symbols};
use crate::ast::{Program, Span};
use crate::diagnostic::{Diagnostic, DiagnosticKind};
use crate::lexer::{self, Token, TokenKind};
use crate::{package, parser};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(crate) struct Edit {
    pub path: PathBuf,
    pub span: Span,
    pub new_text: String,
}

pub(crate) struct QuickFix {
    pub title: String,
    pub diagnostic: Diagnostic,
    pub edits: Vec<Edit>,
    pub preferred: bool,
}

/// Only current server diagnostics can authorize an edit. Client diagnostic
/// messages are deliberately not accepted as instructions for source changes.
pub(crate) fn quick_fixes(
    text: &str,
    start: usize,
    index: &symbols::Index,
    diagnostics: &[Diagnostic],
    requested: Span,
    target: &str,
    overlays: &BTreeMap<PathBuf, String>,
) -> Vec<QuickFix> {
    let Some(source) = index.sources.iter().find(|source| source.start == start) else {
        return vec![];
    };
    let (tokens, _) = lexer::lex_recovering(text);
    let mut fixes = vec![];
    let mut seen = BTreeSet::new();
    for diagnostic in diagnostics {
        if diagnostic.span.start < start || diagnostic.span.end > start + text.len() {
            continue;
        }
        let local = local_span(diagnostic.span, start);
        if !intersects(local, requested) {
            continue;
        }
        if let Some((name, edits)) = mutable_binding(text, &tokens, start, index, diagnostic) {
            let title = format!("Make `{name}` mutable");
            if seen.insert((title.clone(), edits[0].span.start)) {
                fixes.push(QuickFix {
                    title,
                    diagnostic: diagnostic.clone(),
                    edits,
                    preferred: true,
                });
            }
        }
        if let Some((namespace, member)) = missing_namespace(&tokens, start, index, diagnostic) {
            let (program, _) = parser::parse_recovering(text);
            let import_aliases: BTreeSet<_> = program
                .imports
                .iter()
                .map(|import| {
                    program
                        .import_aliases
                        .iter()
                        .find(|(path, _)| path == import)
                        .map_or_else(
                            || import.rsplit('/').next().unwrap(),
                            |(_, alias)| alias.as_str(),
                        )
                })
                .collect();
            if namespace == program.package || import_aliases.contains(namespace.as_str()) {
                continue;
            }
            let Some((at, prefix, suffix)) = import_position(text, &tokens) else {
                continue;
            };
            let mut candidates =
                local_import_candidates(&source.path, &program, &namespace, overlays);
            candidates.extend(
                package::bundled_import_candidates(&namespace, target)
                    .into_iter()
                    .map(|(path, sources)| {
                        (path, sources.into_iter().map(str::to_owned).collect())
                    }),
            );
            let candidates: BTreeSet<_> = candidates
                .into_iter()
                .filter(|(path, sources)| {
                    !program.imports.contains(path) && exposes(sources, &namespace, &member)
                })
                .map(|(path, _)| path)
                .collect();
            let preferred = candidates.len() == 1;
            for import in candidates {
                let title = format!("Import `{import}`");
                if seen.insert((title.clone(), at)) {
                    fixes.push(QuickFix {
                        title,
                        diagnostic: diagnostic.clone(),
                        edits: vec![Edit {
                            path: source.path.clone(),
                            span: Span { start: at, end: at },
                            new_text: format!("{prefix}import \"{import}\"{suffix}"),
                        }],
                        preferred,
                    });
                }
            }
        }
    }
    fixes
}

fn intersects(diagnostic: Span, requested: Span) -> bool {
    if requested.start == requested.end {
        diagnostic.start <= requested.start && requested.start <= diagnostic.end
    } else if diagnostic.start == diagnostic.end {
        requested.start <= diagnostic.start && diagnostic.start < requested.end
    } else {
        diagnostic.start < requested.end && requested.start < diagnostic.end
    }
}

fn mutable_binding(
    text: &str,
    tokens: &[Token],
    start: usize,
    index: &symbols::Index,
    diagnostic: &Diagnostic,
) -> Option<(String, Vec<Edit>)> {
    let (borrow, binding) = match &diagnostic.kind {
        DiagnosticKind::ImmutableAssignment(Some(binding)) => (false, binding),
        DiagnosticKind::ImmutableBorrow(Some(binding)) => (true, binding),
        _ => return None,
    };
    let span = local_span(diagnostic.span, start);
    let usage: Vec<_> = tokens
        .iter()
        .filter(|token| {
            token.span.start >= span.start
                && token.span.end <= span.end
                && token.kind != TokenKind::Newline
        })
        .collect();
    // Restrict to direct local storage. Changing a reference binding does not
    // change its pointee's permissions, and changing fields needs type analysis.
    let name = match usage.as_slice() {
        [name] if !borrow => *name,
        [amp, mutable, name]
            if borrow
                && amp.kind == TokenKind::Symbol("&")
                && matches!(&mutable.kind, TokenKind::Ident(word) if word == "mut") =>
        {
            *name
        }
        _ => return None,
    };
    let TokenKind::Ident(ref name_text) = name.kind else {
        return None;
    };
    if *name_text != binding.name || name.span != local_span(binding.usage, start) {
        return None;
    }
    let occurrence = index.occurrences.iter().find(|occurrence| {
        !occurrence.declaration
            && occurrence.span.start == start + name.span.start
            && occurrence.span.end == start + name.span.end
    })?;
    let symbol = &index.symbols[occurrence.symbol];
    if symbol.name != binding.name
        || symbol.span.start < binding.declaration.start
        || symbol.span.end > binding.declaration.end
    {
        return None;
    }
    if symbol.span.start < start || symbol.span.end > start + tokens.last()?.span.end {
        return None;
    }
    let declaration = tokens
        .iter()
        .position(|token| token.span.start + start == symbol.span.start)?;
    let keyword = tokens[..declaration]
        .iter()
        .rev()
        .find(|token| token.kind != TokenKind::Newline)?;
    if !matches!(&keyword.kind, TokenKind::Ident(word) if word == "let") {
        return None;
    }
    let name = &tokens[declaration];
    let removed_end = if text[keyword.span.end..name.span.start]
        .chars()
        .all(char::is_whitespace)
    {
        name.span.start
    } else {
        keyword.span.end
    };
    let mut edits = vec![Edit {
        path: symbol.key.0.clone(),
        span: Span {
            start: keyword.span.start,
            end: removed_end,
        },
        new_text: String::new(),
    }];
    let following = tokens.get(declaration + 1)?;
    match following.kind {
        TokenKind::Symbol("=") => edits.push(Edit {
            path: symbol.key.0.clone(),
            span: following.span,
            new_text: ":=".into(),
        }),
        TokenKind::Symbol(":") => (),
        _ => return None,
    }
    Some((symbol.name.clone(), edits))
}

fn missing_namespace(
    tokens: &[Token],
    start: usize,
    index: &symbols::Index,
    diagnostic: &Diagnostic,
) -> Option<(String, String)> {
    let name = match &diagnostic.kind {
        DiagnosticKind::UnknownFunction(name)
        | DiagnosticKind::UnknownBinding(name)
        | DiagnosticKind::UnknownType(name)
        | DiagnosticKind::UnknownStruct(name)
        | DiagnosticKind::UnknownVariant(name)
        | DiagnosticKind::UnresolvedReceiver(Some(name)) => name,
        _ => return None,
    };
    let span = local_span(diagnostic.span, start);
    let namespace = name.split('.').next()?;
    let begin = tokens.iter().position(|token| {
        token.span.start >= span.start
            && token.span.start < span.end
            && matches!(&token.kind, TokenKind::Ident(name) if name == namespace)
    })?;
    if index
        .occurrences
        .iter()
        .any(|occurrence| occurrence.span.start == start + tokens[begin].span.start)
    {
        return None;
    }
    let next = tokens.get(begin + 1)?;
    let TokenKind::Ident(member) = &tokens.get(begin + 2)?.kind else {
        return None;
    };
    if next.kind != TokenKind::Symbol(".") {
        return None;
    }
    Some((namespace.to_owned(), member.clone()))
}

/// Insert after the package declaration and any contiguous imports. Token
/// offsets retain license headers, comments, semicolon headers and CRLFs.
fn import_position(text: &str, tokens: &[Token]) -> Option<(usize, &'static str, &'static str)> {
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut cursor = 0;
    while tokens.get(cursor)?.kind == TokenKind::Newline {
        cursor += 1;
    }
    if !matches!(&tokens.get(cursor)?.kind, TokenKind::Ident(word) if word == "package") {
        return None;
    }
    cursor += 1;
    if !matches!(tokens.get(cursor)?.kind, TokenKind::Ident(_)) {
        return None;
    }
    cursor += 1;
    loop {
        let last = tokens.get(cursor.checked_sub(1)?)?.span.end;
        let separator = tokens.get(cursor)?;
        let (mut at, mut prefixed) = match separator.kind {
            TokenKind::Newline => (separator.span.end, false),
            TokenKind::Symbol(";") => (separator.span.end, true),
            TokenKind::Eof => (last, true),
            _ => return None,
        };
        if separator.kind == TokenKind::Symbol(";") {
            // Keep a trailing header comment on its original line.
            if let Some(next) = tokens.get(cursor + 1)
                && matches!(next.kind, TokenKind::Newline | TokenKind::Eof)
            {
                at = next.span.end;
                prefixed = next.kind == TokenKind::Eof;
            }
        }
        while matches!(
            tokens.get(cursor)?.kind,
            TokenKind::Newline | TokenKind::Symbol(";")
        ) {
            cursor += 1;
        }
        if !matches!(&tokens.get(cursor)?.kind, TokenKind::Ident(word) if word == "import") {
            return Some((at, if prefixed { newline } else { "" }, newline));
        }
        cursor += 1;
        if !matches!(tokens.get(cursor)?.kind, TokenKind::String(_, false)) {
            return None;
        }
        cursor += 1;
        if matches!(&tokens.get(cursor)?.kind, TokenKind::Ident(word) if word == "as") {
            cursor += 1;
            if !matches!(tokens.get(cursor)?.kind, TokenKind::Ident(_)) {
                return None;
            }
            cursor += 1;
        }
    }
}

fn exposes(sources: &[String], namespace: &str, member: &str) -> bool {
    let mut exported = false;
    for source in sources {
        let Ok(program) = parser::parse(source) else {
            return false;
        };
        if program.package != namespace && namespace != "native" {
            return false;
        }
        exported |= program
            .functions
            .iter()
            .any(|item| item.public && item.name == member)
            || program
                .structs
                .iter()
                .any(|item| item.public && item.name == member)
            || program
                .enums
                .iter()
                .any(|item| item.public && item.name == member)
            || program
                .constants
                .iter()
                .any(|item| item.public && item.name == member);
    }
    exported
}

fn local_import_candidates(
    source: &Path,
    program: &Program,
    namespace: &str,
    overlays: &BTreeMap<PathBuf, String>,
) -> Vec<(String, Vec<String>)> {
    let Some(directory) = source.parent() else {
        return vec![];
    };
    if !source.is_absolute() || source.starts_with("<stdlib>") {
        return vec![];
    }
    let mut parents = BTreeSet::from([String::new()]);
    for import in &program.imports {
        if !matches!(import.split('/').next(), Some("core" | "alloc" | "std"))
            && let Some((parent, _)) = import.rsplit_once('/')
            && parent
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..")
            && !parent.contains('\\')
            && !parent
                .chars()
                .any(|character| character == '"' || character.is_control())
            && Path::new(parent)
                .components()
                .all(|component| matches!(component, std::path::Component::Normal(_)))
        {
            parents.insert(parent.to_owned());
        }
    }
    let mut candidates = vec![];
    for parent in parents.into_iter().take(16) {
        let import = if parent.is_empty() {
            namespace.into()
        } else {
            format!("{parent}/{namespace}")
        };
        let base = directory.join(&import);
        let file = package::source_path(&base.with_extension("dodo"));
        let has_file = overlays.contains_key(&file) || file.is_file();
        if has_file && base.is_dir() {
            continue;
        }
        let texts = if has_file {
            read_source(&file, overlays).map(|text| vec![text])
        } else if base.is_dir() {
            directory_sources(&package::source_path(&base), overlays)
        } else {
            None
        };
        if let Some(texts) = texts {
            candidates.push((import, texts));
        }
    }
    candidates
}

fn read_source(path: &Path, overlays: &BTreeMap<PathBuf, String>) -> Option<String> {
    // Imported candidates are suggestions, so skip enormous or unreadable files
    // instead of delaying an interactive code-action request.
    const MAX_BYTES: usize = 512 * 1024;
    if let Some(source) = overlays.get(path) {
        return (source.len() <= MAX_BYTES).then(|| source.clone());
    }
    if std::fs::metadata(path).ok()?.len() > MAX_BYTES as u64 {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

fn directory_sources(
    directory: &Path,
    overlays: &BTreeMap<PathBuf, String>,
) -> Option<Vec<String>> {
    let mut files = BTreeSet::new();
    for (number, entry) in std::fs::read_dir(directory).ok()?.take(129).enumerate() {
        if number == 128 {
            return None;
        }
        let path = entry.ok()?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "dodo")
            && path.is_file()
        {
            files.insert(package::source_path(&path));
        }
        if files.len() > 64 {
            return None;
        }
    }
    for path in overlays.keys().filter(|path| {
        path.parent() == Some(directory)
            && path
                .extension()
                .is_some_and(|extension| extension == "dodo")
    }) {
        files.insert(path.clone());
    }
    if files.is_empty() || files.len() > 64 {
        return None;
    }
    let mut total = 0;
    files
        .into_iter()
        .map(|path| {
            let source = read_source(&path, overlays)?;
            total += source.len();
            (total <= 4 * 1024 * 1024).then_some(source)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::Document;
    use crate::sema;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "dodo-editor-actions-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(package::source_path(&path))
        }
        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
        fn write(&self, name: &str, text: &str) {
            let path = self.path(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fixes(document: &Document, overlays: &BTreeMap<PathBuf, String>) -> Vec<QuickFix> {
        quick_fixes(
            &document.text,
            document.start,
            &document.index,
            &document.diagnostics,
            Span {
                start: 0,
                end: document.text.len(),
            },
            env!("DODO_HOST_TARGET"),
            overlays,
        )
    }
    fn apply(text: &str, fix: &QuickFix) -> String {
        let mut result = text.to_owned();
        let mut edits: Vec<_> = fix.edits.iter().collect();
        edits.sort_by_key(|edit| std::cmp::Reverse(edit.span.start));
        for edit in edits {
            result.replace_range(edit.span.start..edit.span.end, &edit.new_text);
        }
        result
    }
    fn checked(text: &str) {
        let mut program = parser::parse(text).unwrap();
        sema::check(&mut program).unwrap();
    }
    fn recheck(path: &Path, text: String, overlays: &BTreeMap<PathBuf, String>) {
        let mut overlays = overlays.clone();
        overlays.insert(path.to_path_buf(), text);
        let mut loaded = package::load_with_overlays(path, &overlays).unwrap();
        sema::check(&mut loaded.program).unwrap();
    }

    #[test]
    fn mutable_fixes_use_binding_metadata_after_diagnostics_are_reworded() {
        for source in [
            "package app\nfn main() { let count = 1; count = 2 }\n",
            "package app\nfn main() { let count: i32 = 1; count = 2 }\n",
            "package app\nfn main() { let count = 1; value := &mut count; *value = 2 }\n",
            "package app\nfn main() { let count = 1; { let count = 2; count = 3 }; _ = count }\n",
        ] {
            let mut document = Document::new(source.into());
            let original = fixes(&document, &BTreeMap::new());
            assert_eq!(original.len(), 1, "{:?}", document.diagnostics);
            for diagnostic in &mut document.diagnostics {
                diagnostic.message = "Reworded diagnostic without a quoted name".into();
            }
            let reworded = fixes(&document, &BTreeMap::new());
            assert_eq!(reworded.len(), 1);
            assert_eq!(reworded[0].title, original[0].title);
            assert_eq!(reworded[0].preferred, original[0].preferred);
            let fixed = apply(source, &reworded[0]);
            assert_eq!(fixed, apply(source, &original[0]));
            checked(&fixed);
        }
    }

    #[test]
    fn import_fixes_use_unresolved_names_after_diagnostics_are_reworded() {
        let fixture = Fixture::new();
        let path = fixture.path("main.dodo");
        let overlays = BTreeMap::from([(
            fixture.path("math.dodo"),
            "package math\npub fn answer() -> i32 { return 42 }\npub struct Number { pub value: i32 }\npub const answer_value: i32 = 42\n".into(),
        )]);
        for source in [
            "package app\nfn main() -> i32 { return math.answer() }\n",
            "package app\nfn main(value: math.Number) {}\n",
            "package app\nfn main() { value := math.Number { value: 42 } }\n",
            "package app\nfn main() -> i32 { return math.answer_value }\n",
        ] {
            let mut document = Document::standalone(source.into(), 64, path.clone());
            let original = fixes(&document, &overlays);
            assert_eq!(original.len(), 1, "{:?}", document.diagnostics);
            for diagnostic in &mut document.diagnostics {
                diagnostic.message = "Reworded diagnostic mentioning `unrelated.symbol`".into();
            }
            let reworded = fixes(&document, &overlays);
            assert_eq!(reworded.len(), 1, "{source}");
            assert_eq!(reworded[0].title, original[0].title);
            assert_eq!(reworded[0].preferred, original[0].preferred);
            let fixed = apply(source, &reworded[0]);
            assert_eq!(fixed, apply(source, &original[0]));
            recheck(&path, fixed, &overlays);
        }
    }

    #[test]
    fn diagnostic_wording_alone_does_not_authorize_a_fix() {
        for source in [
            "package app\nfn main() { let count = 1; count = 2 }\n",
            "package app\nfn main() { let count = 1; value := &mut count }\n",
            "package app\nfn main() -> usize { return num.min(2, 3) }\n",
            "package app\nfn main(value: num.ArithmeticError) {}\n",
        ] {
            let mut document = Document::new(source.into());
            assert!(!fixes(&document, &BTreeMap::new()).is_empty(), "{source}");
            for diagnostic in &mut document.diagnostics {
                diagnostic.kind = DiagnosticKind::Unclassified;
            }
            assert!(fixes(&document, &BTreeMap::new()).is_empty(), "{source}");
        }
    }

    #[test]
    fn fixes_immutable_assignment_and_direct_borrow() {
        for source in [
            "package app\nfn main() { let count = 1; count = 2 }\n",
            "package app\nfn main() { let count: i32 = 1; count = 2 }\n",
            "package app\nfn main() { let count = 1; value := &mut count; *value = 2 }\n",
        ] {
            let document = Document::new(source.into());
            let actions = fixes(&document, &BTreeMap::new());
            assert_eq!(actions.len(), 1, "{:?}", document.diagnostics);
            assert_eq!(actions[0].title, "Make `count` mutable");
            assert!(actions[0].preferred);
            checked(&apply(source, &actions[0]));
        }
    }

    #[test]
    fn mutable_fix_resolves_shadowed_declaration_and_requested_range() {
        let source =
            "package app\nfn main() { let count = 1; { let count = 2; count = 3 }; _ = count }\n";
        let document = Document::new(source.into());
        let actions = fixes(&document, &BTreeMap::new());
        assert_eq!(actions.len(), 1, "{:?}", document.diagnostics);
        assert_eq!(
            actions[0].edits[0].span.start,
            source.rfind("let count").unwrap()
        );
        checked(&apply(source, &actions[0]));
        assert!(
            quick_fixes(
                source,
                0,
                &document.index,
                &document.diagnostics,
                Span { start: 0, end: 11 },
                env!("DODO_HOST_TARGET"),
                &BTreeMap::new()
            )
            .is_empty()
        );
    }

    #[test]
    fn does_not_change_shared_references_fields_constants_or_parameters() {
        for source in [
            "package app\nfn main() { let count = 1; let view = &count; *view = 2 }\n",
            "package app\nfn main() { const count: i32 = 1; count = 2 }\n",
            "package app\nfn main(count: &i32) { *count = 2 }\n",
            "package app\nstruct S { n: i32 }\nfn main() { let value = S { n: 1 }; value.n = 2 }\n",
        ] {
            let document = Document::new(source.into());
            assert!(!document.diagnostics.is_empty());
            assert!(fixes(&document, &BTreeMap::new()).is_empty(), "{source}");
        }
    }

    #[test]
    fn suggests_local_import_and_rechecks_with_unsaved_dependency() {
        let fixture = Fixture::new();
        let source = "// license 😀\r\npackage app // header\r\n\r\n// entry point\r\nfn main() -> i32 { return math.answer() }\r\n";
        let path = fixture.path("main.dodo");
        let overlays = BTreeMap::from([(
            fixture.path("math.dodo"),
            "package math\npub fn answer() -> i32 { return 42 }\n".into(),
        )]);
        let document = Document::standalone(source.into(), 64, path.clone());
        let actions = fixes(&document, &overlays);
        assert_eq!(actions.len(), 1, "{:?}", document.diagnostics);
        assert_eq!(actions[0].title, "Import `math`");
        let fixed = apply(source, &actions[0]);
        assert!(fixed.starts_with(
            "// license 😀\r\npackage app // header\r\nimport \"math\"\r\n\r\n// entry point"
        ));
        recheck(&path, fixed, &overlays);
        assert!(!path.exists(), "quick fixes must not write editor buffers");
    }

    #[test]
    fn suggests_directory_packages_and_existing_import_parent() {
        let fixture = Fixture::new();
        fixture.write(
            "lib/other.dodo",
            "package other\npub fn value() -> i32 { return 1 }\n",
        );
        fixture.write(
            "lib/math/arithmetic.dodo",
            "package math\npub fn answer() -> i32 { return 42 }\n",
        );
        let source = "package app\nimport \"lib/other\" // retain comment\nfn main() -> i32 { return math.answer() }\n";
        let path = fixture.path("main.dodo");
        let document = Document::standalone(source.into(), 64, path.clone());
        let actions = fixes(&document, &BTreeMap::new());
        assert_eq!(actions.len(), 1, "{:?}", document.diagnostics);
        assert_eq!(actions[0].title, "Import `lib/math`");
        let fixed = apply(source, &actions[0]);
        assert!(fixed.contains("import \"lib/other\" // retain comment\nimport \"lib/math\"\n"));
        recheck(&path, fixed, &BTreeMap::new());
    }

    #[test]
    fn bundled_import_supports_functions_and_types() {
        let fixture = Fixture::new();
        for source in [
            "package app\nfn main() -> usize { return num.min(2, 3) }\n",
            "package app\nfn main(value: num.ArithmeticError) {}\n",
        ] {
            let path = fixture.path("main.dodo");
            let document = Document::standalone(source.into(), 64, path.clone());
            let actions = fixes(&document, &BTreeMap::new());
            let fix = actions
                .iter()
                .find(|fix| fix.title == "Import `core/num`")
                .unwrap_or_else(|| panic!("{:?}", document.diagnostics));
            recheck(&path, apply(source, fix), &BTreeMap::new());
        }
    }

    #[test]
    fn rejects_private_missing_members_wrong_packages_and_ambiguous_paths() {
        let fixture = Fixture::new();
        let path = fixture.path("main.dodo");
        let source = "package app\nfn main() -> i32 { return math.answer() }\n";
        for dependency in [
            "package math\nfn answer() -> i32 { return 42 }\n",
            "package math\npub fn something_else() -> i32 { return 42 }\n",
            "package other\npub fn answer() -> i32 { return 42 }\n",
        ] {
            fixture.write("math.dodo", dependency);
            let document = Document::standalone(source.into(), 64, path.clone());
            assert!(fixes(&document, &BTreeMap::new()).is_empty());
        }
        fixture.write(
            "math.dodo",
            "package math\npub fn answer() -> i32 { return 42 }\n",
        );
        fixture.write("math/one.dodo", "package math\n");
        let document = Document::standalone(source.into(), 64, path);
        assert!(fixes(&document, &BTreeMap::new()).is_empty());
    }

    #[test]
    fn existing_imports_and_resolved_receivers_are_not_import_suggestions() {
        let fixture = Fixture::new();
        fixture.write(
            "math.dodo",
            "package math\npub fn answer() -> i32 { return 42 }\n",
        );
        for source in [
            "package app\nimport \"math\"\nfn main() -> i32 { return math.answer() }\n",
            "package app\nimport \"math\" as m\nfn main() -> i32 { return math.answer() }\n",
            "package app\nfn main() -> i32 { math := 1; return math.answer() }\n",
        ] {
            let document = Document::standalone(source.into(), 64, fixture.path("main.dodo"));
            assert!(fixes(&document, &BTreeMap::new()).is_empty(), "{source}");
        }
    }

    #[test]
    fn insertion_preserves_semicolon_header_and_trailing_comments() {
        for source in [
            "// license\npackage app; fn main() -> usize { return num.min(2, 3) }\n",
            "package app; // package note\nfn main() -> usize { return num.min(2, 3) }\n",
        ] {
            let document = Document::new(source.into());
            let actions = fixes(&document, &BTreeMap::new());
            assert_eq!(actions.len(), 1, "{:?}", document.diagnostics);
            let fixed = apply(source, &actions[0]);
            assert!(parser::parse(&fixed).is_ok(), "{fixed}");
            if source.contains("package note") {
                assert!(fixed.contains("package app; // package note\nimport"));
            }
        }
    }

    #[test]
    fn bundled_candidates_respect_target_adapters() {
        let linux = package::bundled_import_candidates("windows", "x86_64-unknown-linux-gnu");
        assert!(linux.iter().all(|(path, _)| !path.starts_with("std/fs/")));
        assert!(!package::bundled_import_candidates("native", "x86_64-pc-windows-msvc").is_empty());
        assert!(package::bundled_import_candidates("native", "wasm32-unknown-unknown").is_empty());
    }

    #[test]
    fn edits_are_local_when_other_package_sources_precede_the_document() {
        let fixture = Fixture::new();
        fixture.write("a.dodo", "package app\nfn helper() {}\n");
        let source =
            "package app\nfn main() -> i32 { let count = 1i32; count = 2; return count }\n";
        fixture.write("z.dodo", source);
        let mut loaded = package::load(&fixture.0).unwrap();
        let original = loaded.program.clone();
        let diagnostics = sema::check_recovering(&mut loaded.program, 64);
        let index = std::sync::Arc::new(symbols::Index::new(&loaded, &original));
        let start = loaded
            .sources
            .iter()
            .find(|source| source.path.ends_with("z.dodo"))
            .unwrap()
            .start;
        assert!(start > 0);
        let document =
            Document::from_checked(source.into(), start, &loaded.program, diagnostics, index);
        let actions = fixes(&document, &BTreeMap::new());
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].edits[0].path, fixture.path("z.dodo"));
        assert_eq!(
            actions[0].edits[0].span.start,
            source.find("let count").unwrap()
        );
        checked(&apply(source, &actions[0]));
    }
}
