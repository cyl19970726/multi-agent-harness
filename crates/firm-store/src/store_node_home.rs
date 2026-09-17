//! Where this machine's NodeDaemon lease document lives (ADR 0075).
//!
//! Machine authority is one document per `(FIRM_HOME, node_id)` under
//! `<FIRM_HOME>/nodes/<node_id>/`, not a row in any Execution Space's data
//! store. A Store therefore has to be able to *name* that directory before it
//! can answer a machine-authority question, and a Store that cannot name it
//! must fail such a question closed rather than quietly fall back to Space
//! data.
//!
//! Two properties make the name trustworthy, and both are refusals rather than
//! repairs:
//!
//! - **Absolute.** A relative home names a different directory from every
//!   process with a different working directory — a lease per cwd, not a lease
//!   per machine.
//! - **Canonical.** One directory reached through two spellings (macOS exposes
//!   `/var` through `/private/var`) would otherwise be two documents under two
//!   flocks: two simultaneous machine authorities, which is the exact failure
//!   ADR 0075 removes, reintroduced through the path layer instead of the lock
//!   layer. `node_daemon_socket_path` already derives the *sibling*
//!   machine-authority path in this same directory from one canonical
//!   filesystem identity "instead of the caller's raw spelling"; the lease has
//!   the same scope, so it needs the same rule.
//!
//! This module carries only the binding and the naming rule. Nothing here
//! reads, writes, or locks the lease document — that mechanism lands with the
//! cutover, and until then `node_home` has no caller outside its own tests.

use super::*;

use firm_core::agentfirm_api::{TrustError, TrustErrorCode};

/// Build the machine-lease refusal as a typed `TrustError`, not a message
/// prefix.
///
/// The consumers are ADR 0075's 46 machine-authority deciders. A string that 46
/// call sites must match identically is a rule enforced by 46 copies of a
/// habit; a variant is enforced by the compiler. The display text keeps the
/// `MACHINE_LEASE_FILE_UNRESOLVED` token so operator-facing output and the
/// existing predicate stay readable.
pub(crate) fn machine_lease_unresolved(node_id: &str, detail: String) -> StoreError {
    StoreError::Conflict(
        serde_json::to_string(&TrustError {
            code: TrustErrorCode::MachineLeaseUnresolved,
            message: format!("{MACHINE_LEASE_FILE_UNRESOLVED}: {detail}"),
            retryable: false,
            resource_kind: "node_daemon_lease".to_string(),
            resource_id: node_id.to_string(),
            current_version: None,
        })
        .unwrap_or_else(|_| format!("{MACHINE_LEASE_FILE_UNRESOLVED}: {detail}")),
    )
}

/// The named refusal for a Store that cannot say where this machine's
/// NodeDaemon lease document lives.
///
/// Fail-closed is the whole point: an unresolved node home means "I do not
/// know who owns this machine", which is never the same as "nobody owns it"
/// and never grounds for skipping a fence. A *meaningless* home is worse than
/// no home at all, because it passes the fence silently — so a home that is
/// relative, or that cannot be resolved to one canonical directory, is refused
/// here rather than used.
pub const MACHINE_LEASE_FILE_UNRESOLVED: &str = "MACHINE_LEASE_FILE_UNRESOLVED";

/// The directory a Firm home registers Execution Space coordination stores
/// under (`execution_space::spaces_dir`).
const EXECUTION_SPACES_DIRECTORY: &str = "execution-spaces";

