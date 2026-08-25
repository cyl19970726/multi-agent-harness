use std::{
    io::Read,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::{CliError, CliResult};

const MAX_READER_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

pub(super) fn read_official_jsonl(session_id: &str) -> CliResult<String> {
    if session_id.trim().is_empty() || session_id.contains('/') || session_id.contains('\\') {
        return Err(CliError::Usage(
            "invalid DeepSeek Harness Session id".into(),
        ));
    }
    let cwd = std::env::current_dir()?;
    let runner = crate::deepseek_harness_runner_path(&cwd)?;
    let reader = runner
        .parent()
        .ok_or_else(|| CliError::Usage("DeepSeek runner has no package bin directory".into()))?
        .join("deepseek-session-reader.mjs");
    if !reader.is_file() {
        return Err(CliError::Usage(format!(
            "DeepSeek Session reader is unavailable at {}",
            reader.display()
        )));
    }
    let mut child = Command::new("node")
        .arg(reader)
        .arg(session_id)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let output = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take((MAX_READER_OUTPUT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let error_output = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr
            .take(16 * 1024)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CliError::Usage(
                "DeepSeek Session reader exceeded its 3 second bound".into(),
            ));
        }
        thread::sleep(Duration::from_millis(10));
    };
    let bytes = output
        .join()
        .map_err(|_| CliError::Usage("DeepSeek Session reader stdout panicked".into()))??;
    let errors = error_output
        .join()
        .map_err(|_| CliError::Usage("DeepSeek Session reader stderr panicked".into()))??;
    if !status.success() {
        return Err(CliError::Usage(format!(
            "DeepSeek Session reader failed: {}",
            String::from_utf8_lossy(&errors).trim()
        )));
    }
    if bytes.len() > MAX_READER_OUTPUT_BYTES {
        return Err(CliError::Usage(
            "DeepSeek Session reader exceeded its bounded response".into(),
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| CliError::Usage("DeepSeek Session reader returned non-UTF-8 JSONL".into()))
}
