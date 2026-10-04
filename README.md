# cftui

I made this so I could have worker stats, git bash and wrangler in the same window. Use it, or don't, whatever.

![Demo dashboard](docs/demo.png)

## Get it running

- **You'll need:** [Git for Windows](https://git-scm.com/download/win) and a terminal such as Windows Terminal. Add Node.js and Wrangler for CLI commands and live logs. Project-local Wrangler works through `npx --no-install wrangler`.
- **Download:** grab the Windows ZIP from [Releases](https://github.com/gordonspence/cftui/releases), extract it and run `cftui.exe`. No Rust or Cargo needed.
- **Build locally:** install Rust plus Windows MSVC Build Tools or MinGW. In PowerShell, `./run.ps1 --demo` runs it; `./build.ps1` puts the EXE, ZIP and checksum in `dist/release`. Git Bash: `cargo run -- --demo`.
- **Demo:** double-click the release's `demo.cmd`, or run `./cftui.exe --demo`. Stats and logs are samples; Bash and HTTP requests are real.
- **Code checks:** `cargo fmt --check`, `cargo test --locked`, `cargo clippy --all-targets --locked -- -D warnings`.

## Using it

- **Configure:** copy `cftui.example.toml` to `cftui.toml`. Set `account_id` to your 32-character Cloudflare account ID and `project_dir` to your project folder. `--config PATH`, `--project PATH` and `CFTUI_ACCOUNT_ID` override these.
- **Connect:** create an [API token](https://dash.cloudflare.com/profile/api-tokens) scoped to your account with **Account Analytics → Read**; add **Workers Scripts → Read** for deployment details. Launch from Git Bash:

  ```bash
  export CFTUI_API_TOKEN='your-token'
  ./cftui.exe
  ```

- **Wrangler:** use `wrangler login` or `CLOUDFLARE_API_TOKEN` in Bash. Its login is separate from the dashboard token.
- **Panels:** click to focus; `[+]` expands, `[-]` restores, `[x]` hides. F1–F4 toggle Workers, D1, Resources and Bash. F6 moves focus, F8 hides, F9 restores the main panels, F11 expands/restores.
- **Workers and D1:** requests/errors and rows read/written over 24 hours. In Resources, Tab switches lists, arrows select, `/` searches, `s` sorts and Enter opens Worker details. F5 refreshes.
- **Logs:** select a Worker and press `L`. `/` searches, `e` filters errors, Space pauses/resumes, `f` follows, `x` stops. Enter opens event JSON; Esc goes back.
- **HTTP:** `c` opens the GET inspector. `u` edits the URL, Ctrl+U clears while editing, Enter/`r` sends. Tab switches headers/body; arrows and PageUp/PageDown scroll. Esc finishes editing, then closes it.
- **Bash:** type normally when focused. Ctrl+C interrupts; Shift+PageUp/PageDown scrolls history. F6 returns to the dashboard. Hiding Bash keeps it running; quitting closes it.
- **Projects:** add `[[projects]]` entries as shown in the config example, restart, then switch with F7. Each keeps its own Bash session. Pass environments explicitly: `wrangler dev --env "$CFTUI_PROJECT_ENV"`.
- **Header and layouts:** project, Git status, account, environment, CLI versions and deployment details sit at the top. `1`/`2`/`3` select Overview/Debugging/Bash; `[`/`]` resize Bash. Layout and project save on exit or with F12.
- **Help:** click Help or press **Alt+H** anywhere for Setup, Keys, Workflows and Fixes. Tab/Shift+Tab change sections; arrows, PageUp/PageDown or the wheel scroll. Esc closes it and resumes your input. `?` opens Keys from the dashboard.
- **Quit:** F10 anywhere, or `q` from the dashboard. Normal typing goes to Bash while it has focus.

## Issues and limits

- **Won't start?** Check Bash and all configured project folders. Use `--shell PATH` for another Bash. Quote Windows paths with single quotes in TOML. Restart after edits; `.env` files aren't loaded. Keep tokens out of the config.
- **Missing data or logs?** Check account scope, token permissions and Wrangler login. Tails show new invocations, not old ones. Trigger a request and check filters/pause. Help's Fixes section has more.
- **Stats:** analytics may be sampled or delayed. Totals cover a rolling 24 hours; graphs show completed UTC hours. Unavailable doesn't mean zero. Metrics refresh every 60 seconds (`refresh_seconds`, minimum 15); deployments every minute. Serving doesn't mean healthy.
- **Git:** status refreshes every five seconds without fetching; ahead/behind uses your locally known upstream.
- **Limits:** logs use up to 32 MiB across live/paused views; event details stop at 32 KiB. HTTP is GET-only, shows redirects without following them, times out after 15 seconds and caps bodies at 256 KiB. Truncation is marked.
- **Checks:** `--check` prints analytics and warnings as JSON; `--check-shell` checks Bash. `--preview preview.html` exports a sample layout; `--help` lists launch options.
- **Windows for now:** macOS hasn't been tested. Minimum terminal size: 50 columns × 15 rows.

[MIT licence](LICENSE). Just messing around.
