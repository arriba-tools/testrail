use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "testrail-arriba",
    about = "TestRail workflows for people and coding agents",
    version,
    after_help = OBJECTS
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// TestRail site, as in acme.testrail.io, when several logins are stored
    #[arg(long, global = true, value_name = "SITE")]
    pub site: Option<String>,

    /// Print machine-readable JSON
    #[arg(long, global = true)]
    pub json: bool,

    /// Skip confirmation prompts
    #[arg(short, long, global = true)]
    pub yes: bool,
}

/// How TestRail's objects relate, after the top-level help since that is what an agent reads first.
const OBJECTS: &str = "\
How TestRail fits together:
  Project                              --project takes its ID or name
  ├── Suite → Section → Case (C39)     the test and its steps: case view
  ├── Milestone
  ├── Plan → Run (R201)                a plan's runs: run list --plan
  └── Run (R201)                       runs outside plans: run list -p
        └── Test (T1001)               one case in either kind of run: test view
              └── Result               added with result add, never edited

IDs take these prefixes or the page's URL.";

#[derive(Subcommand)]
pub enum Command {
    /// Show the login in use, log in, log out, or print the API key
    #[command(subcommand)]
    Auth(AuthCommand),
    /// List and view projects
    #[command(subcommand)]
    Project(ProjectCommand),
    /// List a project's suites
    #[command(subcommand)]
    Suite(SuiteCommand),
    /// List a project's sections, as a tree with --tree
    #[command(subcommand)]
    Section(SectionCommand),
    /// List test cases, and view one with its steps
    #[command(subcommand)]
    Case(CaseCommand),
    /// List and view milestones
    #[command(subcommand)]
    Milestone(MilestoneCommand),
    /// List and view test plans with their runs
    #[command(subcommand)]
    Plan(PlanCommand),
    /// List and view test runs, including the runs inside a plan
    #[command(subcommand)]
    Run(RunCommand),
    /// List the tests in a run, and view one with its steps and results
    #[command(subcommand)]
    Test(TestCommand),
    /// List results, or add one with per-step outcomes and inline screenshots
    #[command(subcommand)]
    Result(ResultCommand),
    /// List, add, download, and delete attachments
    #[command(subcommand)]
    Attachment(AttachmentCommand),
    /// List the result statuses, custom ones included
    #[command(subcommand)]
    Status(StatusCommand),
    /// Call any API endpoint with the same login, such as get_case_fields
    Api(ApiArgs),
}

#[derive(Subcommand)]
pub enum AuthCommand {
    /// Show which login is in use and who it belongs to
    Status,
    /// Store a login for a site, prompting for what the flags leave out
    Login(LoginArgs),
    /// Remove a stored login
    Logout,
    /// Print the API key, or the whole Authorization header with --header
    Token(TokenArgs),
}

#[derive(Args)]
pub struct LoginArgs {
    /// Email of the TestRail account
    #[arg(short, long)]
    pub email: Option<String>,

    /// Read the API key from stdin instead of prompting for it, with --email and --site
    #[arg(long, requires = "email")]
    pub token: bool,
}

#[derive(Args)]
pub struct TokenArgs {
    /// Print as an Authorization header, ready for curl -H
    #[arg(long)]
    pub header: bool,
}

/// Filters on whether a project, milestone, plan, or run is completed.
#[derive(Args)]
pub struct StateArgs {
    /// Only ones still open
    #[arg(long, conflicts_with = "completed")]
    pub active: bool,

    /// Only completed ones
    #[arg(long)]
    pub completed: bool,
}

impl StateArgs {
    /// The `is_completed` filter TestRail takes, or None for both.
    pub fn is_completed(&self) -> Option<bool> {
        match (self.active, self.completed) {
            (true, _) => Some(false),
            (_, true) => Some(true),
            _ => None,
        }
    }
}

#[derive(Args)]
pub struct LimitArgs {
    /// Most to return
    #[arg(short, long, default_value_t = 50, conflicts_with = "all")]
    pub limit: usize,

    /// Return every one
    #[arg(long)]
    pub all: bool,
}

impl LimitArgs {
    pub fn get(&self) -> Option<usize> {
        if self.all { None } else { Some(self.limit) }
    }
}

#[derive(Args)]
pub struct ProjectArg {
    /// Project ID, name, or URL
    #[arg(short, long, value_name = "PROJECT")]
    pub project: String,
}

#[derive(Subcommand)]
pub enum ProjectCommand {
    /// List projects
    List(StateArgs),
    /// Show a project
    View(ProjectViewArgs),
}

