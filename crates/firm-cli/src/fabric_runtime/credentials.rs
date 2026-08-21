use super::*;


pub(super) enum ResolvedNodeTls {
    Files(NodeTlsIdentityFiles),
    Material(NodeTlsIdentityMaterial),
}

pub(super) struct ResolvedNodeCredentials {
    pub(super) tls: ResolvedNodeTls,
    pub(super) certificate_serial: String,
    pub(super) public_key_fingerprint: String,
}

pub(super) fn resolve_node_credentials(
    args: &[String],
    company_id: &str,
    node_id: &str,
) -> CliResult<ResolvedNodeCredentials> {
    match value(args, "--credential-backend")
        .unwrap_or_else(|| "file".into())
        .as_str()
    {
        "file" => Ok(ResolvedNodeCredentials {
            tls: ResolvedNodeTls::Files(NodeTlsIdentityFiles {
                client_certificate_chain_pem: required_path(args, "--client-cert")?,
                client_private_key_pem: required_path(args, "--client-key")?,
                control_plane_ca_pem: required_path(args, "--control-plane-ca")?,
            }),
            certificate_serial: required(args, "--certificate-serial")?,
            public_key_fingerprint: required(args, "--public-key-fingerprint")?,
        }),
        "macos-keychain" => {
            let service = required(args, "--keychain-service")?;
            if service.trim().is_empty() || service.len() > 128 {
                return Err(CliError::Usage(
                    "--keychain-service must be a bounded non-empty service name".into(),
                ));
            }
            // Validate the public enrolled identity before touching Keychain.
            // Besides failing closed, this prevents avoidable ACL prompts when
            // an incomplete login-agent command is installed.
            let certificate_serial = required(args, "--certificate-serial")?;
            let public_key_fingerprint = required(args, "--public-key-fingerprint")?;
            let prefix = format!("{company_id}:{node_id}");
            Ok(ResolvedNodeCredentials {
                tls: ResolvedNodeTls::Material(NodeTlsIdentityMaterial {
                    client_certificate_chain_pem: keychain_secret(
                        &service,
                        &format!("{prefix}:client-certificate"),
                    )?
                    .into_bytes(),
                    client_private_key_pem: keychain_secret(
                        &service,
                        &format!("{prefix}:client-private-key"),
                    )?
                    .into_bytes(),
                    control_plane_ca_pem: keychain_secret(
                        &service,
                        &format!("{prefix}:control-plane-ca"),
                    )?
                    .into_bytes(),
                }),
                // Serial and fingerprint are public certificate identity, not
                // secret key material. Requiring five separate Keychain ACL
                // prompts made a login LaunchAgent repeatedly block on user
                // interaction. Keep only the three PEM materials in Keychain;
                // the explicit public values are still checked by the
                // generation-fenced mTLS welcome against the enrolled Node.
                certificate_serial,
                public_key_fingerprint,
            })
        }
        _ => Err(CliError::Usage(
            "--credential-backend must be file|macos-keychain".into(),
        )),
    }
}

#[cfg(target_os = "macos")]
pub(super) fn keychain_secret(service: &str, account: &str) -> CliResult<String> {
    let output = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-w", "-s", service, "-a", account])
        .output()?;
    if !output.status.success() {
        return Err(CliError::Usage(format!(
            "macOS Keychain item is unavailable for service={service} account={account}"
        )));
    }
    let secret = String::from_utf8(output.stdout)
        .map_err(|_| CliError::Usage("macOS Keychain item is not valid UTF-8".into()))?;
    let secret = secret.trim().to_string();
    if secret.is_empty() {
        return Err(CliError::Usage(format!(
            "macOS Keychain item is empty for service={service} account={account}"
        )));
    }
    Ok(secret)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn keychain_secret(_service: &str, _account: &str) -> CliResult<String> {
    Err(CliError::Usage(
        "macos-keychain credential backend is available only on macOS".into(),
    ))
}

pub(crate) fn firm_home(resolved: &ResolvedStore, args: &[String]) -> CliResult<PathBuf> {
    if let Some(path) = value(args, "--firm-home") {
        return Ok(PathBuf::from(path));
    }
    if let Some(space) = &resolved.execution_space_context {
        return firm_home_from_execution_space_root(&space.store_root);
    }
    crate::execution_space::firm_home().map_err(|error| CliError::Usage(error.to_string()))
}

pub(super) fn firm_home_from_execution_space_root(store_root: &Path) -> CliResult<PathBuf> {
    let execution_spaces = store_root
        .parent()
        .ok_or_else(|| CliError::Usage("cannot derive FIRM_HOME from Execution Space".into()))?;
    if execution_spaces.file_name().and_then(|name| name.to_str()) != Some("execution-spaces") {
        return Err(CliError::Usage(
            "Execution Space store must be a direct child of FIRM_HOME/execution-spaces".into(),
        ));
    }
    execution_spaces
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| CliError::Usage("cannot derive FIRM_HOME from Execution Space".into()))
}

pub(super) fn required(args: &[String], name: &str) -> CliResult<String> {
    value(args, name).ok_or_else(|| CliError::Usage(format!("missing required {name}")))
}

pub(super) fn required_path(args: &[String], name: &str) -> CliResult<PathBuf> {
    Ok(PathBuf::from(required(args, name)?))
}

pub(super) fn value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

pub(super) fn required_key_file(args: &[String], name: &str) -> CliResult<[u8; 32]> {
    let raw = required_secret_file(args, name)?;
    let raw = raw.trim();
    if raw.len() != 64 || !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CliError::Usage(format!(
            "{name} must contain exactly 64 hexadecimal characters"
        )));
    }
    let mut key = [0u8; 32];
    for (index, output) in key.iter_mut().enumerate() {
        *output = u8::from_str_radix(&raw[index * 2..index * 2 + 2], 16)
            .map_err(|_| CliError::Usage(format!("{name} is invalid")))?;
    }
    Ok(key)
}

pub(super) fn required_secret_file(args: &[String], name: &str) -> CliResult<String> {
    use std::os::unix::fs::PermissionsExt;
    let path = required_path(args, name)?;
    let metadata = std::fs::symlink_metadata(&path)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(CliError::Usage(format!(
            "{name} must be a regular non-symlink file with no group/other permissions"
        )));
    }
    Ok(std::fs::read_to_string(path)?.trim().to_string())
}

pub(super) fn fabric_error(error: FabricError) -> CliError {
    CliError::Usage(format!("REMOTE_FABRIC_{:?}: {}", error.code, error.message))
}
