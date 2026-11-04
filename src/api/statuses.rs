//! Result statuses: the five every instance has and the custom ones an admin adds.

use anyhow::{Result, bail};
use serde_json::{Value, json};

use super::Client;

/// The system statuses, by name and ID.
const BUILTIN: [(&str, i64); 5] = [
    ("passed", 1),
    ("blocked", 2),
    ("untested", 3),
    ("retest", 4),
    ("failed", 5),
];

/// The instance's statuses, as `get_statuses` lists them.
pub struct Statuses(Vec<Value>);

impl Statuses {
    pub fn fetch(client: &Client) -> Result<Self> {
        match client.get("get_statuses")? {
            Value::Array(all) => Ok(Self(all)),
            other => bail!("TestRail answered get_statuses with {other}"),
        }
    }

    /// The system statuses alone, for when the instance's are not needed.
    pub fn builtin() -> Self {
        Self(
            BUILTIN
                .iter()
                .map(|(name, id)| {
                    let mut label = name.to_string();
                    label[..1].make_ascii_uppercase();
                    json!({"id": id, "name": name, "label": label})
                })
                .collect(),
        )
    }

    pub fn all(&self) -> &[Value] {
        &self.0
    }

    /// The label TestRail shows for `id`.
    pub fn label(&self, id: &Value) -> String {
        let Some(id) = id.as_i64() else {
            return String::new();
        };
        self.0
            .iter()
            .find(|s| s["id"].as_i64() == Some(id))
            .and_then(|s| s["label"].as_str().or(s["name"].as_str()))
            .map_or_else(|| format!("status {id}"), str::to_string)
    }

    /// The ID for a status given by ID, name, or label, ignoring case.
    pub fn id(&self, text: &str) -> Result<i64> {
        let t = text.trim();
        if let Ok(id) = t.parse::<i64>() {
            return Ok(id);
        }
        let found = self.0.iter().find(|s| {
            ["name", "label"]
                .iter()
                .any(|k| s[*k].as_str().is_some_and(|v| v.eq_ignore_ascii_case(t)))
        });
        match found.and_then(|s| s["id"].as_i64()) {
            Some(id) => Ok(id),
            None => bail!(
                "No status named {t:?}. The statuses are {}",
                self.0
                    .iter()
                    .filter_map(|s| s["name"].as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

/// The ID for a status, asking the instance only when the name is not a system one.
pub fn resolve(client: &Client, text: &str) -> Result<i64> {
    let builtin = Statuses::builtin();
    match builtin.id(text) {
        Ok(id) => Ok(id),
        Err(_) => Statuses::fetch(client)?.id(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[test]
    fn builtin_names_map_without_a_request() {
        let statuses = Statuses::builtin();
        assert_eq!(statuses.id("Passed").unwrap(), 1);
        assert_eq!(statuses.id(" failed ").unwrap(), 5);
        assert_eq!(statuses.id("4").unwrap(), 4);
        assert_eq!(statuses.label(&json!(2)), "Blocked");
        assert_eq!(statuses.label(&json!(9)), "status 9");
        assert_eq!(statuses.label(&json!(null)), "");
        let err = statuses.id("nope").unwrap_err().to_string();
        assert!(
            err.contains("passed, blocked, untested, retest, failed"),
            "{err}"
        );
    }

    #[test]
    fn custom_names_are_looked_up() {
        let mut server = Server::new();
        let mock = server
            .mock("GET", "/index.php?/api/v2/get_statuses")
            .with_body(
                r#"[{"id": 1, "name": "passed", "label": "Passed"},
                    {"id": 6, "name": "custom_status1", "label": "In Progress"}]"#,
            )
            .create();
        let client = Client::for_tests(&server.url());
        assert_eq!(resolve(&client, "failed").unwrap(), 5);
        assert_eq!(resolve(&client, "in progress").unwrap(), 6);
        mock.assert();
    }
}