#[derive(Args)]
pub struct ProjectViewArgs {
    /// Project ID, name, or URL
    pub project: String,
}

#[derive(Subcommand)]
pub enum SuiteCommand {
    /// List a project's suites
    List(ProjectArg),
}

#[derive(Subcommand)]
pub enum SectionCommand {
    /// List a project's sections
    List(SectionListArgs),
}

#[derive(Args)]
pub struct SectionListArgs {
    #[command(flatten)]
    pub project: ProjectArg,

    /// Suite ID or URL, required when the project has several suites
    #[arg(short, long)]
    pub suite: Option<String>,

    /// Indent sections under their parents
    #[arg(long)]
    pub tree: bool,
}

#[derive(Subcommand)]
pub enum CaseCommand {
    /// List test cases
    List(CaseListArgs),
    /// Show a case with its preconditions and steps
    View(CaseViewArgs),
}

#[derive(Args)]
pub struct CaseListArgs {
    #[command(flatten)]
    pub project: ProjectArg,

    /// Suite ID or URL, required when the project has several suites
    #[arg(short, long)]
    pub suite: Option<String>,

    /// Only cases in this section
    #[arg(long, value_name = "SECTION")]
    pub section: Option<String>,

    /// Only cases whose title contains this text
    #[arg(short, long, value_name = "TEXT")]
    pub filter: Option<String>,

    /// Only cases with this reference, such as a Jira key
    #[arg(long = "ref", value_name = "REF")]
    pub reference: Option<String>,

    /// Only cases whose title, preconditions, steps, or custom text have a word starting with this
    /// text, ignoring case, which reads every case in the project
    #[arg(short, long, value_name = "TEXT")]
    pub grep: Option<String>,

    #[command(flatten)]
    pub limit: LimitArgs,
}

#[derive(Args)]
pub struct CaseViewArgs {
    /// Case ID, such as C39, or its URL
    pub case: String,
}

#[derive(Subcommand)]
pub enum MilestoneCommand {
    /// List a project's milestones
    List(MilestoneListArgs),
    /// Show a milestone
    View(MilestoneViewArgs),
}

#[derive(Args)]
pub struct MilestoneListArgs {
    #[command(flatten)]
    pub project: ProjectArg,

    #[command(flatten)]
    pub state: StateArgs,
}

#[derive(Args)]
pub struct MilestoneViewArgs {
    /// Milestone ID or URL
    pub milestone: String,
}

#[derive(Subcommand)]
pub enum PlanCommand {
    /// List a project's test plans, newest first
    List(PlanListArgs),
    /// Show a plan with its entries and runs
    View(PlanViewArgs),
}

#[derive(Args)]
pub struct PlanListArgs {
    #[command(flatten)]
    pub project: ProjectArg,

    /// Only plans whose name contains this text, ignoring case
    #[arg(short, long, value_name = "TEXT")]
    pub name: Option<String>,

    /// Only plans in this milestone
    #[arg(short, long, value_name = "MILESTONE")]
    pub milestone: Option<String>,

    #[command(flatten)]
    pub state: StateArgs,

    #[command(flatten)]
    pub limit: LimitArgs,
}

#[derive(Args)]
pub struct PlanViewArgs {
    /// Plan ID or URL
    pub plan: String,
}

#[derive(Subcommand)]
pub enum RunCommand {
    /// List a project's runs outside plans, or the runs inside a plan with --plan
    List(RunListArgs),
    /// Show a run with its progress
    View(RunViewArgs),
}

#[derive(Args)]
#[command(group = clap::ArgGroup::new("scope").required(true).args(["project", "plan"]))]
pub struct RunListArgs {
    /// Project ID, name, or URL, for the runs outside plans
    #[arg(short, long, value_name = "PROJECT")]
    pub project: Option<String>,

    /// Plan ID or URL, for the runs inside it
    #[arg(long, value_name = "PLAN")]
    pub plan: Option<String>,

    #[command(flatten)]
    pub state: StateArgs,

    #[command(flatten)]
    pub limit: LimitArgs,
}

#[derive(Args)]
pub struct RunViewArgs {
    /// Run ID, such as R201, or its URL
    pub run: String,
}

#[derive(Subcommand)]
pub enum TestCommand {
    /// List the tests in a run
    List(TestListArgs),
    /// Show a test with its steps and latest results
    View(TestViewArgs),
}

#[derive(Args)]
#[command(group = clap::ArgGroup::new("scope").required(true).args(["run", "plan"]))]
pub struct TestListArgs {
    /// Run ID, such as R201, or its URL
    pub run: Option<String>,

