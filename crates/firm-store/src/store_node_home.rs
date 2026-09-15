//! Where this machine's NodeDaemon lease document lives (ADR 0075).
//!
//! Machine authority is one document per `(FIRM_HOME, node_id)` under
//! `<FIRM_HOME>/nodes/<node_id>/`, not a row in any Execution Space's data
//! store. A Store therefore has to be able to *name* that directory before it
//! can answer a machine-authority question, and a Store that cannot name it
//! must fail such a question closed rather than quietly fall back to Space
//! data.
//!
//! This module carries only the binding and the naming rule. Nothing here
//! reads, writes, or locks the lease document — that mechanism lands with the
//! cutover, and until then `node_home` has no caller outside its own tests.

use super::*;

/// The named refusal for a Store that cannot say where this machine's
/// NodeDaemon lease document lives.
///
/// Fail-closed is the whole point: an unresolved node home means "I do not
/// know who owns this machine", which is never the same as "nobody owns it"
/// and never grounds for skipping a fence.
pub const MACHINE_LEASE_FILE_UNRESOLVED: &str = "MACHINE_LEASE_FILE_UNRESOLVED";

/// The two directories a Firm home registers stores under: Execution Spaces
/// (`execution_space::spaces_dir`) own coordination, and the centralized
/// per-project stores (`project::projects_dir`) are the other registered
/// layout. Both live directly under the same Firm home, so both name it.
const REGISTERED_STORE_DIRECTORIES: [&str; 2] = ["execution-spaces", "projects"];

/// The one rule that recovers a Firm home from a registered store root.
///
/// A registered store is `<FIRM_HOME>/<execution-spaces|projects>/<id>` — the
/// shapes `execution_space::space_store_root` and the project registry write,
/// and the shape the CLI already refuses to deviate from when it derives a
/// Firm home for credentials. Recovering the home from the root is a *path*
/// fact, not an authority decision: the authority still lives in exactly one
/// file, and this only says which directory to look in.
///
/// A root of any other shape returns `None`, which leaves the Store unbound so
/// that every machine-authority read fails closed with
/// [`MACHINE_LEASE_FILE_UNRESOLVED`]. Callers that know their Firm home out of
/// band — a Fabric collaboration root, an import, a fixture — bind it
/// explicitly with [`HarnessStore::with_firm_home`] instead.
pub fn firm_home_of_registered_store_root(store_root: &Path) -> Option<PathBuf> {
    let registered_dir = store_root.parent()?;
    let name = registered_dir.file_name()?;
    if !REGISTERED_STORE_DIRECTORIES
        .iter()
        .any(|directory| name == *directory)
    {
        return None;
    }
    registered_dir.parent().map(Path::to_path_buf)
}

/// A node id names one directory under `<FIRM_HOME>/nodes/`, so it must be one
/// ordinary path segment. Refusing separators and the relative entries here
/// keeps a foreign or malformed id from naming a directory outside the node
/// tree; it can never become a traversal.
fn require_node_directory_segment(node_id: &str) -> StoreResult<()> {
    let refuse = |reason: &str| {
        Err(StoreError::Conflict(format!(
            "{MACHINE_LEASE_FILE_UNRESOLVED}: Node id {node_id:?} {reason}, so it cannot name a directory under a Firm home"
        )))
    };
    if node_id.is_empty() {
        return refuse("is empty");
    }
    if node_id == "." || node_id == ".." {
        return refuse("is a relative directory entry");
    }
    if node_id.contains('/') || node_id.contains('\\') || node_id.contains('\0') {
        return refuse("contains a path separator");
    }
    Ok(())
}

impl HarnessStore {
    /// Bind the Firm home whose `nodes/<node_id>/` directory holds this
    /// machine's NodeDaemon lease document.
    ///
    /// An explicit binding always wins over the shape-derived one, so a caller
    /// that knows its Firm home never depends on its store root's layout.
    pub fn with_firm_home(mut self, firm_home: impl Into<PathBuf>) -> Self {
        self.firm_home = Some(firm_home.into());
        self
    }

    /// The Firm home this Store is bound to, if any.
    pub fn firm_home(&self) -> Option<&Path> {
        self.firm_home.as_deref()
    }

    /// `<FIRM_HOME>/nodes/<node_id>` — the machine rendezvous that already
    /// holds `daemon.sock` and `node-daemon.log`, and which from ADR 0075 also
    /// holds the machine lease document, its history and its own lock.
    ///
    /// Returns the [`MACHINE_LEASE_FILE_UNRESOLVED`] refusal when this Store
    /// is unbound, so a machine-authority caller fails closed by construction
    /// instead of having to remember to check.
    pub fn node_home(&self, node_id: &str) -> StoreResult<PathBuf> {
        require_node_directory_segment(node_id)?;
        let firm_home = self.firm_home.as_ref().ok_or_else(|| {
            StoreError::Conflict(format!(
                "{MACHINE_LEASE_FILE_UNRESOLVED}: Store {} is bound to no Firm home, so the machine lease document for Node {node_id} cannot be named",
                self.root.display()
            ))
        })?;
        Ok(firm_home.join("nodes").join(node_id))
    }
}
