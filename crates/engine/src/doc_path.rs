//! Paths stored in a document reach the file system only through [`resolve`] (built on
//! [`photocraft_io::doc_path::resolve`]): linked smart objects, the Displace map and Color Lookup
//! file a smart filter re-reads, data-set images and video footage. A refused path is handled like
//! a missing file: the layer keeps its last rendering, and commands that need the file say why.
//!
//! Smart-object rendering has no `Session` argument, so the path of the document being worked on
//! travels in a thread-local scope, entered where commands run ([`crate::Session::edit`], command
//! dispatch, background jobs). Outside a scope there is no document: relative paths are refused
//! and only local absolute paths are followed.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

pub use photocraft_io::doc_path::{NotFollowed, Refused};

thread_local! {
    static DOCUMENT: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Restores the previous document path when dropped.
pub(crate) struct DocumentScope(Option<PathBuf>);

impl Drop for DocumentScope {
    fn drop(&mut self) {
        let previous = self.0.take();
        DOCUMENT.with(|d| *d.borrow_mut() = previous);
    }
}

/// Resolve stored paths against `document` (the path of the document being worked on, `None`
/// for one without a file) until the returned scope is dropped.
pub(crate) fn scope(document: Option<&Path>) -> DocumentScope {
    let next = document.filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf);
    DocumentScope(DOCUMENT.with(|d| d.replace(next)))
}

/// The document path of the current scope (to carry it to another thread).
pub(crate) fn current() -> Option<PathBuf> {
    DOCUMENT.with(|d| d.borrow().clone())
}

/// The file to open for `stored`, a path stored in the document of the current scope.
pub fn resolve(stored: &str) -> Result<PathBuf, NotFollowed> {
    resolve_in(stored, current().as_deref())
}

/// The file to open for `stored`, a path stored in the document saved at `document`.
pub fn resolve_in(stored: &str, document: Option<&Path>) -> Result<PathBuf, NotFollowed> {
    photocraft_io::doc_path::resolve(stored, document)
}

/// Parameter names that carry a file or folder a played step reads or writes.
const PATH_KEYS: &[&str] =
    &["path", "paths", "file", "files", "folder", "dir", "directory", "source", "destination", "output", "input", "src", "dest", "target"];

/// Played steps (an action, a script, a droplet, a batch) carry paths that came from a file the
/// user loaded, not from the user. This refuses a step that names a file on another computer, a
/// device or a URL, so playing a loaded action never contacts a server the user did not choose
/// (Windows sends the user's login hash to the server of a `\\host\share` path). A path on the
/// share of the document being worked on (`document`) is allowed. Local paths are not touched,
/// and neither is a command the user runs directly: only playback calls this.
///
/// The check is lexical, as [`resolve_in`] is: a mapped drive letter or a link on the disk can
/// still lead to another computer.
pub fn refuse_remote_step_paths(id: &str, params: &serde_json::Value, document: Option<&Path>) -> crate::Result<()> {
    fn walk(id: &str, v: &serde_json::Value, key_is_path: bool, document: Option<&Path>, depth: usize) -> crate::Result<()> {
        if depth > 8 {
            return Ok(());
        }
        match v {
            serde_json::Value::String(s) if key_is_path && !s.is_empty() => match resolve_in(s, document) {
                Err(NotFollowed::Refused(why @ (Refused::Network | Refused::Device | Refused::Url))) => Err(crate::EngineError::Other(format!(
                    "step `{id}` was not run: its path {s} is not on this computer ({why}). Playback only follows local paths; \
                     run the step yourself if you trust the path"
                ))),
                _ => Ok(()),
            },
            serde_json::Value::Array(items) => items.iter().try_for_each(|item| walk(id, item, key_is_path, document, depth + 1)),
            serde_json::Value::Object(map) => map.iter().try_for_each(|(k, item)| walk(id, item, PATH_KEYS.contains(&k.as_str()), document, depth + 1)),
            _ => Ok(()),
        }
    }
    walk(id, params, false, document, 0)
}

/// The message for a stored path that is not followed.
pub(crate) fn refusal(what: &str, stored: &str, why: NotFollowed) -> crate::EngineError {
    crate::EngineError::Other(format!("{what} {stored} is not opened: {why}"))
}

#[cfg(test)]
mod tests;
