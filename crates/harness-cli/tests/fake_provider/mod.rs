//! Shared test helper: a FAKE provider binary (`codex` / `claude`) that records
//! the cwd it was spawned in, so persistent-delivery cwd tests can prove the
//! harness spawns the worker in the SELECTED project's `project_root` — not the
//! harness process cwd — without invoking a real provider (goal-multi-project P3,
//! Stage 3).
//!
//! The harness spawns providers by BARE NAME (`Command::new("codex")` /
//! `Command::new("claude")`), so prepending a dir holding an executable shim to
//! `PATH` intercepts the spawn. The shim writes its `$PWD` to a known file and
//! emits one harmless NDJSON line so `run_ndjson_child` has something to read,
//! then exits. We assert on the recorded cwd, not on delivery success.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Create a `bin/` dir containing an executable shim named `provider` (e.g.
/// `codex` or `claude`) that, when run, writes its current working directory to
/// `cwd_marker` and emits a single NDJSON line on stdout. Returns the `bin/` dir
/// to prepend to `PATH`.
///
/// `which <provider>` (used by the harness when starting the runtime) also
/// resolves to this shim, so `--start-runtime` reports the provider as available.
pub fn install_provider_shim(base: &Path, provider: &str, cwd_marker: &Path) -> PathBuf {
    install_provider_shim_capturing(base, provider, cwd_marker, None)
}

/// Like [`install_provider_shim`] but, when `capture_file` is `Some((name, dst))`,
/// the shim also copies the content of `<cwd>/<name>` (if present) to `dst`. This
/// proves a provider can READ a project-root file (e.g. `CLAUDE.md`) from the cwd
/// the harness spawned it in — the whole point of cwd routing.
pub fn install_provider_shim_capturing(
    base: &Path,
    provider: &str,
    cwd_marker: &Path,
    capture_file: Option<(&str, &Path)>,
) -> PathBuf {
    let bin_dir = base.join(format!("fakebin-{provider}"));
    fs::create_dir_all(&bin_dir).expect("mk fake bin dir");
    let shim_path = bin_dir.join(provider);
    // POSIX shell shim. `pwd -P` resolves symlinks so the recorded path matches a
    // canonicalized project root. The NDJSON line keeps the reader happy; its
    // content is irrelevant to the cwd assertion.
    let mut script = String::from("#!/bin/sh\n");
    script.push_str(&format!(
        "pwd -P > {marker}\n",
        marker = shell_single_quote(&cwd_marker.display().to_string()),
    ));
    if let Some((name, dst)) = capture_file {
        // Copy the named file from the cwd to `dst` iff it exists (cat is run
        // relative to the shim's cwd — the project root the harness chose).
        script.push_str(&format!(
            "if [ -f {name} ]; then cat {name} > {dst}; fi\n",
            name = shell_single_quote(name),
            dst = shell_single_quote(&dst.display().to_string()),
        ));
    }
    script.push_str("printf '%s\\n' '{\"type\":\"fake\"}'\nexit 0\n");
    fs::write(&shim_path, script).expect("write shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&shim_path).expect("stat shim").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&shim_path, perms).expect("chmod shim");
    }
    bin_dir
}

/// Read the cwd a shim recorded, trimmed. Panics if the marker was never written
/// (the provider shim never ran), which itself is a useful failure signal.
pub fn read_recorded_cwd(cwd_marker: &Path) -> PathBuf {
    let raw = fs::read_to_string(cwd_marker)
        .unwrap_or_else(|e| panic!("provider shim never recorded a cwd at {cwd_marker:?}: {e}"));
    let trimmed = raw.trim();
    assert!(!trimmed.is_empty(), "recorded cwd was empty");
    PathBuf::from(trimmed)
}

/// Single-quote a string for safe inclusion in a POSIX shell script.
fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Create a `bin/` dir holding a fake `kimi` executable speaking just enough
/// line-delimited ACP JSON-RPC (stdio) for `team-run start` integration tests.
///
/// The shim answers `initialize` / `session/new` with canned results, and for
/// `session/prompt` streams (in order): one `agent_thought_chunk` (eligible
/// only for the volatile live preview; never journaled), one `tool_call` + terminal
/// `tool_call_update`, one `agent_message_chunk` carrying a `## RESULT` /
/// `## SUMMARY` report, then the terminal `{"result":{"stopReason":...}}`
/// response. `FAKE_KIMI_RESULT` (done|blocked|failed, default done) selects
/// the RESULT word so tests can drive both run outcomes. Prepend the returned
/// dir to PATH so [`resolve_kimi_bin`] picks the shim over a real install.
pub fn install_kimi_acp_shim(base: &Path) -> PathBuf {
    let bin_dir = base.join("fakebin-kimi");
    fs::create_dir_all(&bin_dir).expect("mk fake kimi bin dir");
    let shim_path = bin_dir.join("kimi");
    // printf format strings: `\\n` emits a literal backslash-n (a JSON escape
    // inside string values); a trailing `\n` emits the record newline.
    let script = r###"#!/bin/sh
# Fake `kimi acp` (Agent Team v0 tests): line-delimited JSON-RPC over stdio.
result="${FAKE_KIMI_RESULT:-done}"
if [ "$1" != "acp" ]; then
  echo "fake kimi: only 'acp' is implemented" >&2
  exit 2
fi
session_id="session_fake_$$"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1,"agentCapabilities":{},"authMethods":[],"agentInfo":{"name":"fake-kimi","version":"0.0.0"}}}\n' "$id"
      ;;
    *'"method":"session/new"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"%s","configOptions":[]}}\n' "$id" "$session_id"
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"available_commands_update","availableCommands":[]}}}\n' "$session_id"
      ;;
    *'"method":"session/prompt"'*)
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"hidden reasoning"}}}}\n' "$session_id"
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call","toolCallId":"tool-1","title":"fake_edit","kind":"edit","status":"in_progress"}}}\n' "$session_id"
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call_update","toolCallId":"tool-1","status":"completed"}}}\n' "$session_id"
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"## RESULT\\n%s\\n## SUMMARY\\nfake member finished round\\n"}}}}\n' "$session_id" "$result"
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
      ;;
    *'"method":"session/cancel"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id"
      ;;
  esac
