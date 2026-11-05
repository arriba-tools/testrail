//! The API key in whichever secret store the platform has.

#[cfg(any(not(windows), test))]
use anyhow::bail;
use anyhow::{Context, Result};
use base64::Engine;

/// Keys are stored base64-encoded, so no character in them needs quoting for the store's tools.
const BASE64_PREFIX: &str = "base64:";

/// The keyring service a site's key is filed under.
pub fn service(site: &str) -> String {
    format!("testrail-arriba:{site}")
}

/// The secret stored under `service` and `account`, or None when there is none.
pub fn read(service: &str, account: &str) -> Result<Option<Vec<u8>>> {
    read_platform(service, account)
}

/// Store `secret` under `service` and `account`, replacing any already there.
pub fn write(service: &str, account: &str, secret: &str) -> Result<()> {
    write_platform(service, account, &encode(secret))
}

/// Remove the secret under `service` and `account`, if there is one.
pub fn delete(service: &str, account: &str) -> Result<()> {
    delete_platform(service, account)
}

fn encode(secret: &str) -> String {
    format!(
        "{BASE64_PREFIX}{}",
        base64::engine::general_purpose::STANDARD.encode(secret)
    )
}

/// The stored bytes with the encoding [`write`] adds undone.
pub fn decode(raw: &[u8]) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(raw).ok().map(str::trim);
    match text {
        Some(t) if t.starts_with(BASE64_PREFIX) => base64::engine::general_purpose::STANDARD
            .decode(&t[BASE64_PREFIX.len()..])
            .context("The stored API key is not valid base64"),
        _ => Ok(raw.to_vec()),
    }
}

