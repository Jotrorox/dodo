//! Scoped collection edits retain ownership and reject escaping dependencies.
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn rejects(source: &str, diagnostic: &str) {
    let path = std::env::temp_dir().join(format!(
        "dodo-collection-mutation-{}-{}.dodo",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let path = dodoc::package::source_path(&path);
    let overlays = std::collections::BTreeMap::from([(path.clone(), source.to_owned())]);
    let mut loaded = dodoc::package::load_with_overlays(&path, &overlays).unwrap();
    let error = dodoc::sema::check(&mut loaded.program)
        .expect_err("unsafe collection mutation was accepted");
    let rendered = loaded.render(&error);
    assert!(rendered.contains(diagnostic), "{source}\n{rendered}");
}

fn program(element: &str, mutation: &str, body: &str) -> String {
    format!(
        r#"package app
import "alloc/error"
import "alloc/shared_arena"
import "std/collections/vector"
import "std/collections/shared_vector"
import "std/collections/shared_hash_map"
import "std/text_shared"
{mutation}
fn run() -> void!error.AllocError {{
    backing := [0u8; 4096]
    arena := shared_arena.SharedArena.new(&mut backing)?
    values := shared_vector.new::<{element}>(arena.handle())
    {body}
    return ok()
}}
"#
    )
}

const APPEND: &str = r#"
pub struct Edit {
    pub fn apply(&mut self, value: &mut text_shared.String) -> void!error.AllocError {
        return value.append_str("!")
    }
}
"#;

const UPDATE: &str = "result: bool!error.AllocError = values.try_update(0, &mut edit)\nresult?";

#[test]
fn owned_string_mutation_checks() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/stdlib/collection_mutation.dodo");
    let mut loaded = dodoc::package::load(&path).unwrap();
    if let Err(error) = dodoc::sema::check(&mut loaded.program) {
        panic!("{}", loaded.render(&error));
    }
}

#[test]
fn live_views_and_allocator_dependencies_prevent_conflicting_mutations() {
    for body in [
        format!("view := values.get(0)\n{UPDATE}\ncore.drop(view)"),
        format!("view := values.as_slice()\n{UPDATE}\ncore.drop(view)"),
        format!("{UPDATE}\ncore.drop(arena)"),
        format!("{UPDATE}\nunsafe {{ arena.reset() }}"),
        format!(
            "{UPDATE}\nremoved := values.pop()\ncore.drop(values)\ncore.drop(arena)\ncore.drop(removed)"
        ),
    ] {
        rejects(
            &program(
                "text_shared.String",
                APPEND,
                &format!(
                    "values.push(text_shared.from_str(arena.handle(), \"a\", 64)?)?\nedit := Edit {{}}\n{body}"
                ),
            ),
            "borrow",
        );
    }
    for access in ["values.get_mut(0)", "values.as_mut_slice()"] {
        rejects(
            &program("text_shared.String", "", &format!("core.drop({access})")),
            "requires plain elements",
        );
    }
}

#[test]
fn map_updates_keep_view_conflicts_and_callback_restrictions() {
    let create = "map := shared_hash_map.new::<text_shared.String, text_shared.String, text_shared.Key>(arena.handle(), text_shared.Key {})\nquery := text_shared.from_str(arena.handle(), \"key\", 64)?";
    let update = "result: bool!error.AllocError = map.try_update(&query, &mut edit)\nresult?";
    for view in ["map.get(&query)", "map.entry(0)"] {
        rejects(
            &program(
                "i32",
                APPEND,
                &format!("{create}\nedit := Edit {{}}\nview := {view}\n{update}\ncore.drop(view)"),
            ),
            "borrow",
        );
    }
    rejects(
        &program(
            "i32",
            "pub struct Edit { captured: vector.Vector<&str, shared_arena.Handle>\npub fn apply(&mut self, value: &mut text_shared.String) -> void!error.AllocError stores(self, value) { return self.captured.push(value.as_str()) } }",
            &format!(
                "{create}\nedit := Edit {{ captured: shared_vector.new::<&str>(arena.handle()) }}\n{update}"
            ),
        ),
        "source",
    );
}

#[test]
fn mutation_results_are_mandatory_and_errors_must_be_plain() {
    rejects(
        &program(
            "text_shared.String",
            APPEND,
            "edit := Edit {}\nresult: bool!error.AllocError = values.try_update(0, &mut edit)",
        ),
        "Result",
    );
    for (error, returned, from) in [
        ("&str", "value.as_str()", " from(value)"),
        ("Option<&str>", "some(value.as_str())", " from(value)"),
        ("Result<i32, u8>", "ok(1)", ""),
    ] {
        rejects(
            &program(
                "text_shared.String",
                &format!(
                    "pub struct Edit {{ pub fn apply(&mut self, value: &mut text_shared.String) -> void!{error}{from} {{ return err({returned}) }} }}"
                ),
                &format!(
                    "edit := Edit {{}}\nresult: bool!{error} = values.try_update(0, &mut edit)\nmatch result {{ ok(_) => {{}}, err(_) => {{}} }}"
                ),
            ),
            "requires plain elements",
        );
    }
}

#[test]
fn scoped_edits_cannot_replace_reference_bearing_payloads() {
    for (element, declarations, replacement) in [
        ("&i32", "", "self.source"),
        ("Option<&i32>", "", "some(self.source)"),
        (
            "Borrowed",
            "pub struct Borrowed { source: &i32 }",
            "Borrowed { source: self.source }",
        ),
    ] {
        rejects(
            &program(
                element,
                &format!(
                    "{declarations}\npub struct Edit {{ source: &i32\npub fn apply(&mut self, value: &mut {element}) -> void!u8 {{ *value = {replacement}\nreturn ok() }} }}"
                ),
                "source := 1i32\nedit := Edit { source: &source }\nresult: bool!u8 = values.try_update(0, &mut edit)\nmatch result { ok(_) => {}, err(_) => {} }",
            ),
            "replacing borrow-carrying fields through a reference",
        );
    }
    rejects(
        &program(
            "text_shared.String",
            "pub struct Edit { arena: &shared_arena.SharedArena\npub fn apply(&mut self, value: &mut text_shared.String) -> void!error.AllocError { *value = text_shared.new(self.arena.handle(), 64)?\nreturn ok() } }",
            &format!("edit := Edit {{ arena: &arena }}\n{UPDATE}"),
        ),
        "replacing borrow-carrying fields through a reference",
    );
}

#[test]
fn callbacks_cannot_deposit_new_sources_or_retain_temporary_element_views() {
    rejects(
        &program(
            "vector.Vector<&i32, shared_arena.Handle>",
            "pub struct Edit { source: &i32\npub fn apply(&mut self, value: &mut vector.Vector<&i32, shared_arena.Handle>) -> void!error.AllocError stores(value, self) { return value.push(self.source) } }",
            &format!("source := 1i32\nedit := Edit {{ source: &source }}\n{UPDATE}"),
        ),
        "stored borrow source is outside",
    );
    rejects(
        &program(
            "text_shared.String",
            "pub struct Edit { captured: vector.Vector<&str, shared_arena.Handle>\npub fn apply(&mut self, value: &mut text_shared.String) -> void!error.AllocError stores(self, value) { return self.captured.push(value.as_str()) } }",
            &format!(
                "edit := Edit {{ captured: shared_vector.new::<&str>(arena.handle()) }}\n{UPDATE}"
            ),
        ),
        "source",
    );
}

#[test]
fn callback_aliases_cannot_reenter_the_collection() {
    rejects(
        &program(
            "text_shared.String",
            "pub struct Edit { owner: &mut vector.Vector<text_shared.String, shared_arena.Handle>\npub fn apply(&mut self, value: &mut text_shared.String) -> void!error.AllocError { self.owner.clear()\nreturn ok() } }",
            &format!("edit := Edit {{ owner: &mut values }}\n{UPDATE}"),
        ),
        "borrow",
    );
}
