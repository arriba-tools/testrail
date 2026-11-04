use anyhow::{Context, Result, bail};
use reqwest::blocking::{Client as Http, RequestBuilder, Response};
use reqwest::header::{CONTENT_TYPE, RETRY_AFTER};
use reqwest::{Method, StatusCode};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

use crate::credentials::Credentials;

/// Retries after a 429 before giving up.
const RETRIES: u32 = 5;
/// Wait when a 429 names no Retry-After.
const DEFAULT_WAIT: Duration = Duration::from_secs(5);
/// Retries of a read after a gateway error, waiting twice as long each time from one second.
const GATEWAY_RETRIES: u32 = 3;

/// Install the TLS crypto provider once, before any client is built. The ring provider builds
/// without the C toolchains aws-lc-rs needs.
pub fn init_tls() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// REST client for one TestRail login.
pub struct Client {
    http: Http,
    auth: String,
    /// The API root, ending in `index.php?/api/v2`.
    root: String,
    /// Longest wait honored from a 429's Retry-After.
    max_wait: Duration,
}

impl Client {
    pub fn new(creds: &Credentials) -> Result<Self> {
        init_tls();
        let http = Http::builder()
            .user_agent(concat!("testrail-arriba/", env!("CARGO_PKG_VERSION")))
            // Uploads and downloads of large logs outlast the default 30 seconds.
            .timeout(Duration::from_secs(300))
            .build()
            .context("Failed to create HTTP client")?;
        Ok(Self {
            http,
            auth: creds.header(),
            root: creds.api_root(),
            max_wait: Duration::from_secs(60),
        })
    }

    /// A client for an instance served at `base`, such as a local mock server in tests.
    #[cfg(test)]
    pub fn for_tests(base: &str) -> Self {
        init_tls();
        // The test server is local, so skip any proxy the environment configures.
        Self {
            http: Http::builder().no_proxy().build().unwrap(),
            auth: crate::credentials::basic_header("me@x.com", "key"),
            root: format!("{}/index.php?/api/v2", base.trim_end_matches('/')),
            max_wait: Duration::ZERO,
        }
    }

    /// The URL of an endpoint such as `get_case/1`. The `/api/v2/` prefix that pagination links
    /// carry is accepted too.
    pub fn url(&self, endpoint: &str) -> String {
        let e = endpoint.trim();
        let e = e
            .strip_prefix("index.php?")
            .unwrap_or(e)
            .trim_start_matches('/');
        let e = e.strip_prefix("api/v2/").unwrap_or(e);
        format!("{}/{e}", self.root)
    }

    fn request(&self, method: Method, endpoint: &str) -> RequestBuilder {
        self.http
            .request(method, self.url(endpoint))
            .header(reqwest::header::AUTHORIZATION, &self.auth)
    }

    /// Send the request `build` makes, building it again to retry when TestRail rate limits. Reads
    /// also retry gateway errors, which writes cannot since the server may have acted on them.
    fn execute(
        &self,
        what: &str,
        read: bool,
        build: &dyn Fn() -> Result<RequestBuilder>,
    ) -> Result<Response> {
        let mut attempt = 0;
        let mut gateway = 0;
        loop {
            let resp = build()?.send().with_context(|| format!("{what} failed"))?;
            let status = resp.status();
            if read && gateway < GATEWAY_RETRIES && matches!(status.as_u16(), 502..=504) {
                let wait = Duration::from_secs(1 << gateway).min(self.max_wait);
                if !wait.is_zero() {
                    std::thread::sleep(wait);
                }
                gateway += 1;
                continue;
            }
            if status == StatusCode::TOO_MANY_REQUESTS && attempt < RETRIES {
                let wait = retry_after(&resp).min(self.max_wait);
                if !wait.is_zero() {
                    eprintln!(
                        "TestRail is rate limiting, retrying in {}s",
                        wait.as_secs_f32().ceil()
                    );
                    std::thread::sleep(wait);
                }
                attempt += 1;
                continue;
            }
            return check(resp, what);
        }
    }

