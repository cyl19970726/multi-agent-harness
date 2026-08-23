use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::*;

fn runner_root(runner_path: &Path) -> CliResult<PathBuf> {
    runner_path
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            CliError::Usage(format!(
                "cannot resolve DeepSeek runner package from {}",
                runner_path.display()
            ))
        })
}

pub(crate) fn embedded_reviewed_provider() -> CliResult<Value> {
    let contract: Value = serde_json::from_str(include_str!(
        "../../../apps/deepseek-member-runner/contract/runner-v1.json"
    ))?;
    contract.get("reviewedProvider").cloned().ok_or_else(|| {
        CliError::Usage(
            "DEEPSEEK_HARNESS_PROTOCOL_ERROR: embedded contract lacks reviewedProvider".into(),
        )
    })
}

pub(crate) fn verify_runner_harness_composition(runner_path: &Path) -> CliResult<()> {
    let root = runner_root(runner_path)?;
    let package = root.join("package.json");
    let raw = fs::read_to_string(&package).map_err(|error| {
        CliError::Usage(format!(
            "failed to read DeepSeek runner package {}: {error}",
            package.display()
        ))
    })?;
    let value: Value = serde_json::from_str(&raw)?;
    let observed_dependencies = value
        .get("dependencies")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            CliError::Usage(format!(
                "DeepSeek runner package {} lacks dependencies",
                package.display()
            ))
        })?;
    let reviewed = embedded_reviewed_provider()?;
    let expected_dependencies = reviewed
        .get("dependencies")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            CliError::Usage(
                "DEEPSEEK_HARNESS_PROTOCOL_ERROR: reviewedProvider lacks dependencies".into(),
            )
        })?;
    if observed_dependencies != expected_dependencies {
        return Err(CliError::Usage(format!(
            "DEEPSEEK_HARNESS_COMPOSITION_UNREVIEWED: runner package dependency set differs from the reviewed {}-package DSH composition",
            expected_dependencies.len()
        )));
    }

    for (name, expected) in expected_dependencies {
        let expected = expected.as_str().ok_or_else(|| {
            CliError::Usage(format!(
                "DEEPSEEK_HARNESS_PROTOCOL_ERROR: dependency {name} has a non-string reviewed version"
            ))
        })?;
        let installed_package = root.join("node_modules").join(name).join("package.json");
        let installed_raw = fs::read_to_string(&installed_package).map_err(|error| {
            CliError::Usage(format!(
                "DEEPSEEK_HARNESS_INSTALL_UNVERIFIED: cannot read {}: {error}",
                installed_package.display()
            ))
        })?;
        let installed: Value = serde_json::from_str(&installed_raw)?;
        let observed = installed
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("<missing>");
        if observed != expected {
            return Err(CliError::Usage(format!(
                "DEEPSEEK_HARNESS_DEPENDENCY_UNREVIEWED: {name} expected {expected}, installed {observed}"
            )));
        }
    }

    let composition = fs::read(root.join("cordis.yml")).map_err(|error| {
        CliError::Usage(format!(
            "DEEPSEEK_HARNESS_COMPOSITION_UNVERIFIED: cannot read reviewed cordis.yml: {error}"
        ))
    })?;
    let observed_fingerprint = format!("sha256:{:x}", Sha256::digest(composition));
    if observed_fingerprint != REVIEWED_DEEPSEEK_COMPOSITION_FINGERPRINT {
        return Err(CliError::Usage(format!(
            "DEEPSEEK_HARNESS_COMPOSITION_UNREVIEWED: expected {}, observed {observed_fingerprint}",
            REVIEWED_DEEPSEEK_COMPOSITION_FINGERPRINT
        )));
    }
    Ok(())
}

pub(crate) fn verify_session_bound_provider_identity(data: &Value) -> CliResult<String> {
    let required = |field: &str, code: &str| {
        data.get(field)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| CliError::Usage(format!("{code}: session_bound lacked {field}")))
    };
    let version = required("providerVersion", "DEEPSEEK_HARNESS_VERSION_UNVERIFIED")?;
    if version != REVIEWED_DEEPSEEK_HARNESS_VERSION {
        return Err(CliError::Usage(format!(
            "DEEPSEEK_HARNESS_VERSION_UNREVIEWED: expected DSH {}, observed {version}",
            REVIEWED_DEEPSEEK_HARNESS_VERSION
        )));
    }
    let source = required(
        "sourceRevision",
        "DEEPSEEK_HARNESS_SOURCE_REVISION_UNVERIFIED",
    )?;
    if source != REVIEWED_DEEPSEEK_SOURCE_REVISION {
        return Err(CliError::Usage(format!(
            "DEEPSEEK_HARNESS_SOURCE_REVISION_UNREVIEWED: expected {}, observed {source}",
            REVIEWED_DEEPSEEK_SOURCE_REVISION
        )));
    }
    let composition = required(
        "compositionFingerprint",
        "DEEPSEEK_HARNESS_COMPOSITION_UNVERIFIED",
    )?;
    if composition != REVIEWED_DEEPSEEK_COMPOSITION_FINGERPRINT {
        return Err(CliError::Usage(format!(
            "DEEPSEEK_HARNESS_COMPOSITION_UNREVIEWED: expected {}, observed {composition}",
            REVIEWED_DEEPSEEK_COMPOSITION_FINGERPRINT
        )));
    }
    Ok(version.to_string())
}
