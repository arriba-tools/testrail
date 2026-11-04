# testrail-arriba

[![Build](https://github.com/nmoinvaz/testrail-arriba/actions/workflows/build.yml/badge.svg)](https://github.com/nmoinvaz/testrail-arriba/actions/workflows/build.yml)
[![Lint](https://github.com/nmoinvaz/testrail-arriba/actions/workflows/lint.yml/badge.svg)](https://github.com/nmoinvaz/testrail-arriba/actions/workflows/lint.yml)
[![codecov](https://codecov.io/gh/nmoinvaz/testrail-arriba/graph/badge.svg)](https://codecov.io/gh/nmoinvaz/testrail-arriba)

A TestRail CLI for the manual test workflows [trcli](https://github.com/gurock/trcli) doesn't cover:
adding one result with per-step outcomes, screenshots that show inline in those steps, attachments on
runs, and browsing plans, runs, and tests by the IDs TestRail shows. Step results are written in
Markdown and sent as the HTML TestRail renders.

Every command takes `--json`, and every prompt has a flag that answers it, so scripts and coding
agents can drive the same workflows a person runs by hand.

## Install

Download a binary from the [releases](https://github.com/nmoinvaz/testrail-arriba/releases) and put
it on your `PATH`, or build it from source:

```
cargo install --git https://github.com/nmoinvaz/testrail-arriba
```

## Use with Claude

Add this line to `~/.claude/CLAUDE.md` so Claude Code reaches for `testrail-arriba` when a task
touches TestRail:

```
* Use `testrail-arriba` when interacting with TestRail, for plans, runs, tests, cases, attachments, and results with per-step outcomes and inline screenshots. Run `testrail-arriba --help` for the commands.
```

## Authentication

The login comes from the environment when it is set, and from the one `auth login` stored otherwise.

| Variables | Login |
|-----------|-------|
| `TESTRAIL_URL`, `TESTRAIL_EMAIL`, `TESTRAIL_API_KEY` | An API key from My Settings > API Keys, sent as basic auth to the site |
| `TESTRAIL_PASSWORD` | Accepted in place of `TESTRAIL_API_KEY`, as the testrail-api Python package reads it |
| none | The login `auth login` stored, with its URL and email in `~/.config/testrail-arriba/config.yml` and its key in the platform keyring |

`auth login` checks the key against the site before storing it. The keyring is reached through
`security` on macOS, `secret-tool` on Linux, and the Windows credential store. Pass `--site` to pick
one of several stored logins.

```
testrail-arriba auth login --site acme.testrail.io --email me@acme.com
op read "op://Private/TestRail/credential" | testrail-arriba auth login --site acme.testrail.io --email me@acme.com --token
testrail-arriba auth status
testrail-arriba auth token --header     # Authorization: Basic ..., for curl -H
testrail-arriba auth logout
```

## Usage

```
testrail-arriba [options] <command>
```

Wherever a command takes an ID, it also takes TestRail's prefixed form, such as `C39`, `R201`, or
`T1001`, and the URL of the page. `--project` also takes the project's name.

### Browsing

| Command | Description |
|---------|-------------|
| `project list` | List projects, `--active` or `--completed` |
| `project view <PROJECT>` | Show a project |
| `suite list -p <PROJECT>` | List a project's suites |
| `section list -p <PROJECT>` | List sections, `--tree` to indent them under their parents |
| `case list -p <PROJECT>` | List cases, by `--suite`, `--section`, or title `--filter` |
| `case view <CASE>` | Show a case with its preconditions and steps |
| `milestone list -p <PROJECT>` | List milestones with their sub-milestones |
| `milestone view <MILESTONE>` | Show a milestone |
| `plan list -p <PROJECT>` | List plans, newest first, by `--name` or `--milestone` |
| `plan view <PLAN>` | Show a plan with every run in its entries |
| `run list -p <PROJECT>` | List the runs outside plans |
| `run list --plan <PLAN>` | List the runs inside a plan, which TestRail's run list leaves out |
| `run view <RUN>` | Show a run with its progress |
| `test list <RUN>` | List a run's tests, `--status failed,retest` to pick some |
| `test view <TEST>` | Show a test with its steps and latest results |
| `status list` | List result statuses with their IDs, custom ones included |

Case, plan, run, test, and result lists stop at 50 unless `--limit` or `--all` says otherwise, and
say so on stderr when more remain.

### Results

| Command | Description |
|---------|-------------|
| `result list --test <TEST>` | List a test's results, or `--run` alone or with `--case` |
| `result add --test <TEST> -s <STATUS>` | Add a result, or with `--run` and `--case` in place of `--test` |

`result add` takes `--comment` or `--comment-file`, `--elapsed`, `--version`, `--defect`, and
`--field NAME=VALUE` for custom fields. `--steps` reads per-step outcomes, `--attach` adds files to
the result once it exists, and `--dry-run` prints what would be sent without sending anything.

### Attachments

| Command | Description |
|---------|-------------|
| `attachment list --run <RUN>` | List attachments, or `--case`, `--test`, or `--plan` |
| `attachment add <FILE>... --run <RUN>` | Attach files, or to `--result`, `--case`, or `--plan`, printing each ID and its inline `src` |
| `attachment download <ID>...` | Download attachments, or every one with `--run`, `--case`, `--test`, or `--plan` |
| `attachment delete <ID>...` | Delete attachments, after confirming |

Downloads go to a `testrail-arriba` folder in the temp directory unless `-o` names one.

### API

| Command | Description |
|---------|-------------|
| `api <ENDPOINT>` | Call any endpoint, such as `get_case_fields` or `get_cases/2&suite_id=1`, `-d` to POST JSON |

### Options

| Option | Description |
|--------|-------------|
| `--site <SITE>` | TestRail site, as in `acme.testrail.io`, when several logins are stored |
| `--json` | Print machine-readable JSON |
| `-y`, `--yes` | Skip confirmation prompts |
| `-h`, `--help` | Print help for any command |
| `-V`, `--version` | Print version information |

## Step results

TestRail renders a step result's text as HTML only when it carries `<p>` tags, ignores Markdown image
syntax there, and cannot edit a result once it is added. So `result add --steps` takes Markdown,
uploads the local images it names to the run, writes their IDs into the HTML, and adds the result
once.

The steps file is a JSON array with one object per step, in the case's order.

```json
[
  {"status": "passed", "actual": "The page loaded with both fields.\n\n![](shots/step1.png)"},
  {"status": "failed", "actual": "Nothing happened after **Submit**.\n\n![](shots/step2.png)"}
]
```

| Field | Sent as |
|-------|---------|
| `content` | HTML paragraphs, or the case's step when left out |
| `expected` | HTML in a `<blockquote>`, or the case's expected result when left out |
| `actual` | HTML paragraphs, with images lifted out after them as `<img>` tags |
| `status` or `status_id` | A status name, as `status list` shows them, or its ID |

Image paths resolve against the steps file's folder, and each file is uploaded once however often it
appears. `--step-image 2=shot.png` adds a screenshot to a step without editing the file. Text that
already starts with an HTML tag is sent as it is, and `--raw` sends every field as given.

## Examples

Find a plan's runs and what is left to test in one of them:

```
testrail-arriba plan list -p Acme --active --name "2.1"
testrail-arriba run list --plan 15
testrail-arriba test list R201 --status untested,retest
```

Read a test's steps, run them, then record the outcome with a screenshot per step:

```
testrail-arriba test view T1001
testrail-arriba result add --test T1001 -s failed -e "4m" --version 2.1.0 \
    --comment "Fails at step 2." --steps steps.json --attach console.log
```

Check what would be sent before sending it:

```
testrail-arriba result add --test T1001 -s passed --steps steps.json --dry-run
```

Call the REST API yourself with the same login:

```
curl -H "$(testrail-arriba auth token --header)" -H "Content-Type: application/json" \
    "https://acme.testrail.io/index.php?/api/v2/get_case_fields"
```

## Development

```
cargo build --release
cargo test
```

## License

MIT