    /// Plan ID or URL, for the tests in every run inside it
    #[arg(long, value_name = "PLAN")]
    pub plan: Option<String>,

    /// Only tests of this case, such as C52
    #[arg(short, long, value_name = "CASE")]
    pub case: Option<String>,

    /// Only tests with this status, by name or ID, repeatable or comma-separated
    #[arg(short, long, value_name = "STATUS", value_delimiter = ',')]
    pub status: Vec<String>,

    #[command(flatten)]
    pub limit: LimitArgs,
}

#[derive(Args)]
pub struct TestViewArgs {
    /// Test ID, such as T1001, or its URL
    pub test: String,

    /// Most results to show
    #[arg(short, long, default_value_t = 5)]
    pub results: usize,
}

#[derive(Subcommand)]
pub enum ResultCommand {
    /// List the results of a test, of a case in a run, or of a whole run
    List(ResultListArgs),
    /// Add a result, with per-step outcomes whose screenshots show inline
    Add(Box<ResultAddArgs>),
}

#[derive(Args)]
#[command(group = clap::ArgGroup::new("target").required(true).args(["test", "run"]))]
pub struct ResultListArgs {
    /// Test ID, such as T1001, or its URL
    #[arg(short, long, conflicts_with_all = ["run", "case"])]
    pub test: Option<String>,

    /// Run ID or URL, for the whole run or, with --case, one case in it
    #[arg(short, long)]
    pub run: Option<String>,

    /// Case ID, such as C39, with --run
    #[arg(short, long, requires = "run")]
    pub case: Option<String>,

    #[command(flatten)]
    pub limit: LimitArgs,
}

#[derive(Args)]
#[command(group = clap::ArgGroup::new("target").required(true).args(["test", "run"]))]
pub struct ResultAddArgs {
    /// Test ID, such as T1001, or its URL, instead of --run and --case
    #[arg(short, long, conflicts_with_all = ["run", "case"])]
    pub test: Option<String>,

    /// Run ID or URL, with --case
    #[arg(short, long, requires = "case")]
    pub run: Option<String>,

    /// Case ID, such as C39, or its URL, with --run
    #[arg(short, long, requires = "run")]
    pub case: Option<String>,

    /// Status by name or ID, such as passed, failed, blocked, or retest
    #[arg(short, long)]
    pub status: String,

    /// Comment as Markdown or HTML
    #[arg(long, conflicts_with = "comment_file")]
    pub comment: Option<String>,

    /// Read the comment from a file
    #[arg(long, value_name = "FILE")]
    pub comment_file: Option<PathBuf>,

    /// Time spent, such as 1m 30s
    #[arg(short, long)]
    pub elapsed: Option<String>,

    /// Build or version tested
    #[arg(long)]
    pub version: Option<String>,

    /// Defect ID, repeatable or comma-separated
    #[arg(short, long = "defect", value_name = "ID", value_delimiter = ',')]
    pub defects: Vec<String>,

    /// Per-step outcomes as a JSON file, or - for stdin
    #[arg(long, value_name = "FILE")]
    pub steps: Option<PathBuf>,

    /// Add a screenshot to a step's actual result as N=FILE, repeatable
    #[arg(long = "step-image", value_name = "N=FILE", requires = "steps")]
    pub step_images: Vec<String>,

    /// File to attach to the result once it is added, repeatable
    #[arg(short, long, value_name = "FILE")]
    pub attach: Vec<PathBuf>,

    /// Custom result field as NAME=VALUE, repeatable, with VALUE read as JSON when it parses
    #[arg(long = "field", value_name = "NAME=VALUE")]
    pub fields: Vec<String>,

    /// Send text as given instead of converting Markdown to HTML and uploading local images
    #[arg(long)]
    pub raw: bool,

    /// Print the result that would be added, without uploading or adding anything
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Subcommand)]
pub enum AttachmentCommand {
    /// List the attachments of a case, run, test, or plan
    List(AttachmentListArgs),
    /// Attach files to a run, result, case, or plan, printing the IDs for inline images
    #[command(alias = "upload")]
    Add(AttachmentAddArgs),
    /// Download attachments by ID, or every one on a case, run, test, or plan
    Download(AttachmentDownloadArgs),
    /// Delete attachments by ID
    Delete(AttachmentDeleteArgs),
}

/// What holds the attachments to read.
#[derive(Args)]
#[group(multiple = false)]
pub struct HolderArgs {
    /// Case ID, such as C39, or its URL
    #[arg(long, value_name = "CASE")]
    pub case: Option<String>,

