//! The stored logins: each site's URL and email, with the API key kept in the keyring.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::{normalize_url, site_of};

const FILE: &str = "config.yml";

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Config {
    /// Site of the login used when --site names none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    #[serde(default)]
    pub logins: Vec<Login>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Login {
    pub url: String,
    pub email: String,
}

impl Login {
    pub fn site(&self) -> String {
        site_of(&self.url)
    }
}

/// Where the config lives: `$XDG_CONFIG_HOME/testrail-arriba`, else `~/.config/testrail-arriba`.
pub fn dir() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|h| h.join(".config")))
        .context("Could not find the home directory")?;
    Ok(base.join("testrail-arriba"))
}

/// The config in `dir`, or an empty one when there is none yet.
pub fn load(dir: &Path) -> Result<Config> {
    let path = dir.join(FILE);
    if !path.exists() {
        return Ok(Config::default());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    serde_yaml::from_str(&text).with_context(|| format!("Failed to parse {}", path.display()))
}

pub fn save(dir: &Path, config: &Config) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    let path = dir.join(FILE);
    std::fs::write(&path, serde_yaml::to_string(config)?)
        .with_context(|| format!("Failed to write {}", path.display()))
}

impl Config {
    /// The login for `site`, or else the current one, or else the only one.
    pub fn select(&self, site: Option<&str>) -> Result<&Login> {
        if let Some(site) = site {
            let wanted = site_of(&normalize_url(site)?);
            return self
                .logins
                .iter()
                .find(|l| l.site() == wanted)
                .with_context(|| {
                    format!(
                        "No login for {wanted}. Run `testrail-arriba auth login --site {wanted}`."
                    )
                });
        }
        if let Some(current) = &self.current
            && let Some(login) = self.logins.iter().find(|l| &l.site() == current)
        {
            return Ok(login);
        }
        match self.logins.as_slice() {
            [one] => Ok(one),
            [] => bail!(
                "Not logged in. Run `testrail-arriba auth login`, or set {}, {}, and {}.",
                super::URL,
                super::EMAIL,
                super::API_KEY
            ),
            many => bail!(
                "Several logins are stored, so pass --site: {}",
                many.iter().map(Login::site).collect::<Vec<_>>().join(", ")
            ),
        }
    }

    /// Store `login`, replacing any for the same site, and make it current.
    pub fn upsert(&mut self, login: Login) {
        let site = login.site();
        self.logins.retain(|l| l.site() != site);
        self.logins.push(login);
        self.current = Some(site);
    }

    /// Forget the login for `site`, returning it.
    pub fn remove(&mut self, site: &str) -> Option<Login> {
        let index = self.logins.iter().position(|l| l.site() == site)?;
        let login = self.logins.remove(index);
        if self.current.as_deref() == Some(site) {
            self.current = self.logins.first().map(Login::site);
        }
        Some(login)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn login(url: &str) -> Login {
        Login {
            url: url.to_string(),
            email: "me@x.com".to_string(),
        }
    }

    #[test]
    fn logins_round_trip_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).unwrap().logins.is_empty());
        let mut config = Config::default();
        config.upsert(login("https://a.testrail.io"));
        save(dir.path(), &config).unwrap();
        let loaded = load(dir.path()).unwrap();
        assert_eq!(loaded.current.as_deref(), Some("a.testrail.io"));
        assert_eq!(loaded.logins, vec![login("https://a.testrail.io")]);
    }

    #[test]
    fn selection_prefers_the_site_then_current_then_the_only_one() {
        let mut config = Config::default();
        let err = config.select(None).unwrap_err().to_string();
        assert!(err.starts_with("Not logged in"), "{err}");

        config.logins.push(login("https://a.testrail.io"));
        assert_eq!(config.select(None).unwrap().site(), "a.testrail.io");

        config.logins.push(login("https://b.testrail.io"));
        let err = config.select(None).unwrap_err().to_string();
        assert!(err.contains("a.testrail.io, b.testrail.io"), "{err}");

        config.current = Some("b.testrail.io".to_string());
        assert_eq!(config.select(None).unwrap().site(), "b.testrail.io");
        assert_eq!(
            config
                .select(Some("https://A.testrail.io/index.php?/runs"))
                .unwrap()
                .site(),
            "a.testrail.io"
        );
        assert!(config.select(Some("c.testrail.io")).is_err());
    }

    #[test]
    fn upsert_replaces_and_remove_moves_current() {
        let mut config = Config::default();
        config.upsert(login("https://a.testrail.io"));
        config.upsert(login("https://b.testrail.io"));
        let mut again = login("https://a.testrail.io");
        again.email = "other@x.com".to_string();
        config.upsert(again);
        assert_eq!(config.logins.len(), 2);
        assert_eq!(config.current.as_deref(), Some("a.testrail.io"));

        assert!(config.remove("a.testrail.io").is_some());
        assert_eq!(config.current.as_deref(), Some("b.testrail.io"));
        assert!(config.remove("a.testrail.io").is_none());
    }

    #[test]
    fn the_config_lives_in_a_testrail_arriba_folder() {
        let dir = dir().unwrap();
        assert_eq!(dir.file_name().unwrap(), "testrail-arriba");
    }
}