    fn send(&self, method: Method, endpoint: &str, body: Option<&Value>) -> Result<Value> {
        let what = format!("{method} {endpoint}");
        let read = method == Method::GET;
        let resp = self.execute(&what, read, &|| {
            // TestRail rejects API requests that do not declare JSON, even ones without a body.
            let req = self
                .request(method.clone(), endpoint)
                .header(CONTENT_TYPE, "application/json");
            Ok(match body {
                Some(b) => req.json(b),
                None => req,
            })
        })?;
        parse_json(resp)
    }

    pub fn get(&self, endpoint: &str) -> Result<Value> {
        self.send(Method::GET, endpoint, None)
    }

    pub fn post(&self, endpoint: &str, body: &Value) -> Result<Value> {
        self.send(Method::POST, endpoint, Some(body))
    }

    /// POST a file as the multipart `attachment` field the attachment endpoints take.
    pub fn upload(&self, endpoint: &str, file: &Path) -> Result<Value> {
        if !file.is_file() {
            bail!("{} is not a file", file.display());
        }
        let what = format!("POST {endpoint}");
        let resp = self.execute(&what, false, &|| {
            let form = reqwest::blocking::multipart::Form::new()
                .file("attachment", file)
                .with_context(|| format!("Failed to read {}", file.display()))?;
            Ok(self.request(Method::POST, endpoint).multipart(form))
        })?;
        parse_json(resp)
    }

    /// GET an endpoint that answers with a file rather than JSON.
    pub fn fetch(&self, endpoint: &str) -> Result<Response> {
        let what = format!("GET {endpoint}");
        self.execute(&what, true, &|| Ok(self.request(Method::GET, endpoint)))
    }
}

/// The wait a 429 asks for, in seconds as TestRail sends it.
fn retry_after(resp: &Response) -> Duration {
    resp.headers()
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s >= 0.0)
        .map_or(DEFAULT_WAIT, Duration::from_secs_f64)
}

/// Fail with TestRail's own message on a non-2xx response.
fn check(resp: Response, what: &str) -> Result<Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let text = resp.text().unwrap_or_default();
    let message = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v["error"].as_str().map(str::to_string))
        .unwrap_or_else(|| {
            let t = text.trim();
            if t.is_empty() || t.starts_with('<') {
                status
                    .canonical_reason()
                    .unwrap_or("request failed")
                    .to_string()
            } else {
                t.to_string()
            }
        });
    bail!("TestRail API {} on {what}: {message}", status.as_u16())
}

