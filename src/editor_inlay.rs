//! Binding hints use checked types and validate source tokens before displaying
//! them, so explicit annotations and semantic lowering never produce duplicates.
use super::source_names;
use crate::ast::*;
use crate::lexer::{Token, TokenKind};
use std::collections::BTreeMap;

pub(super) struct Hint {
    pub offset: usize,
    pub label: String,
}

pub(super) struct Index<'a> {
    tokens: &'a [Token],
    start: usize,
    length: usize,
    aliases: BTreeMap<String, String>,
    hints: BTreeMap<usize, Option<String>>,
}

impl<'a> Index<'a> {
    pub fn new(
        tokens: &'a [Token],
        start: usize,
        length: usize,
        namespaces: Option<&BTreeMap<String, String>>,
    ) -> Self {
        let mut aliases = BTreeMap::new();
        for (alias, canonical) in namespaces.into_iter().flatten() {
            // The empty alias denotes this source's own package and sorts first.
            aliases
                .entry(canonical.clone())
                .or_insert_with(|| alias.clone());
        }
        Self {
            tokens,
            start,
            length,
            aliases,
            hints: BTreeMap::new(),
        }
    }

    pub fn finish(self) -> Vec<Hint> {
        self.hints
            .into_iter()
            .filter_map(|(offset, label)| label.map(|label| Hint { offset, label }))
            .collect()
    }

    fn tokens_in(&self, span: Span) -> &'a [Token] {
        if span.start < self.start || span.end > self.start + self.length {
            return &[];
        }
        let first = self
            .tokens
            .partition_point(|token| token.span.start < span.start - self.start);
        let last = self
            .tokens
            .partition_point(|token| token.span.end <= span.end - self.start);
        self.tokens.get(first..last).unwrap_or(&[])
    }

    fn add(&mut self, token: &Token, ty: &Type) {
        if *ty == Type::Void
            || !known(ty)
            || !matches!(&token.kind, TokenKind::Ident(name) if name != "_" && !name.contains('$'))
        {
            return;
        }
        let label = format!(": {}", self.display_type(ty));
        // Conflicting generated views of one declaration are ambiguous; omit
        // them instead of choosing an arbitrary specialization's concrete type.
        self.hints
            .entry(token.span.end)
            .and_modify(|previous| {
                if previous.as_ref() != Some(&label) {
                    *previous = None;
                }
            })
            .or_insert(Some(label));
    }

    fn display_type(&self, ty: &Type) -> String {
        let source = source_names(&ty.to_string());
        let mut display = String::new();
        let mut chars = source.char_indices().peekable();
        while let Some((start, ch)) = chars.next() {
            if !ch.is_ascii_alphabetic() && ch != '_' {
                display.push(ch);
                continue;
            }
            let mut end = start + ch.len_utf8();
            while let Some(&(byte, ch)) = chars.peek() {
                if !ch.is_ascii_alphanumeric() && ch != '_' && ch != '.' {
                    break;
                }
                end = byte + ch.len_utf8();
                chars.next();
            }
            let name = &source[start..end];
            if let Some((package, tail)) = name.split_once('.')
                && let Some(alias) = self.aliases.get(package)
            {
                if !alias.is_empty() {
                    display.push_str(alias);
                    display.push('.');
                }
                display.push_str(tail);
            } else {
                display.push_str(name);
            }
        }
        display
    }

    pub fn statement(&mut self, statement: &Stmt) {
        match &statement.kind {
            StmtKind::Let { name, ty, .. } => {
                let tokens: Vec<_> = self
                    .tokens_in(statement.span)
                    .iter()
                    .filter(|token| token.kind != TokenKind::Newline)
                    .take(3)
                    .collect();
                let binding = match tokens.as_slice() {
                    [binding, assign, ..]
                        if ident(binding, name) && assign.kind == TokenKind::Symbol(":=") =>
                    {
                        Some(*binding)
                    }
                    [keyword, binding, assign]
                        if ident(keyword, "let")
                            && ident(binding, name)
                            && assign.kind == TokenKind::Symbol("=") =>
                    {
                        Some(*binding)
                    }
                    // Integer range loops become checked Let nodes that retain
                    // the original `for name in ...` statement span.
                    [keyword, binding, in_keyword]
                        if ident(keyword, "for")
                            && ident(binding, name)
                            && ident(in_keyword, "in") =>
                    {
                        Some(*binding)
                    }
                    _ => None,
                };
                if let Some(binding) = binding {
                    self.add(binding, ty);
                }
            }
            StmtKind::ForEach {
                index,
                name,
                copy,
                iterable,
                ..
            } => {
                let Some(element) = collection_element(&iterable.ty) else {
                    return;
                };
                let mutable = if matches!(
                    iterable.kind,
                    ExprKind::Unary(UnaryOp::Borrow | UnaryOp::BorrowMut, _)
                ) {
                    matches!(iterable.ty, Type::Ref(true, _) | Type::Slice(true, _))
                } else {
                    matches!(iterable.ty, Type::Slice(true, _))
                };
                if *copy && (mutable || !element.is_copy()) {
                    return;
                }
                let ty = if *copy {
                    element.clone()
                } else {
                    Type::Ref(mutable, Box::new(element.clone()))
                };
                let tokens = self.tokens_in(Span {
                    start: statement.span.start,
                    end: iterable.span.start,
                });
                if let Some(binding) = tokens.iter().find(|token| ident(token, name)) {
                    self.add(binding, &ty);
                }
                if let Some(index) = index
                    && let Some(binding) = tokens.iter().find(|token| ident(token, index))
                {
                    self.add(binding, &Type::usize());
                }
            }
            _ => {}
        }
    }
}

