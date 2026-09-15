//! Read-only source documents served from this compiler's embedded library.
use crate::package;
use std::path::{Path, PathBuf};

const PREFIX: &str = "dodo-stdlib:/";

pub(super) fn path(uri: &str) -> Option<PathBuf> {
    let name = uri.strip_prefix(PREFIX)?;
    // Only the exact inventory spelling is accepted: no authorities, escapes,
    // queries, fragments, or traversal. This is not a filesystem read endpoint.
    if name
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
        || name.contains('\\')
    {
        return None;
    }
    let path = PathBuf::from("<stdlib>").join(name);
    package::bundled_source(&path).map(|_| path)
}

pub(super) fn uri(path: &Path) -> Option<String> {
    package::bundled_source(path)?;
    let name = path
        .strip_prefix("<stdlib>")
        .ok()?
        .to_str()?
        .replace('\\', "/");
    Some(format!("{PREFIX}{name}"))
}
