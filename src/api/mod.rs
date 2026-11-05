//! Communication with TestRail: the authenticated REST client, paging through bulk endpoints, and
//! the status table results are written against.

mod client;
pub mod statuses;

pub use client::{Client, init_tls};

use anyhow::{Result, bail};
use serde_json::Value;

/// Most items TestRail returns in one page.
const PAGE: usize = 250;

/// Items from a bulk endpoint, and whether more remained past the limit.
pub struct Page {
    pub items: Vec<Value>,
    pub more: bool,
}

/// Items of a bulk endpoint under `key`, following its pages, up to `limit`. Servers older than
/// TestRail 6.7 answer with a bare array, which is taken whole.
pub fn list(client: &Client, endpoint: &str, key: &str, limit: Option<usize>) -> Result<Page> {
    let mut items: Vec<Value> = Vec::new();
    if limit == Some(0) {
        return Ok(Page { items, more: true });
    }
    loop {
        let size = limit.map_or(PAGE, |l| (l - items.len()).min(PAGE));
        let url = with_params(
            endpoint,
            &[
                ("limit", size.to_string()),
                ("offset", items.len().to_string()),
            ],
        );
        let (batch, next) = match client.get(&url)? {
            Value::Array(all) => (all, false),
            Value::Object(mut page) => {
                let next = page
                    .get("_links")
                    .and_then(|l| l.get("next"))
                    .is_some_and(Value::is_string);
                match page.remove(key) {
                    Some(Value::Array(batch)) => (batch, next),
                    _ => bail!("TestRail's answer to {endpoint} has no {key}"),
                }
            }
            other => bail!("TestRail answered {endpoint} with {other}"),
        };
        let count = batch.len();
        items.extend(batch);
        if let Some(l) = limit
            && items.len() >= l
        {
            let more = next || items.len() > l;
            items.truncate(l);
            return Ok(Page { items, more });
        }
        if !next || count == 0 {
            return Ok(Page { items, more: false });
        }
    }
}

/// Cut `items` to `limit`, as after a filter applied here, returning whether any were cut.
pub fn cut<T>(items: &mut Vec<T>, limit: Option<usize>) -> bool {
    let more = limit.is_some_and(|l| items.len() > l);
    items.truncate(limit.unwrap_or(usize::MAX));
    more
}

/// An endpoint with query parameters, which TestRail takes after `&` since its route already
/// fills the query string.
pub fn with_params(endpoint: &str, params: &[(&str, String)]) -> String {
    let mut out = endpoint.to_string();
    for (name, value) in params {
        out.push('&');
        out.push_str(name);
        out.push('=');
        out.push_str(&encode(value));
    }
    out
}

/// Percent-encode a query value, keeping commas for TestRail's ID lists.
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b',' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use serde_json::json;

    #[test]
    fn params_are_appended_and_encoded() {
        assert_eq!(
            with_params(
                "get_cases/2",
                &[("filter", "a b&c".into()), ("status_id", "1,5".into())]
            ),
            "get_cases/2&filter=a%20b%26c&status_id=1,5"
        );
    }

    #[test]
    fn pages_follow_next_links_up_to_the_limit() {
        let mut server = Server::new();
        server
            .mock("GET", "/index.php?/api/v2/get_cases/2&limit=250&offset=0")
            .with_body(
                json!({"cases": [{"id": 1}, {"id": 2}], "_links": {"next": "/api/v2/get_cases/2&limit=250&offset=2"}})
                    .to_string(),
            )
            .create();
        server
            .mock("GET", "/index.php?/api/v2/get_cases/2&limit=250&offset=2")
            .with_body(json!({"cases": [{"id": 3}], "_links": {"next": null}}).to_string())
            .create();
        server
            .mock("GET", "/index.php?/api/v2/get_cases/2&limit=2&offset=0")
            .with_body(
                json!({"cases": [{"id": 1}, {"id": 2}], "_links": {"next": "/api/v2/x"}})
                    .to_string(),
            )
            .create();
        let client = Client::for_tests(&server.url());
        let all = list(&client, "get_cases/2", "cases", None).unwrap();
        assert_eq!(all.items.len(), 3);
        assert!(!all.more);
        let some = list(&client, "get_cases/2", "cases", Some(2)).unwrap();
        assert_eq!(some.items.len(), 2);
        assert!(some.more);
    }

    #[test]
    fn bare_arrays_are_taken_whole() {
        let mut server = Server::new();
        server
            .mock("GET", "/index.php?/api/v2/get_projects&limit=1&offset=0")
            .with_body(r#"[{"id": 1}, {"id": 2}]"#)
            .create();
        let client = Client::for_tests(&server.url());
        let page = list(&client, "get_projects", "projects", Some(1)).unwrap();
        assert_eq!(page.items.len(), 1);
        assert!(page.more);
    }

    #[test]
    fn missing_keys_are_errors() {
        let mut server = Server::new();
        server
            .mock("GET", "/index.php?/api/v2/get_runs/1&limit=250&offset=0")
            .with_body(r#"{"plans": []}"#)
            .create();
        let client = Client::for_tests(&server.url());
        let err = list(&client, "get_runs/1", "runs", None).err().unwrap();
        assert_eq!(
            err.to_string(),
            "TestRail's answer to get_runs/1 has no runs"
        );
    }
}
