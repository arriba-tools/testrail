mod api;
mod attachment;
mod auth;
mod case;
mod cli;
mod context;
mod credentials;
mod ids;
mod markup;
mod milestone;
mod output;
mod plan;
mod project;
mod prompt;
mod raw;
mod result;
mod run;
mod section;
mod status;
mod suite;
mod test;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Command};
use context::Ctx;

fn main() -> Result<()> {
    // Exit quietly when stdout closes early, as when piped into head, instead of panicking.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    api::init_tls();
    let cli = Cli::parse();

    // Built on first use, so commands that never reach TestRail need no login.
    let ctx = || Ctx::new(cli.site.as_deref(), cli.json, cli.yes);

    match cli.command {
        Command::Auth(cmd) => auth::run(cli.site.as_deref(), cli.json, cli.yes, cmd),
        Command::Project(cmd) => project::run(&ctx()?, cmd),
        Command::Suite(cmd) => suite::run(&ctx()?, cmd),
        Command::Section(cmd) => section::run(&ctx()?, cmd),
        Command::Case(cmd) => case::run(&ctx()?, cmd),
        Command::Milestone(cmd) => milestone::run(&ctx()?, cmd),
        Command::Plan(cmd) => plan::run(&ctx()?, cmd),
        Command::Run(cmd) => run::run(&ctx()?, cmd),
        Command::Test(cmd) => test::run(&ctx()?, cmd),
        Command::Result(cmd) => result::run(&ctx()?, cmd),
        Command::Attachment(cmd) => attachment::run(&ctx()?, cmd),
        Command::Status(cmd) => status::run(&ctx()?, cmd),
        Command::Api(args) => raw::run(&ctx()?, args),
    }
}