/// Reject text the store's command line could misread.
#[cfg(any(target_os = "macos", test))]
fn check_plain(what: &str, text: &str) -> Result<()> {
    if text.contains(['\'', '\n', '\r']) {
        bail!("The {what} {text:?} has a quote or line break, which the keychain cannot take");
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn read_platform(service: &str, account: &str) -> Result<Option<Vec<u8>>> {
    let output = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", service, "-a", account, "-w"])
        .output()
        .context("Failed to run security")?;
    if !output.status.success() {
        // errSecItemNotFound
        if output.status.code() == Some(44) {
            return Ok(None);
        }
        bail!(
            "security find-generic-password failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let mut bytes = output.stdout;
    while bytes.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
        bytes.pop();
    }
    Ok(Some(bytes))
}

#[cfg(target_os = "macos")]
fn write_platform(service: &str, account: &str, secret: &str) -> Result<()> {
    use std::io::Write;
    check_plain("site", service)?;
    check_plain("email", account)?;
    // Commands go through stdin, so the secret never shows in the process list.
    let mut child = std::process::Command::new("/usr/bin/security")
        .arg("-i")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to run security")?;
    let command = format!("add-generic-password -U -s '{service}' -a '{account}' -w '{secret}'\n");
    child
        .stdin
        .take()
        .context("Failed to open security's stdin")?
        .write_all(command.as_bytes())
        .context("Failed to write to security")?;
    let output = child.wait_with_output().context("Failed to run security")?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() || !stderr.trim().is_empty() {
        bail!("security add-generic-password failed: {}", stderr.trim());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn delete_platform(service: &str, account: &str) -> Result<()> {
    let output = std::process::Command::new("/usr/bin/security")
        .args(["delete-generic-password", "-s", service, "-a", account])
        .output()
        .context("Failed to run security")?;
    if !output.status.success() && output.status.code() != Some(44) {
        bail!(
            "security delete-generic-password failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn secret_tool(args: &[&str]) -> std::process::Command {
    let mut cmd = std::process::Command::new("secret-tool");
    cmd.args(args);
    cmd
}

#[cfg(target_os = "linux")]
fn run_secret_tool(mut cmd: std::process::Command) -> Result<std::process::Output> {
    match cmd.output() {
        Ok(o) => Ok(o),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!(
            "secret-tool is not installed, so the API key cannot be kept in the Secret Service. Install libsecret-tools, or set {} instead.",
            super::API_KEY
        ),
        Err(e) => Err(e).context("Failed to run secret-tool"),
    }
}

#[cfg(target_os = "linux")]
fn read_platform(service: &str, account: &str) -> Result<Option<Vec<u8>>> {
    let output = run_secret_tool(secret_tool(&[
        "lookup", "service", service, "username", account,
    ]))?;
    if !output.status.success() || output.stdout.is_empty() {
        return Ok(None);
    }
    Ok(Some(output.stdout))
}

#[cfg(target_os = "linux")]
fn write_platform(service: &str, account: &str, secret: &str) -> Result<()> {
    use std::io::Write;
    let mut cmd = secret_tool(&[
        "store", "--label", service, "service", service, "username", account,
    ]);
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!(
            "secret-tool is not installed, so the API key cannot be kept in the Secret Service. Install libsecret-tools, or set {} instead.",
            super::API_KEY
        ),
        Err(e) => return Err(e).context("Failed to run secret-tool"),
    };
    // secret-tool reads the secret from stdin, so it never shows in the process list.
    child
        .stdin
        .take()
        .context("Failed to open secret-tool's stdin")?
        .write_all(secret.as_bytes())
        .context("Failed to write to secret-tool")?;
    let output = child
        .wait_with_output()
        .context("Failed to run secret-tool")?;
    if !output.status.success() {
        bail!(
            "secret-tool store failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn delete_platform(service: &str, account: &str) -> Result<()> {
    let output = run_secret_tool(secret_tool(&[
        "clear", "service", service, "username", account,
    ]))?;
    if !output.status.success() {
        bail!(
            "secret-tool clear failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(windows)]
fn entry(service: &str, account: &str) -> Result<keyring_core::Entry> {
    let store = windows_native_keyring_store::Store::new()
        .context("Failed to open the Windows credential store")?;
    keyring_core::set_default_store(store);
    keyring_core::Entry::new(service, account).context("Failed to describe the credential")
}

#[cfg(windows)]
fn read_platform(service: &str, account: &str) -> Result<Option<Vec<u8>>> {
    match entry(service, account)?.get_secret() {
        Ok(bytes) => Ok(Some(bytes)),
        Err(keyring_core::Error::NoEntry) => Ok(None),
        Err(e) => Err(e).context("Failed to read the credential"),
    }
}

#[cfg(windows)]
fn write_platform(service: &str, account: &str, secret: &str) -> Result<()> {
    entry(service, account)?
        .set_secret(secret.as_bytes())
        .context("Failed to store the credential")
}

#[cfg(windows)]
fn delete_platform(service: &str, account: &str) -> Result<()> {
    match entry(service, account)?.delete_credential() {
        Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
        Err(e) => Err(e).context("Failed to delete the credential"),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn read_platform(_service: &str, _account: &str) -> Result<Option<Vec<u8>>> {
    bail!(
        "Stored logins are not supported on this platform, so set {} instead",
        super::API_KEY
    )
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn write_platform(_service: &str, _account: &str, _secret: &str) -> Result<()> {
    read_platform("", "").map(|_| ())
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn delete_platform(_service: &str, _account: &str) -> Result<()> {
    read_platform("", "").map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_keys_decode_and_raw_ones_pass_through() {
        let stored = encode("a'b/c+d");
        assert!(stored.starts_with(BASE64_PREFIX));
        assert!(!stored.contains('\''));
        assert_eq!(decode(stored.as_bytes()).unwrap(), b"a'b/c+d");
        assert_eq!(decode(b"plain-key\n").unwrap(), b"plain-key\n");
        assert!(decode(b"base64:!!").is_err());
    }

    #[test]
    fn quotes_and_line_breaks_are_refused() {
        assert!(check_plain("email", "me@x.com").is_ok());
        assert!(check_plain("email", "o'neil@x.com").is_err());
        assert!(check_plain("site", "a\nb").is_err());
    }

    #[test]
    fn services_name_the_site() {
        assert_eq!(
            service("acme.testrail.io"),
            "testrail-arriba:acme.testrail.io"
        );
    }
}
