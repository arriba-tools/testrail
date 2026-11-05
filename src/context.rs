use anyhow::Result;

use crate::api::Client;
use crate::credentials::{self, Credentials};
use crate::ids::Kind;

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

    /// Web link to the page of a case, run, or other kind that has one.
    pub fn page_url(&self, kind: Kind, id: u64) -> String {
        let page = kind.page().expect("only kinds with a page are linked");
        self.creds.web_url(&format!("{page}/{id}"))
    }
}
