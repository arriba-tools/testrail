use anyhow::{Context, Result, bail};
use dialoguer::{Confirm, Input, Password};
use std::io::{IsTerminal, Read};
use std::path::Path;

/// True when a person can answer prompts. Prompts render on stderr so stdout stays clean.
pub fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

/// Ask a yes/no question. `yes` answers it up front, and without a terminal it is required.
pub fn confirm(prompt: &str, yes: bool) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    if !interactive() {
        bail!("{prompt} Pass --yes to confirm without a prompt.");
    }
    Ok(Confirm::new()
        .with_prompt(prompt)
        .default(false)
        .interact()?)
}

/// Ask for a line of text, failing with `missing` when no one can answer.
pub fn input(prompt: &str, missing: &str) -> Result<String> {
    if !interactive() {
        bail!("{missing}");
    }
    Ok(Input::<String>::new()
        .with_prompt(prompt)
        .interact_text()?
        .trim()
        .to_string())
}

/// Ask for a secret without echoing it, failing with `missing` when no one can answer.
pub fn password(prompt: &str, missing: &str) -> Result<String> {
    if !interactive() {
        bail!("{missing}");
    }
    Ok(Password::new()
        .with_prompt(prompt)
        .interact()?
        .trim()
        .to_string())
}

/// All of stdin.
pub fn read_stdin() -> Result<String> {
    let mut s = String::new();
    std::io::stdin()
        .read_to_string(&mut s)
        .context("Failed to read stdin")?;
    Ok(s)
}

/// A file's text, or stdin's for `-`.
pub fn read_file(path: &Path) -> Result<String> {
    if path.as_os_str() == "-" {
        return read_stdin();
    }
    std::fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yes_confirms_without_asking() {
        assert!(confirm("Delete it?", true).unwrap());
    }

    #[test]
    fn files_read_whole() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("steps.json");
        std::fs::write(&path, "[]\n").unwrap();
        assert_eq!(read_file(&path).unwrap(), "[]\n");
        assert!(read_file(&dir.path().join("missing")).is_err());
    }
}
