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
        // Step results carry their status as a numeric string.
        let id = match id {
            Value::Number(n) => n.as_i64(),
            Value::String(s) => s.trim().parse().ok(),
            _ => None,
        };
        let Some(id) = id else {
            return String::new();
        };
        self.0
            .iter()
            .find(|s| s["id"].as_i64() == Some(id))
            .and_then(|s| s["label"].as_str().or(s["name"].as_str()))
            .map_or_else(|| format!("status {id}"), str::to_string)
    }

    /// A run or plan's tallies, as in `12 passed, 1 failed, 4 not applicable`, from its `*_count`
    /// fields. Custom status N is counted in `custom_statusN_count` and has ID 5 + N.
    pub fn tally(&self, v: &Value) -> String {
        let get = |k: &str| v[k].as_u64().unwrap_or(0);
        let mut parts: Vec<String> = [
            ("passed", "passed_count"),
            ("blocked", "blocked_count"),
            ("retest", "retest_count"),
            ("failed", "failed_count"),
            ("untested", "untested_count"),
        ]
        .iter()
        .filter(|(_, k)| get(k) > 0)
        .map(|(name, k)| format!("{} {name}", get(k)))
        .collect();
        for n in 1..=7 {
            let count = get(&format!("custom_status{n}_count"));
            if count > 0 {
                let label = self.label(&json!(5 + n)).to_lowercase();
                parts.push(format!("{count} {label}"));
            }
        }
        if parts.is_empty() {
            "no tests".to_string()
        } else {
            parts.join(", ")
        }
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
        assert_eq!(statuses.label(&json!("5")), "Failed");
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

    #[test]
    fn tallies_name_custom_statuses() {
        let statuses = Statuses(vec![
            json!({"id": 1, "name": "passed", "label": "Passed"}),
            json!({"id": 6, "name": "not_applicable", "label": "Not Applicable"}),
        ]);
        let run = json!({
            "passed_count": 12, "failed_count": 1, "untested_count": 27,
            "blocked_count": 0, "custom_status1_count": 4, "custom_status2_count": 3
        });
        assert_eq!(
            statuses.tally(&run),
            "12 passed, 1 failed, 27 untested, 4 not applicable, 3 status 7"
        );
        assert_eq!(statuses.tally(&json!({})), "no tests");
    }
}