    /// Run ID, such as R201, or its URL
    #[arg(long, value_name = "RUN")]
    pub run: Option<String>,

    /// Test ID, such as T1001, or its URL
    #[arg(long, value_name = "TEST")]
    pub test: Option<String>,

    /// Plan ID or URL
    #[arg(long, value_name = "PLAN")]
    pub plan: Option<String>,
}

#[derive(Args)]
#[command(group = clap::ArgGroup::new("holder").required(true).args(["case", "run", "test", "plan"]))]
pub struct AttachmentListArgs {
    #[command(flatten)]
    pub holder: HolderArgs,
}

#[derive(Args)]
#[command(group = clap::ArgGroup::new("target").required(true).args(["run", "result", "case", "plan"]))]
pub struct AttachmentAddArgs {
    /// Files to attach
    #[arg(required = true, value_name = "FILE")]
    pub files: Vec<PathBuf>,

    /// Run ID or URL, the usual home for screenshots shown inline in step results
    #[arg(long, value_name = "RUN")]
    pub run: Option<String>,

    /// Result ID
    #[arg(long, value_name = "RESULT")]
    pub result: Option<String>,

    /// Case ID or URL
    #[arg(long, value_name = "CASE")]
    pub case: Option<String>,

    /// Plan ID or URL
    #[arg(long, value_name = "PLAN")]
    pub plan: Option<String>,
}

#[derive(Args)]
pub struct AttachmentDownloadArgs {
    /// Attachment IDs, as `attachment list` shows them
    #[arg(value_name = "ID")]
    pub ids: Vec<String>,

    #[command(flatten)]
    pub holder: HolderArgs,

    /// Directory to download into (default: a testrail-arriba folder in the temp directory)
    #[arg(short = 'o', long, value_name = "DIR")]
    pub dir: Option<PathBuf>,
}

#[derive(Args)]
pub struct AttachmentDeleteArgs {
    /// Attachment IDs, as `attachment list` shows them
    #[arg(required = true, value_name = "ID")]
    pub ids: Vec<String>,
}

#[derive(Subcommand)]
pub enum StatusCommand {
    /// List the result statuses with their IDs
    List,
}

#[derive(Args)]
pub struct ApiArgs {
    /// Endpoint, such as get_case/39 or get_cases/2&suite_id=1
    pub endpoint: String,

    /// HTTP method (default: POST with --data, GET otherwise)
    #[arg(short = 'X', long, value_name = "METHOD")]
    pub method: Option<String>,

    /// JSON body, @FILE to read one, or - for stdin
    #[arg(short, long, value_name = "JSON")]
    pub data: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_well_formed() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn state_maps_to_is_completed() {
        let state = |active, completed| StateArgs { active, completed };
        assert_eq!(state(true, false).is_completed(), Some(false));
        assert_eq!(state(false, true).is_completed(), Some(true));
        assert_eq!(state(false, false).is_completed(), None);
    }

    #[test]
    fn result_add_needs_a_test_or_a_run_and_case() {
        let parse = |args: &[&str]| {
            let mut full = vec!["testrail-arriba", "result", "add", "-s", "passed"];
            full.extend_from_slice(args);
            Cli::try_parse_from(full)
        };
        assert!(parse(&["--test", "T1"]).is_ok());
        assert!(parse(&["--run", "R1", "--case", "C1"]).is_ok());
        assert!(parse(&["--run", "R1"]).is_err());
        assert!(parse(&[]).is_err());
        assert!(parse(&["--test", "T1", "--run", "R1", "--case", "C1"]).is_err());
        assert!(parse(&["--test", "T1", "--step-image", "1=a.png"]).is_err());
    }

    #[test]
    fn test_list_takes_a_run_or_a_plan() {
        let parse = |args: &[&str]| {
            let mut full = vec!["testrail-arriba", "test", "list"];
            full.extend_from_slice(args);
            Cli::try_parse_from(full)
        };
        assert!(parse(&["R92"]).is_ok());
        assert!(parse(&["--plan", "91", "--case", "C52"]).is_ok());
        assert!(parse(&["R92", "--plan", "91"]).is_err());
        assert!(parse(&["--case", "C52"]).is_err());
    }

    #[test]
    fn help_ends_with_the_object_model() {
        use clap::CommandFactory;
        let help = Cli::command().render_help().to_string();
        assert!(help.contains("How TestRail fits together:"), "{help}");
    }
}