done
exit 0
"###;
    fs::write(&shim_path, script).expect("write fake kimi shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&shim_path).expect("stat shim").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&shim_path, perms).expect("chmod shim");
    }
    bin_dir
}

/// One spawned `harness` invocation's result.
pub struct CliOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

/// Drive a full persistent delivery through the real `harness` binary against a
/// SELECTED project, with a fake provider on `PATH`. Every step runs from
/// `process_cwd` (deliberately != the project root) to prove the worker cwd
/// derives from the selected project, not the harness process cwd.
///
/// `envs` are the base env pairs (HOME / HARNESS_HOME from `TempHome::envs()`);
/// `fake_bin` is prepended to PATH so the provider shim intercepts the spawn.
pub struct DeliveryDriver {
    bin: PathBuf,
    project_root: PathBuf,
    process_cwd: PathBuf,
    envs: Vec<(String, String)>,
    fake_bin: PathBuf,
}

impl DeliveryDriver {
    pub fn new(
        project_root: &Path,
        process_cwd: &Path,
        envs: Vec<(String, String)>,
        fake_bin: &Path,
    ) -> Self {
        Self {
            bin: PathBuf::from(env!("CARGO_BIN_EXE_harness")),
            project_root: project_root.to_path_buf(),
            process_cwd: process_cwd.to_path_buf(),
            envs,
            fake_bin: fake_bin.to_path_buf(),
        }
    }

    fn run(&self, args: &[&str]) -> CliOutput {
        let path = format!(
            "{}:{}",
            self.fake_bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let out = Command::new(&self.bin)
            .arg("--project")
            .arg(&self.project_root)
            .args(args)
            .current_dir(&self.process_cwd)
            .envs(self.envs.iter().cloned())
            .env("PATH", path)
            .env_remove("HARNESS_ROOT")
            .output()
            .expect("run harness");
        CliOutput {
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            success: out.status.success(),
        }
    }

    /// `harness --project <root> init` (registers + activates the project).
    pub fn init_project(&self) {
        let out = self.run(&["init"]);
        assert!(out.success, "init failed: {}", out.stderr);
    }

    /// Create a member for `provider`. When `worktree` is `Some`, pins the member's
    /// workspace via `--worktree`. Returns the new member id.
    pub fn create_member(&self, provider: &str, worktree: Option<&Path>) -> String {
        let mut args = vec![
            "agent",
            "create",
            "--name",
            "worker",
            "--role",
            "worker",
            "--provider",
            provider,
        ];
        let worktree_str;
        if let Some(wt) = worktree {
            worktree_str = wt.display().to_string();
            args.push("--worktree");
            args.push(&worktree_str);
        }
        let out = self.run(&args);
        assert!(out.success, "agent create failed: {}", out.stderr);
        let value: serde_json::Value = serde_json::from_str(&out.stdout)
            .unwrap_or_else(|e| panic!("create stdout not JSON ({e}): {}", out.stdout));
        value["id"]
            .as_str()
            .expect("member id in create output")
            .to_string()
    }

    /// Queue a message for `member_id`.
    pub fn send_message(&self, member_id: &str, content: &str) {
        let out = self.run(&[
            "agent",
            "send",
            "--to",
            member_id,
            "--from",
            "lead",
            "--content",
            content,
        ]);
        assert!(out.success, "agent send failed: {}", out.stderr);
    }

    /// Deliver queued messages to `member_id`, starting the runtime. Returns the
    /// delivery output (delivery may report failure since the shim is not a real
    /// provider; the cwd is recorded regardless).
    pub fn deliver(&self, member_id: &str) -> CliOutput {
        self.run(&[
            "agent",
            "deliver",
            "--agent",
            member_id,
            "--start-runtime",
            "--timeout-ms",
            "5000",
        ])
    }
}
