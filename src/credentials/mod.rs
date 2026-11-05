//! Where the login comes from: environment variables first, then the login `auth login` stored.

pub mod config;
pub mod keyring;

use anyhow::{Context, Result, bail};
use base64::Engine;
use std::path::Path;

pub const URL: &str = "TESTRAIL_URL";
pub const EMAIL: &str = "TESTRAIL_EMAIL";
pub const API_KEY: &str = "TESTRAIL_API_KEY";
/// The name the testrail-api Python package reads, accepted in place of [`API_KEY`].
pub const PASSWORD: &str = "TESTRAIL_PASSWORD";

/// Where the login was found.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// The environment variables that supplied it.
    Env(Vec<&'static str>),
    /// A login stored by `auth login`, with its key in the platform keyring.
    Stored,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Env(vars) => write!(f, "environment ({})", vars.join(", ")),
            Source::Stored => write!(f, "stored login (keyring)"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Credentials {
    /// Base URL of the instance, without a trailing slash or `index.php`.
    pub url: String,
    pub email: String,
    /// API key, or the account password where API keys are off.
    pub key: String,
    pub source: Source,
}

impl Credentials {
    /// The site, as the URL without its scheme.
    pub fn site(&self) -> String {
        site_of(&self.url)
    }

    /// Root of the REST API, which TestRail routes through the query string.
    pub fn api_root(&self) -> String {
        format!("{}/index.php?/api/v2", self.url)
    }

    /// Web link to a page such as `cases/view/39`.
    pub fn web_url(&self, page: &str) -> String {
        format!("{}/index.php?/{page}", self.url)
    }

    /// The Authorization header value.
    pub fn header(&self) -> String {
        basic_header(&self.email, &self.key)
    }
}

pub fn basic_header(email: &str, key: &str) -> String {
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{email}:{key}"))
    )
}

/// The login to use: from the environment when set, otherwise the stored one for `site`.
pub fn resolve(site: Option<&str>) -> Result<Credentials> {
    let env = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    if let Some(creds) = from_env(&env, site)? {
        return Ok(creds);
    }
    stored(site)
}

/// Whether the environment supplies a login, so the stored one is not consulted.
pub fn env_present() -> bool {
    [API_KEY, PASSWORD]
        .iter()
        .any(|v| std::env::var(v).is_ok_and(|t| !t.trim().is_empty()))
}

/// Credentials from the environment, or None when it sets no key. A key missing its companions is
/// an error, so a half-set environment is not silently ignored.
fn from_env(
    get: &dyn Fn(&str) -> Option<String>,
    site: Option<&str>,
) -> Result<Option<Credentials>> {
    let Some((key_var, key)) = [API_KEY, PASSWORD]
        .into_iter()
        .find_map(|var| get(var).map(|key| (var, key)))
    else {
        return Ok(None);
    };
    let email = get(EMAIL).with_context(|| format!("{key_var} is set but {EMAIL} is not"))?;
    let (url, vars) = match site {
        Some(site) => (site.to_string(), vec![key_var, EMAIL]),
        None => (
            get(URL).with_context(|| format!("{key_var} is set but {URL} is not"))?,
            vec![URL, EMAIL, key_var],
        ),
    };
    Ok(Some(Credentials {
        url: normalize_url(&url)?,
        email,
        key,
        source: Source::Env(vars),
    }))
}

/// The login `auth login` stored for `site`, or the current one.
fn stored(site: Option<&str>) -> Result<Credentials> {
    let dir = config::dir().context("Could not find the home directory")?;
    stored_in(&dir, site, &keyring::read)
}

/// The secret stored under a keyring service and account, as [`keyring::read`] returns it.
type ReadSecret = dyn Fn(&str, &str) -> Result<Option<Vec<u8>>>;

/// The login stored in the config under `dir`, with its key from `read`.
fn stored_in(dir: &Path, site: Option<&str>, read: &ReadSecret) -> Result<Credentials> {
    let cfg = config::load(dir)?;
    let login = cfg.select(site)?;
    let raw = read(&keyring::service(&login.site()), &login.email)?.with_context(|| {
        format!(
            "The API key for {} on {} is missing from the keyring. Run `testrail-arriba auth login` again.",
            login.email,
            login.site()
        )
    })?;
    let key =
        String::from_utf8(keyring::decode(&raw)?).context("The stored API key is not text")?;
    Ok(Credentials {
        url: login.url.clone(),
        email: login.email.clone(),
        key,
        source: Source::Stored,
    })
}

/// The instance's base URL from a host, a URL, or a link to any page on it.
pub fn normalize_url(text: &str) -> Result<String> {
    let s = text.trim();
    let (scheme, rest) = if let Some(rest) = s.strip_prefix("https://") {
        ("https", rest)
    } else if let Some(rest) = s.strip_prefix("http://") {
        ("http", rest)
    } else {
        ("https", s)
    };
    // Pages hang off index.php, and the API off its query string, so neither is part of the base.
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let rest = rest.find("/index.php").map_or(rest, |i| &rest[..i]);
    let rest = rest.trim_end_matches('/');
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    if host.is_empty() {
        bail!("{text:?} is not a TestRail URL");
    }
    Ok(format!("{scheme}://{}{path}", host.to_lowercase()))
}

/// A base URL without its scheme, as logins are named.
pub fn site_of(url: &str) -> String {
    url.split_once("://")
        .map_or(url, |(_, rest)| rest)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn api_key_login_comes_from_the_environment() {
        let vars = env(&[
            (URL, "https://Acme.testrail.io/index.php?/dashboard"),
            (EMAIL, "me@x.com"),
            (API_KEY, "key"),
        ]);
        let creds = from_env(&|k| vars.get(k).cloned(), None).unwrap().unwrap();
        assert_eq!(creds.url, "https://acme.testrail.io");
        assert_eq!(creds.site(), "acme.testrail.io");
        assert_eq!(
            creds.api_root(),
            "https://acme.testrail.io/index.php?/api/v2"
        );
        assert_eq!(
            creds.web_url("cases/view/39"),
            "https://acme.testrail.io/index.php?/cases/view/39"
        );
        assert_eq!(creds.header(), "Basic bWVAeC5jb206a2V5");
        assert_eq!(
            creds.source.to_string(),
            "environment (TESTRAIL_URL, TESTRAIL_EMAIL, TESTRAIL_API_KEY)"
        );
    }

    #[test]
    fn password_variable_stands_in_for_the_key() {
        let vars = env(&[(EMAIL, "me@x.com"), (PASSWORD, "pw")]);
        let creds = from_env(&|k| vars.get(k).cloned(), Some("tr.example.com"))
            .unwrap()
            .unwrap();
        assert_eq!(creds.key, "pw");
        assert_eq!(creds.url, "https://tr.example.com");
        assert_eq!(
            creds.source.to_string(),
            "environment (TESTRAIL_PASSWORD, TESTRAIL_EMAIL)"
        );
    }

    #[test]
    fn half_set_environment_is_an_error() {
        let vars = env(&[(API_KEY, "key")]);
        let err = from_env(&|k| vars.get(k).cloned(), None).unwrap_err();
        assert_eq!(
            err.to_string(),
            "TESTRAIL_API_KEY is set but TESTRAIL_EMAIL is not"
        );
        let vars = env(&[(API_KEY, "key"), (EMAIL, "me@x.com")]);
        let err = from_env(&|k| vars.get(k).cloned(), None).unwrap_err();
        assert_eq!(
            err.to_string(),
            "TESTRAIL_API_KEY is set but TESTRAIL_URL is not"
        );
    }

    #[test]
    fn no_key_defers_to_the_stored_login() {
        let vars = env(&[(URL, "x.testrail.io"), (EMAIL, "me@x.com")]);
        assert!(from_env(&|k| vars.get(k).cloned(), None).unwrap().is_none());
    }

    #[test]
    fn stored_logins_take_their_key_from_the_keyring() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = config::Config::default();
        cfg.upsert(config::Login {
            url: "https://acme.testrail.io".into(),
            email: "me@x.com".into(),
        });
        config::save(dir.path(), &cfg).unwrap();

        let read = |service: &str, account: &str| {
            assert_eq!(service, "testrail-arriba:acme.testrail.io");
            assert_eq!(account, "me@x.com");
            Ok(Some(b"base64:a2V5".to_vec()))
        };
        let creds = stored_in(dir.path(), None, &read).unwrap();
        assert_eq!(creds.key, "key");
        assert_eq!(creds.url, "https://acme.testrail.io");
        assert_eq!(creds.source, Source::Stored);
        assert_eq!(creds.source.to_string(), "stored login (keyring)");

        let missing = |_: &str, _: &str| Ok(None);
        let err = stored_in(dir.path(), None, &missing)
            .unwrap_err()
            .to_string();
        assert!(err.contains("missing from the keyring"), "{err}");
        let err = stored_in(dir.path(), Some("other.testrail.io"), &read).unwrap_err();
        assert!(
            err.to_string()
                .starts_with("No login for other.testrail.io"),
            "{err}"
        );
    }

    #[test]
    fn urls_normalize_to_the_base() {
        assert_eq!(
            normalize_url("acme.testrail.io").unwrap(),
            "https://acme.testrail.io"
        );
        assert_eq!(
            normalize_url(" http://Host:8080/testrail/index.php?/runs/view/3 ").unwrap(),
            "http://host:8080/testrail"
        );
        assert_eq!(
            normalize_url("https://acme.testrail.io/").unwrap(),
            "https://acme.testrail.io"
        );
        assert!(normalize_url("https://").is_err());
        assert_eq!(site_of("http://host:8080/testrail"), "host:8080/testrail");
    }
}
