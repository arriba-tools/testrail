use anyhow::Result;

use crate::api::Client;
use crate::credentials::{self, Credentials};

/// State shared by every command: the API client, the login it uses, and the global flags.
pub struct Ctx {
    pub client: Client,
    pub creds: Credentials,
    pub json: bool,
    pub yes: bool,
}

impl Ctx {
    pub fn new(site: Option<&str>, json: bool, yes: bool) -> Result<Self> {
        let creds = credentials::resolve(site)?;
        let client = Client::new(&creds)?;
        Ok(Self {
            client,
            creds,
            json,
            yes,
        })
    }

    /// A context for an instance served at `base`, such as a local mock server in tests.
    #[cfg(test)]
    pub fn for_tests(base: &str) -> Self {
        Self {
            client: Client::for_tests(base),
            creds: Credentials {
                url: base.trim_end_matches('/').to_string(),
                email: "me@x.com".to_string(),
                key: "key".to_string(),
                source: credentials::Source::Stored,
            },
            json: false,
            yes: true,
        }
    }

    /// Web link to a page such as `cases/view/39`.
    pub fn web_url(&self, page: &str) -> String {
        self.creds.web_url(page)
    }
}