fn ident(token: &Token, name: &str) -> bool {
    matches!(&token.kind, TokenKind::Ident(word) if word == name)
}

fn collection_element(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Array(_, element) | Type::Slice(_, element) => Some(element),
        Type::Ref(_, inner) => collection_element(inner),
        _ => None,
    }
}

fn known(ty: &Type) -> bool {
    match ty {
        Type::Unknown => false,
        Type::Array(_, inner)
        | Type::ArrayExpr(_, inner)
        | Type::Slice(_, inner)
        | Type::Ref(_, inner)
        | Type::Raw(_, inner)
        | Type::Option(inner)
        | Type::MaybeUninit(inner) => known(inner),
        Type::Result(ok, error) => known(ok) && known(error),
        Type::Generic(_, arguments) => arguments.iter().all(known),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{Document, position};
    use crate::json::{Value, json};

    fn all(document: &Document) -> Vec<Value> {
        document.inlay_hints(0, 0, u32::MAX, u32::MAX)
    }

    fn hint_at(document: &Document, needle: &str, name: &str) -> Value {
        let offset = document.text.find(needle).unwrap() + name.len();
        let expected = position(&document.text, offset);
        all(document)
            .into_iter()
            .find(|hint| hint["position"] == expected)
            .unwrap_or_else(|| panic!("missing hint at {needle}: {:?}", all(document)))
    }

    #[test]
    fn inferred_bindings_exclude_annotations_and_unknown_types() {
        let document = Document::new(
            "package test\nfn main() {\nlet immutable = 42i32\nmutable := true\nlet explicit: i32 = 1\nnamed: u8 = 2\nu16 typed = 3\nconst compile_time: i32 = 4\nbad := absent\n}\n".into(),
        );
        assert!(!document.diagnostics.is_empty());
        let hints = all(&document);
        assert_eq!(hints.len(), 2, "{hints:?}");
        assert_eq!(
            hint_at(&document, "immutable =", "immutable")["label"],
            ": i32"
        );
        assert_eq!(
            hint_at(&document, "mutable :=", "mutable")["label"],
            ": bool"
        );
        assert!(hints.iter().all(|hint| hint["kind"] == 1));
    }

    #[test]
    fn hint_ranges_use_utf16_and_half_open_boundaries() {
        let document =
            Document::new("package test\r\nfn main() { text := \"😀\"; count := 1u32 }\r\n".into());
        assert!(
            document.diagnostics.is_empty(),
            "{:?}",
            document.diagnostics
        );
        let hint = hint_at(&document, "count :=", "count");
        assert_eq!(hint["position"], json!({"line": 1, "character": 31}));
        assert_eq!(document.inlay_hints(1, 31, 1, 32), vec![hint]);
        assert!(document.inlay_hints(1, 30, 1, 31).is_empty());
        assert!(document.inlay_hints(1, 31, 1, 31).is_empty());
        assert!(document.inlay_hints(2, 0, 1, 0).is_empty());
        assert!(document.inlay_hints(99, 0, 100, 0).is_empty());
    }

    #[test]
    fn loop_bindings_preserve_checked_reference_and_integer_types() {
        let document = Document::new(
            "package test\nfn main() {\nvalues := [2]u8{1, 2}\nfor index, value in values { let copy = *value }\nfor element in &mut values { *element += 1 }\nfor &copied in values {}\nfor step in 0u32..2u32 {}\nfor counter := 0u32; counter < 2; counter += 1 {}\nfor _ in 0u32..2u32 {}\n}\n".into(),
        );
        assert!(
            document.diagnostics.is_empty(),
            "{:?}",
            document.diagnostics
        );
        for (needle, name, label) in [
            ("values :=", "values", ": [2]u8"),
            ("index, value", "index", ": usize"),
            ("value in values", "value", ": &u8"),
            ("copy =", "copy", ": u8"),
            ("element in", "element", ": &mut u8"),
            ("copied in", "copied", ": u8"),
            ("step in", "step", ": u32"),
            ("counter :=", "counter", ": u32"),
        ] {
            assert_eq!(hint_at(&document, needle, name)["label"], label);
        }
        assert_eq!(all(&document).len(), 8);
    }

    #[test]
    fn source_type_names_and_nested_value_bindings_have_one_hint() {
        let document = Document::new(
            "package test\nstruct Box<T> { value: T }\nfn main() {\nwrapped := Box<i32>{value: 1}\nanswer := if true { let nested = 42i32; nested } else { 0i32 }\n}\n".into(),
        );
        assert!(
            document.diagnostics.is_empty(),
            "{:?}",
            document.diagnostics
        );
        assert_eq!(
            hint_at(&document, "wrapped :=", "wrapped")["label"],
            ": Box<i32>"
        );
        assert_eq!(hint_at(&document, "answer :=", "answer")["label"], ": i32");
        assert_eq!(hint_at(&document, "nested =", "nested")["label"], ": i32");
        assert_eq!(all(&document).len(), 3);
    }

    #[test]
    fn generic_instances_do_not_choose_arbitrary_binding_hints() {
        let document = Document::new(
            "package test\nfn identity<T>(value: T) -> T { let local = value; return local }\nfn main() { first := identity(1i32); second := identity(true) }\n".into(),
        );
        assert!(
            document.diagnostics.is_empty(),
            "{:?}",
            document.diagnostics
        );
        assert_eq!(hint_at(&document, "first :=", "first")["label"], ": i32");
        assert_eq!(hint_at(&document, "second :=", "second")["label"], ": bool");
        assert_eq!(all(&document).len(), 2);
    }

    #[test]
    fn source_offsets_do_not_leak_other_files_or_duplicate_lowered_nodes() {
        let text = "let answer = 42i32";
        let (tokens, _) = crate::lexer::lex_recovering(text);
        let mut index = Index::new(&tokens, 100, text.len(), None);
        let statement = |start, ty| Stmt {
            span: Span {
                start,
                end: start + text.len(),
            },
            kind: StmtKind::Let {
                name: "answer".into(),
                ty,
                value: None,
                constant: false,
                mutable: false,
            },
        };
        index.statement(&statement(0, Type::Bool));
        index.statement(&statement(100, Type::u8()));
        index.statement(&statement(100, Type::u8()));
        index.statement(&statement(200, Type::Bool));
        let hints = index.finish();
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].offset, 10);
        assert_eq!(hints[0].label, ": u8");
    }

    #[test]
    fn colliding_import_packages_use_source_aliases_in_nested_types() {
        use crate::{editor::symbols, package, sema};
        use std::sync::Arc;

        let workspace = std::env::temp_dir().join("dodo-inlay-overlay");
        let main = package::source_path(&workspace.join("main.dodo"));
        let left = package::source_path(&workspace.join("left/items.dodo"));
        let right = package::source_path(&workspace.join("right/items.dodo"));
        let text = "package app\nimport \"left/items\" as first\nimport \"right/items\" as second\nfn main() {\nleft := first.make()\nright := second.make()\n}\n";
        let library = "package items\npub struct Item {}\npub struct Box<T> { value: T }\npub fn make() -> Box<Item> { result := Box<Item>{value: Item{}}; return result }\n";
        let overlays = BTreeMap::from([
            (main.clone(), text.into()),
            (left.clone(), library.into()),
            (right, library.into()),
        ]);
        let mut loaded = package::load_with_overlays(&main, &overlays).unwrap();
        let original = loaded.program.clone();
        let diagnostics = sema::check_recovering(&mut loaded.program, usize::BITS);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let symbols = Arc::new(symbols::Index::new(&loaded, &original));
        let source = loaded
            .sources
            .iter()
            .find(|source| source.path == main)
            .unwrap();
        let document = Document::from_checked(
            source.text.clone(),
            source.start,
            &loaded.program,
            vec![],
            symbols.clone(),
        );
        assert_eq!(
            hint_at(&document, "left :=", "left")["label"],
            ": first.Box<first.Item>"
        );
        assert_eq!(
            hint_at(&document, "right :=", "right")["label"],
            ": second.Box<second.Item>"
        );
        assert_eq!(all(&document).len(), 2);

        let source = loaded
            .sources
            .iter()
            .find(|source| source.path == left)
            .unwrap();
        let document = Document::from_checked(
            source.text.clone(),
            source.start,
            &loaded.program,
            vec![],
            symbols,
        );
        assert_eq!(
            hint_at(&document, "result :=", "result")["label"],
            ": Box<Item>"
        );
        assert_eq!(all(&document).len(), 1);
    }
}