/// Recover a Firm home from an Execution Space store root — the fixture and
/// open-by-path affordance, not production's path.
///
/// A Space store is `<FIRM_HOME>/execution-spaces/<space_id>`: the shape
/// `execution_space::space_store_root` writes, and the shape the CLI already
/// refuses to deviate from when it derives a Firm home for credentials.
///
/// Production binds its home explicitly with [`HarnessStore::with_firm_home`],
/// for the reason stated a few lines above `with_provider_compatibility_scope`
/// in `store_store_base.rs`: operational authority "is deliberately explicit
/// and is never inferred from a path". This derivation exists so a fixture
/// rooted in production's layout works without a declaration, and so a store
/// opened by path alone still fails closed rather than silently.
///
/// Only `execution-spaces` is accepted. The centralized project store
/// directory is deliberately **not**: `projects` is one of the most common
/// directory names on a developer machine, so a lexical rule accepting it would
/// read `/Users/x/dev/projects/myrepo` as the Firm home `/Users/x/dev` and put
/// a machine lease document somewhere nobody else looks. A project store that
/// needs a home binds one.
///
/// Returns `None` for any other shape, and for any home that is not absolute,
/// which leaves the Store unbound so every machine-authority read fails closed
/// with [`MACHINE_LEASE_FILE_UNRESOLVED`]. This function performs no I/O, so
/// `HarnessStore::new` stays I/O-free; canonicalization happens in
/// [`HarnessStore::node_home`], where touching the filesystem is allowed.
pub fn firm_home_of_execution_space_root(store_root: &Path) -> Option<PathBuf> {
    let spaces_dir = store_root.parent()?;
    if spaces_dir.file_name()? != EXECUTION_SPACES_DIRECTORY {
        return None;
    }
    let firm_home = spaces_dir.parent()?;
    // `Path::parent` of a relative `execution-spaces/<id>` is the *empty* path,
    // which is `Some(_)` and would otherwise bind the Store to a home of "",
    // naming the cwd-relative `nodes/<id>`. Requiring absoluteness is what
    // turns that fail-open into the refusal above.
    if !firm_home.is_absolute() {
        return None;
    }
    Some(firm_home.to_path_buf())
}

/// Resolve a Firm home to the one canonical directory it names.
///
/// Best-effort in the same sense as the daemon socket's rule — a home that does
/// not exist yet still resolves the aliases on the part that does — but
/// deliberately **without** that helper's cwd-join fallback, which would paper
/// over a relative home instead of refusing it.
///
/// Public so the CLI's own home derivation applies the same rule instead of
/// growing a second, weaker copy of it (#993): one canonical identity per Firm
/// home, in both directions.
pub fn canonical_firm_home(firm_home: &Path) -> Option<PathBuf> {
    if !firm_home.is_absolute() {
        return None;
    }
    if let Ok(canonical) = fs::canonicalize(firm_home) {
        return Some(canonical);
    }
    // Canonicalize the deepest ancestor that does exist and re-attach the rest,
    // so a home about to be created still gets one identity rather than two.
    let mut suffix = Vec::new();
    let mut cursor = firm_home;
    while let Some(parent) = cursor.parent() {
        suffix.push(cursor.file_name()?.to_os_string());
        if let Ok(canonical) = fs::canonicalize(parent) {
            let mut resolved = canonical;
            for component in suffix.iter().rev() {
                resolved.push(component);
            }
            return Some(resolved);
        }
        cursor = parent;
    }
    Some(firm_home.to_path_buf())
}

/// A node id names one directory under `<FIRM_HOME>/nodes/`, so it must be one
/// safe canonical path component.
///
/// This reuses the crate's existing allowlist for that very same
/// `<FIRM_HOME>/nodes/<node_id>` segment rather than standing a second, laxer
/// rule beside it: a denylist of separators and relative entries stops
/// traversal but still admits ids the Remote Fabric store already refuses, and
/// the module claiming the stricter threat model should not be the weaker of
/// the two.
fn require_node_directory_segment(node_id: &str) -> StoreResult<()> {
    if !crate::remote_fabric_store::is_safe_path_component(node_id) {
        return Err(machine_lease_unresolved(
            node_id,
            format!(
                "Node id {node_id:?} is not a safe canonical path component, so it cannot name a directory under a Firm home"
            ),
        ));
    }
    Ok(())
}

