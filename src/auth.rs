use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::api::Client;
use crate::cli::{AuthCommand, LoginArgs, TokenArgs};
use crate::credentials::{self, Credentials, Source, config, keyring};
use crate::output::{print_json, table, text};
use crate::prompt;

pub fn run(site: Option<&str>, json: bool, yes: bool, cmd: AuthCommand) -> Result<()> {
    match cmd {
        AuthCommand::Status => status(site, json),
        AuthCommand::Login(args) => login(site, json, args),
        AuthCommand::Logout => logout(site, json, yes),
        AuthCommand::Token(args) => token(site, args),
    }
}

/// Report the login and who TestRail says it belongs to, exiting 1 when there is none that works.
fn status(site: Option<&str>, json: bool) -> Result<()> {
    let creds = match credentials::resolve(site) {
        Ok(c) => c,
        Err(e) => return not_logged_in(json, &format!("{e:#}")),
    };
    let me = match current_user(&creds) {
        Ok(me) => me,
        Err(e) => {
            return not_logged_in(
                json,
                &format!(
                    "{} rejected the login from {}: {e:#}",
                    creds.site(),
                    creds.source
                ),
            );
        }
    };
    print_login(&creds, &me, json)
}

fn current_user(creds: &Credentials) -> Result<Value> {
    Client::new(creds)?.get("get_current_user")
}

fn print_login(creds: &Credentials, me: &Value, json: bool) -> Result<()> {
    if json {
        return print_json(&json!({
            "logged_in": true,
            "url": creds.url,
            "user_id": me["id"],
            "name": me["name"],
            "email": me["email"],
            "source": creds.source.to_string(),
        }));
    }
    println!(
        "✓ Logged in to {} as {} ({})",
        creds.site(),
        text(&me["name"]),
        text(&me["email"])
    );
    let rows = vec![
        vec!["Source".to_string(), creds.source.to_string()],
        vec!["User".to_string(), text(&me["id"])],
    ];
    println!("{}", table(&rows, "  "));
    Ok(())
}

fn not_logged_in(json: bool, reason: &str) -> Result<()> {
    if json {
        print_json(&json!({ "logged_in": false, "error": reason }))?;
    } else {
        // Config errors already open with "Not logged in."
        match reason.strip_prefix("Not logged in. ") {
            Some(rest) => eprintln!("✗ Not logged in. {rest}"),
            None => eprintln!("✗ Not logged in: {reason}"),
        }
    }
    std::process::exit(1)
}

/// Check a login against the site, then keep its URL and email in the config and its key in the
/// keyring.
fn login(site: Option<&str>, json: bool, args: LoginArgs) -> Result<()> {
    if credentials::env_present() {
        bail!(
            "The login comes from the environment ({} or {}). Unset it to store a login.",
            credentials::API_KEY,
            credentials::PASSWORD
        );
    }
    let url = match site {
        Some(s) => s.to_string(),
        None => prompt::input("TestRail URL", "Pass --site with the TestRail URL")?,
    };
    let url = credentials::normalize_url(&url)?;
    let email = match args.email {
        Some(e) => e.trim().to_string(),
        None => prompt::input("Email", "Pass --email with the account's email")?,
    };
    let key = if args.token {
        prompt::read_stdin()?.trim().to_string()
    } else {
        prompt::password(
            "API key (from My Settings > API Keys)",
            "Pass --token and the API key on stdin",
        )?
    };
    if key.is_empty() {
        bail!("The API key is empty");
    }
    let creds = Credentials {
        url,
        email,
        key,
        source: Source::Stored,
    };
    let me =
        current_user(&creds).with_context(|| format!("{} rejected the login", creds.site()))?;
    keyring::write(&keyring::service(&creds.site()), &creds.email, &creds.key)?;
    let dir = config::dir()?;
    let mut cfg = config::load(&dir)?;
    cfg.upsert(config::Login {
        url: creds.url.clone(),
        email: creds.email.clone(),
    });
    config::save(&dir, &cfg)?;
    print_login(&creds, &me, json)
}

/// Forget a stored login and its key.
fn logout(site: Option<&str>, json: bool, yes: bool) -> Result<()> {
    let dir = config::dir()?;
    let mut cfg = config::load(&dir)?;
    let login = cfg.select(site)?.clone();
    let question = format!("Log out {} from {}?", login.email, login.site());
    if !prompt::confirm(&question, yes)? {
        bail!("Cancelled");
    }
    keyring::delete(&keyring::service(&login.site()), &login.email)?;
    cfg.remove(&login.site());
    config::save(&dir, &cfg)?;
    if json {
        return print_json(&json!({"logged_out": login.site(), "email": login.email}));
    }
    println!("✓ Logged out {} from {}", login.email, login.site());
    if credentials::env_present() {
        println!("  The environment still supplies a login.");
    }
    Ok(())
}

fn token(site: Option<&str>, args: TokenArgs) -> Result<()> {
    let creds = credentials::resolve(site)?;
    if args.header {
        println!("Authorization: {}", creds.header());
    } else {
        println!("{}", creds.key);
    }
    Ok(())
}
