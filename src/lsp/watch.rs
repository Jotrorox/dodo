//! Client-side watches include new/missing imports and package siblings. Watching
//! source directories also covers dependencies outside the workspace via symlinks.
use crate::json::{Value, json};
use crate::{editor, file_uri};
use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Default)]
pub(super) struct Watches {
    pub supported: bool,
    pub relative: bool,
    pub initialized: bool,
    pub manifest: Option<PathBuf>,
    roots: BTreeSet<PathBuf>,
    registered: BTreeSet<PathBuf>,
    registration: Option<String>,
    pending: BTreeSet<String>,
    next_id: u64,
}

impl Watches {
    pub fn contains(&self, path: &Path) -> bool {
        self.manifest
            .as_deref()
            .is_some_and(|manifest| crate::package::source_path(manifest) == path)
            || path.extension().is_some_and(|ext| ext == "dodo")
                && self.roots.iter().any(|root| path.starts_with(root))
    }

    pub fn sync(&mut self, roots: BTreeSet<PathBuf>, output: &mut impl Write) -> io::Result<()> {
        // A parent watch includes all nested source directories.
        self.roots = roots
            .iter()
            .filter(|root| {
                !roots
                    .iter()
                    .any(|parent| parent != *root && root.starts_with(parent))
            })
            .cloned()
            .collect();
        if !self.supported || !self.initialized || self.roots == self.registered {
            return Ok(());
        }
        if let Some(id) = self.registration.take() {
            // The misspelling is part of the LSP wire format.
            self.request(
                output,
                "client/unregisterCapability",
                json!({
                    "unregisterations": [{"id": id, "method": "workspace/didChangeWatchedFiles"}]
                }),
            )?;
        }
        let mut watchers: Vec<_> = self
            .roots
            .iter()
            .filter_map(|root| {
                let pattern = if self.relative {
                    json!({"baseUri": file_uri::from_path(root).ok()?, "pattern": "**/*.dodo"})
                } else {
                    // URI conversion removes Windows' canonical \\?\ prefix.
                    let uri = file_uri::from_path(root).ok()?;
                    let path = file_uri::to_path(&uri).ok()?;
                    let path = path.to_str()?.replace('\\', "/");
                    // Glob metacharacters in directory names must stay literal.
                    let mut pattern = String::new();
                    for c in path.chars() {
                        if "*?[]{}".contains(c) {
                            pattern.push('[');
                            pattern.push(c);
                            pattern.push(']');
                        } else {
                            pattern.push(c);
                        }
                    }
                    json!(format!("{}/**/*.dodo", pattern.trim_end_matches('/')))
                };
                Some(json!({"globPattern": pattern, "kind": 7}))
            })
            .collect();
        if let Some(path) = &self.manifest
            && let Ok(uri) = file_uri::from_path(path)
            && let Ok(path) = file_uri::to_path(&uri)
            && let Some(path) = path.to_str()
        {
            let mut pattern = String::new();
            for c in path.replace('\\', "/").chars() {
                if "*?[]{}".contains(c) {
                    pattern.push('[');
                    pattern.push(c);
                    pattern.push(']');
                } else {
                    pattern.push(c);
                }
            }
            watchers.push(json!({"globPattern":pattern,"kind":7}));
        }
        if !watchers.is_empty() {
            let id = format!("dodo/watchedFiles/{}", self.next_id);
            self.request(
                output,
                "client/registerCapability",
                json!({
                    "registrations": [{"id": id, "method": "workspace/didChangeWatchedFiles",
                        "registerOptions": {"watchers": watchers}}]
                }),
            )?;
            self.registration = Some(id);
        }
        self.registered = self.roots.clone();
        Ok(())
    }

    fn request(&mut self, output: &mut impl Write, method: &str, params: Value) -> io::Result<()> {
        let id = format!("dodo/watch/{}", self.next_id);
        self.next_id += 1;
        self.pending.insert(id.clone());
        editor::write_message(
            output,
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        )
    }

    pub fn response(&mut self, id: &Value, error: Option<&str>) {
        if let Some(id) = id.as_str()
            && self.pending.remove(id)
            && let Some(error) = error
        {
            self.supported = false;
            eprintln!(
                "LSP: client rejected file watches: {error}; save a document to refresh dependencies"
            );
        }
    }
}