/// The machine lease document, read Store-free — the shape `daemon status`
/// needs (ADR 0075, #671).
///
/// Status answers on the one control lane that must stay answerable while an
/// Execution Space scan is busy, so it reads only the node file and never
/// opens a Space store. That is also why there is deliberately no legacy-row
/// fallback here: the fallback needs a Store. A node with no document is
/// `Ok(None)` — the one honest "nobody" — and the caller reports it as
/// absent.
///
/// Returns the canonical document path alongside the lease, so status can
/// name the file an operator can open.
pub fn machine_lease_document_at(
    firm_home: &Path,
    node_id: &str,
) -> StoreResult<(PathBuf, Option<NodeDaemonLease>)> {
    require_node_directory_segment(node_id)?;
    let canonical = canonical_firm_home(firm_home).ok_or_else(|| {
        machine_lease_unresolved(
            node_id,
            format!(
                "Firm home {} is not an absolute, resolvable directory, so the machine lease document for Node {node_id} cannot be named",
                firm_home.display()
            ),
        )
    })?;
    let node_home = canonical.join("nodes").join(node_id);
    let path = crate::node_lease_document::lease_document_path(&node_home);
    let document = crate::node_lease_document::read_lease_document(&node_home, node_id)?;
    Ok((path, document.map(|document| document.lease)))
}

impl HarnessStore {
    /// Bind the Firm home whose `nodes/<node_id>/` directory holds this
    /// machine's NodeDaemon lease document.
    ///
    /// This is production's path: a caller that knows its Firm home never
    /// depends on its store root's layout, and an explicit binding always wins
    /// over the shape-derived one. The value is validated where it is *used*
    /// (see [`HarnessStore::node_home`]) rather than dropped here, so a
    /// relative or unresolvable home surfaces as the named refusal instead of
    /// disappearing into a silently unbound Store.
    pub fn with_firm_home(mut self, firm_home: impl Into<PathBuf>) -> Self {
        self.firm_home = Some(firm_home.into());
        self
    }

    /// The Firm home this Store is bound to, exactly as bound — before the
    /// absoluteness and canonicalization rules that
    /// [`HarnessStore::node_home`] applies.
    pub fn firm_home(&self) -> Option<&Path> {
        self.firm_home.as_deref()
    }

    /// `<FIRM_HOME>/nodes/<node_id>` — the machine rendezvous that also holds
    /// `node-daemon.log`, and which from ADR 0075 holds the machine lease
    /// document, its generation history and its own lock.
    ///
    /// `daemon.sock` normally lives here too, but not always: it falls back to
    /// a hashed path under `/tmp` when this path would exceed the macOS AF_UNIX
    /// 104-byte limit. The lease document has no such fallback, so under a long
    /// Firm home the socket and the lease sit in different directories.
    ///
    /// Returns the [`MACHINE_LEASE_FILE_UNRESOLVED`] refusal when this Store is
    /// unbound, when its home is relative, or when that home cannot be resolved
    /// to one canonical directory — so a machine-authority caller fails closed
    /// by construction instead of having to remember to check.
    pub fn node_home(&self, node_id: &str) -> StoreResult<PathBuf> {
        require_node_directory_segment(node_id)?;
        let firm_home = self.firm_home.as_ref().ok_or_else(|| {
            machine_lease_unresolved(
                node_id,
                format!(
                    "Store {} is bound to no Firm home, so the machine lease document for Node {node_id} cannot be named",
                    self.root.display()
                ),
            )
        })?;
        let canonical = canonical_firm_home(firm_home).ok_or_else(|| {
            machine_lease_unresolved(
                node_id,
                format!(
                    "Firm home {} is not an absolute, resolvable directory, so the machine lease document for Node {node_id} would name a different file from every other process",
                    firm_home.display()
                ),
            )
        })?;
        Ok(canonical.join("nodes").join(node_id))
    }
}

impl StoreError {
    /// Did this error refuse because the machine lease document could not be
    /// named?
    ///
    /// Machine-authority callers must be able to recognise this refusal without
    /// matching the display text, in the same spirit as `trust_error()`, which
    /// exists so policy callers never classify a message by its words. The
    /// typed `TrustErrorCode::MachineLeaseUnresolved` variant is what the
    /// writers emit; the prefix fallback covers pre-typed refusals still
    /// carrying the bare token.
    pub fn is_machine_lease_unresolved(&self) -> bool {
        match self.trust_error() {
            Some(error) => error.code == TrustErrorCode::MachineLeaseUnresolved,
            // Pre-typed refusals and the non-trust paths still carry the token.
            None => matches!(self, Self::Conflict(message)
                if message.starts_with(MACHINE_LEASE_FILE_UNRESOLVED)),
        }
    }
}
