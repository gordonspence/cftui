# cftui

I made this so I could have worker stats, git bash and wrangler in the same window. Use it, or don't, whatever.

`cftui` is a Windows terminal dashboard for Cloudflare Workers and D1 metrics, with an embedded Git Bash session, live Worker logs through Wrangler, and a quick HTTP GET inspector. This is an early Windows release; macOS has not been tested.

![Demo dashboard with sample Workers and D1 metrics](docs/demo.png)

The image shows sample data and an illustrative shell prompt. Running the app opens a real shell. [MIT licensed](LICENSE).

## Try it

### Windows download

When a release is available, download `cftui-v0.1.0-windows-x64.zip` from this repository's **Releases** page, extract it, and double-click `demo.cmd`. You can also run `./cftui.exe --demo` from Git Bash in the extracted folder. The ZIP includes the executable, example configuration, README, license, security policy, and demo image. Rust is not needed to run it. Until then, use the source build below.

You need [Git for Windows](https://git-scm.com/download/win) for the embedded Bash pane, and a VT-capable terminal such as Windows Terminal. Node.js and Wrangler are only needed for Wrangler commands and live Worker logs; the dashboard and HTTP inspector run without them. Demo mode needs no Cloudflare account, but its Bash pane is real and the HTTP inspector sends real requests if you use it.

To connect an account, follow [Connect your account](#connect-your-account) below. The executable uses `cftui.toml` in your current directory unless you pass `--config PATH`. You can use `--project PATH` to choose the shell's starting folder.

`demo.cmd` pauses if startup fails. Run `./cftui.exe --help` in a terminal for all command-line options.

### Build from source

Install Rust and the matching Windows MSVC Build Tools or MinGW toolchain, plus Git for Windows. From PowerShell in this folder:

```powershell
.\run.ps1 --demo
.\build.ps1
```

The first build downloads Cargo dependencies. `build.ps1` writes a fresh versioned ZIP and SHA-256 file under `dist/release/`. The source checkout may also use a workspace-local toolchain under ignored `.tools/`; that folder is not part of the repository or release.

Git Bash users with Rust and a linker on PATH can use `cargo run -- --demo` or `bash run.sh --demo`.

## Connect your account

Create a dedicated token in [Cloudflare's API token settings](https://dash.cloudflare.com/profile/api-tokens) with **Account → Account Analytics → Read**, scoped to your account. See [Cloudflare analytics token documentation](https://developers.cloudflare.com/analytics/graphql-api/getting-started/authentication/api-token-auth/). Dataset access can depend on your plan and token permissions. The dashboard uses this token only for read-only analytics queries. The embedded Bash/Wrangler pane can run whatever commands you enter, including deployments; the HTTP inspector sends GET requests to URLs you enter.

In the extracted ZIP, copy `cftui.example.toml` to `cftui.toml`, replace the placeholder `account_id` and `project_dir`, then set the token in the terminal where you launch the app:

```powershell
Copy-Item .\cftui.example.toml .\cftui.toml
notepad .\cftui.toml
$env:CFTUI_API_TOKEN = 'your-token'
.\cftui.exe
```

Or from Git Bash:

```bash
export CFTUI_API_TOKEN='your-token'
./cftui.exe
```

Do not paste your token into chat or commit it. `cftui.toml` and `.env` are ignored; `.env` is not automatically loaded. You can also use `CFTUI_ACCOUNT_ID` instead of the configuration field. Tokens are not written to disk or printed, and the dashboard token is removed from the embedded Bash environment. Wrangler uses its own existing login or `CLOUDFLARE_API_TOKEN` environment variable.

```powershell
.\cftui.exe --demo --project C:\path\to\your\project
.\cftui.exe --demo --shell 'C:\Program Files\Git\bin\bash.exe'
.\cftui.exe --check       # JSON analytics / nonzero exit on unavailable datasets
.\cftui.exe --check-shell # local Git Bash PTY smoke check; no Cloudflare requests
```

Inside the Bash pane, use your usual workflow:

```bash
pwd
node --version
wrangler --version
# If Wrangler is project-local:
npx --no-install wrangler --version
```

## Controls

| Key | Action |
| --- | --- |
| F1 / F2 / F3 / F4 | Toggle Workers / D1 / Resources / Bash panels |
| F6 | Focus the next visible panel |
| F7 | Open the project picker; Enter switches, Esc cancels |
| F12 | Save the current layout and project |
| F8 | Hide the focused panel |
| F9 | Restore all panels and the normal layout |
| F5 | Refresh analytics |
| F11 | Expand/restore the focused panel |
| F10 | Exit and terminate the embedded shell |
| Tab / Left / Right | Switch Workers/D1 view, with dashboard focused |
| Up / Down | Select a resource or log entry, with dashboard focused |
| / | Search resources or logs; Enter accepts, Esc clears |
| s | Cycle name, activity, errors/writes, and rate sorting |
| Enter | Worker details; expand/collapse a selected log event |
| L | Start/restart logs for the selected Worker |
| e / Space / f / x | Logs: errors only / pause / follow / stop |
| Esc | Close log details or return to the resource list |
| 1 / 2 / 3 | Overview / Debugging / Shell layouts, outside Bash |
| [ / ] | Decrease/increase Bash's share of the screen |
| ? | Show keyboard help, outside Bash |
| c | Open/expand the HTTP request inspector, outside Bash |
| Shift+PageUp / Shift+PageDown | Shell scrollback, with shell focused |
| Ctrl+C | Interrupt shell command, with shell focused |
| q | Exit, with dashboard focused |

Click a panel to focus it or a resource/log row to select it. Click `[+]` to expand, `[-]` to restore, or `[x]` to hide it. Remaining panels use the freed space. Hidden panels can be reopened with F1–F4; hiding Bash keeps its shell session and commands running. Layouts and the last project save on exit to `cftui.state.toml` beside the selected config file; F12 saves immediately. The state file contains no credentials, logs, or shell history.

Tab, Enter, arrows, Ctrl keys, Alt-character keys, Unicode, and pasted text are forwarded while the shell is focused. F1–F12 are reserved by the app. Search and the project picker own input while open. Paste follows the shell application's bracketed-paste setting. Exiting the app stops all project shells and the log tail; stop important foreground jobs first. The initial shell directory is configurable, and you can `cd` normally. An exited shell is labelled; restart cftui to reopen it.

## Debug a Worker

Use `/` to find a Worker, arrows or a click to select it, and Enter for its rolling totals, error rate, and hourly graphs. These graphs use that Worker's data, not the account totals. Press L for logs. Use `e` for errors, `/` to search event content, Space to freeze/resume the view, and Enter to inspect an event's full JSON. Arrow keys scroll expanded event details. `f` returns to following new events; `x` stops the tail. Esc returns to the resource list; the tail continues until stopped, changed, or the app exits.

Logs are a separate Wrangler process using your existing Wrangler login or `CLOUDFLARE_API_TOKEN`. The analytics token is not passed to it. Wrangler must be on Bash's PATH, or installed locally so `npx --no-install wrangler` works. Live tails show new invocations, not historical logs, and may be sampled. The exact deployed name from the list is used, without appending the project environment. The app uses a temporary, credential-free Wrangler configuration containing the dashboard account ID, so a project config cannot silently change the target account. Authentication/permission failures appear in the log pane. Demo mode uses sample log events and starts no remote tail.

The live buffer holds 500 entries. Pausing freezes the displayed buffer while the bounded live buffer continues; older entries can roll out. Events over 1 MiB, physical lines over 64 KiB, and events dropped during bursts are counted. cftui keeps its log buffer in memory; Wrangler retains its usual diagnostic logging.

## HTTP request inspector

Press `c` with dashboard focus to open the request panel. Enter a complete HTTP or HTTPS URL and press Enter to send a GET. Requests run in the background. **Requests are real even in demo mode.** The inspector does not inherit the dashboard or Wrangler API token.

The response shows status, time to response headers, total elapsed time, and captured body bytes. Tab switches headers/body. Cloudflare, cache, redirect, and CORS headers are highlighted. JSON bodies are formatted; binary/non-UTF-8 bodies show a small hex preview. 4xx/5xx responses keep their headers and bodies. Redirects are not followed, so you can inspect their status and Location header. This is an HTTP inspector rather than a browser; it does not enforce browser CORS rules or run JavaScript.

`u` edits the URL, Ctrl+U clears it while editing, Enter or `r` sends/repeats, arrows/PageUp/PageDown scroll, and Esc closes the panel (or finishes editing). `[+]`/`[-]` expands/restores it. Requests time out after 15 seconds, with a 5-second connection timeout; at most 256 KiB of body is captured, with truncation/read errors labelled. TLS certificate validation stays enabled. Only one request can be in flight. Headers, cookies, URLs, and bodies are not saved; only panel visibility/layout is persisted. Custom methods, request headers/bodies, saved requests, and copying curl commands are future additions.

## Saved projects

Add projects to `cftui.toml` (the configured `project_dir` remains the Default project):

```toml
[[projects]]
name = "My app"
path = 'C:\path\to\your\app'
environment = "production"
```

F7 opens the picker. Switching creates a separate Bash session in that folder on first use; switching back preserves its directory, variables, and running commands. A project switch stops the previous log tail to avoid mixing project contexts. All project sessions continue draining output in the background. The environment appears in the Bash title and is available as `$CFTUI_PROJECT_ENV`; normal Wrangler commands still need their explicit `--env` option, for example `wrangler dev --env "$CFTUI_PROJECT_ENV"`. An empty environment means the project's default.

## Metrics and limitations

- Rolling 24-hour Workers requests/errors, grouped by script name.
- Rolling 24-hour D1 rows read/written, grouped by database ID.
- Yellow/purple activity graphs for requests/errors and reads/writes, covering the last 24 **completed UTC hours**. Graphs and rolling totals have different window boundaries; captions show hourly peaks. Each trace uses its own scale, so bar heights are not comparable across metrics.
- Charcoal panels, cyan focus borders, aligned resource tables, and a compact keyboard strip. Graphs use font-safe coloured cell bars with hourly gaps where space allows. At 80×24, graphs compress to ASCII intensity traces; below that, the compact totals/table layout keeps Bash visible. Expanded panels show UTC time labels. Taller histograms round positive buckets up to the next cell; compact traces use relative intensity from . (zero) to @ (peak).
- Automatic refresh every 60 seconds by default; configuration minimum is 15 seconds.
- Independent availability for each dataset: denied/missing/error results show **unavailable**, not zero. Errors include a permissions hint. Background refresh never blocks shell input.
- Adaptive Cloudflare analytics may be sampled and delayed. These are activity metrics, not an exact invoice or billing forecast.
- The app refuses to display totals when its 10,000-resource result limit is reached.
- History queries are independent of totals. Unavailable history shows a label instead of a graph; a successful query with no events produces zero buckets. A 10,000-row history response is rejected as potentially incomplete.
- CPU percentiles, friendly D1 names, R2 inventory, account switching, and persistent configuration UI are not in this first version.
- Terminal emulation uses tui-term/vt100. Ordinary shell/CLI interaction is supported; full parity with a standalone terminal emulator, clipboard integration, mouse reporting, and every advanced terminal protocol is not promised.
- Account analytics and your actual Wrangler authentication/deploy workflow need live verification with your credentials. No credentials are needed for demo mode or the shell smoke check.

Sources: [Workers query](https://developers.cloudflare.com/analytics/graphql-api/tutorials/querying-workers-metrics/), [D1 analytics](https://developers.cloudflare.com/d1/observability/metrics-analytics/).

## Development

```powershell
# Set the local toolchain paths as in run.ps1, or use your normal Rust install.
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Export a standalone HTML preview directly from the TUI's cell renderer (sample data, no shell or network calls):

```bash
bash run.sh --preview preview.html
```

The preview uses your browser's monospace font; the live TUI uses your terminal's font. Generated previews are ignored by Git.

## Project status

Windows is the supported platform for this alpha. macOS code paths have not been built or tested on a Mac. The tests and CI use demo data and local HTTP fixtures; Workers/D1 analytics and Wrangler logs still need a real-account smoke test before a release is described as live-verified. Contributions and reproducible bug reports are welcome. Please remove tokens, account identifiers, request headers, and log content from public issues.

The GitHub Actions workflow runs formatting, tests, Clippy, and a Windows release build. Its uploaded ZIP is a build artifact for review; publishing a GitHub Release is a separate step.