/// Parse a JSON body, mapping an empty one to Null.
fn parse_json(resp: Response) -> Result<Value> {
    let text = resp.text().context("Failed to read response body")?;
    // Some older servers put this line before the JSON.
    let text = text
        .strip_prefix("USER AUTHENTICATION SUCCESSFUL!\n")
        .unwrap_or(&text);
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(text).context("Failed to parse JSON response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Matcher, Server};
    use serde_json::json;

    #[test]
    fn urls_route_through_index_php() {
        let client = Client::for_tests("https://x.testrail.io/");
        let want = "https://x.testrail.io/index.php?/api/v2/get_cases/2&offset=250";
        assert_eq!(client.url("get_cases/2&offset=250"), want);
        assert_eq!(client.url("/api/v2/get_cases/2&offset=250"), want);
        assert_eq!(client.url("index.php?/api/v2/get_cases/2&offset=250"), want);
    }

    #[test]
    fn get_sends_basic_auth_and_json_content_type() {
        let mut server = Server::new();
        let mock = server
            .mock("GET", "/index.php?/api/v2/get_case/39")
            .match_header("authorization", "Basic bWVAeC5jb206a2V5")
            .match_header("content-type", "application/json")
            .with_body(r#"{"id": 39}"#)
            .create();
        let client = Client::for_tests(&server.url());
        assert_eq!(client.get("get_case/39").unwrap()["id"], 39);
        mock.assert();
    }

    #[test]
    fn post_sends_the_body() {
        let mut server = Server::new();
        server
            .mock("POST", "/index.php?/api/v2/add_result/7")
            .match_body(Matcher::Json(json!({"status_id": 1})))
            .with_body(r#"{"id": 100}"#)
            .create();
        let client = Client::for_tests(&server.url());
        let v = client
            .post("add_result/7", &json!({"status_id": 1}))
            .unwrap();
        assert_eq!(v["id"], 100);
    }

    #[test]
    fn errors_carry_testrail_messages() {
        let mut server = Server::new();
        server
            .mock("GET", "/index.php?/api/v2/get_case/0")
            .with_status(400)
            .with_body(r#"{"error": "Field :case_id is not a valid test case."}"#)
            .create();
        server
            .mock("GET", "/index.php?/api/v2/get_run/1")
            .with_status(403)
            .with_body("<html>nope</html>")
            .create();
        server
            .mock("POST", "/index.php?/api/v2/x")
            .with_status(500)
            .with_body("plain failure")
            .create();
        let client = Client::for_tests(&server.url());
        assert_eq!(
            client.get("get_case/0").unwrap_err().to_string(),
            "TestRail API 400 on GET get_case/0: Field :case_id is not a valid test case."
        );
        assert_eq!(
            client.get("get_run/1").unwrap_err().to_string(),
            "TestRail API 403 on GET get_run/1: Forbidden"
        );
        assert_eq!(
            client.post("x", &json!({})).unwrap_err().to_string(),
            "TestRail API 500 on POST x: plain failure"
        );
    }

    #[test]
    fn rate_limits_are_retried() {
        let mut server = Server::new();
        let limited = server
            .mock("GET", "/index.php?/api/v2/get_projects")
            .with_status(429)
            .with_header("retry-after", "1")
            .expect(2)
            .create();
        server
            .mock("GET", "/index.php?/api/v2/get_projects")
            .with_body("[]")
            .create();
        let client = Client::for_tests(&server.url());
        assert_eq!(client.get("get_projects").unwrap(), json!([]));
        limited.assert();
    }

    #[test]
    fn reads_retry_gateway_errors_and_writes_do_not() {
        let mut server = Server::new();
        let timeout = server
            .mock("GET", "/index.php?/api/v2/get_run/1")
            .with_status(504)
            .expect(2)
            .create();
        server
            .mock("GET", "/index.php?/api/v2/get_run/1")
            .with_body("{}")
            .create();
        let write = server
            .mock("POST", "/index.php?/api/v2/add_result/1")
            .with_status(504)
            .expect(1)
            .create();
        let client = Client::for_tests(&server.url());
        assert_eq!(client.get("get_run/1").unwrap(), json!({}));
        timeout.assert();
        assert!(client.post("add_result/1", &json!({})).is_err());
        write.assert();
    }

    #[test]
    fn empty_and_prefixed_bodies_parse() {
        let mut server = Server::new();
        server
            .mock("POST", "/index.php?/api/v2/delete_attachment/5")
            .create();
        server
            .mock("GET", "/index.php?/api/v2/get_user/1")
            .with_body("USER AUTHENTICATION SUCCESSFUL!\n{\"id\": 1}")
            .create();
        let client = Client::for_tests(&server.url());
        assert_eq!(
            client.post("delete_attachment/5", &json!({})).unwrap(),
            Value::Null
        );
        assert_eq!(client.get("get_user/1").unwrap()["id"], 1);
    }

    #[test]
    fn uploads_send_the_attachment_field() {
        let mut server = Server::new();
        server
            .mock("POST", "/index.php?/api/v2/add_attachment_to_run/16")
            .match_header("content-type", Matcher::Regex("multipart/form-data".into()))
            .match_body(Matcher::Regex(
                "name=\"attachment\"; filename=\"shot.png\"".into(),
            ))
            .with_body(r#"{"attachment_id": 55}"#)
            .create();
        let client = Client::for_tests(&server.url());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shot.png");
        std::fs::write(&path, "png").unwrap();
        let v = client.upload("add_attachment_to_run/16", &path).unwrap();
        assert_eq!(v["attachment_id"], 55);
        let err = client
            .upload("add_attachment_to_run/16", &dir.path().join("missing.png"))
            .unwrap_err();
        assert!(err.to_string().ends_with("is not a file"), "{err}");
    }
}
