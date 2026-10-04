use ratatui::{style::Style, text::Line};

pub const TOPICS: [&str; 4] = ["Setup", "Keys", "Workflows", "Fixes"];

#[derive(Default)]
pub struct Guide {
    pub topic: usize,
    pub scroll: u16,
    pub max_scroll: u16,
    pub page_rows: u16,
}

impl Guide {
    pub fn shortcuts() -> Self {
        Self {
            topic: 1,
            ..Self::default()
        }
    }

    pub fn select(&mut self, topic: usize) {
        self.topic = topic % TOPICS.len();
        self.scroll = 0;
        self.max_scroll = 0;
    }

    pub fn scroll_by(&mut self, rows: i32) {
        self.scroll = (i32::from(self.scroll) + rows).clamp(0, i32::from(self.max_scroll)) as u16;
    }

    pub fn fit(&mut self, lines: usize, rows: u16) {
        self.page_rows = rows.saturating_sub(1).max(1);
        self.max_scroll = lines.saturating_sub(rows as usize).min(u16::MAX as usize) as u16;
        self.scroll = self.scroll.min(self.max_scroll);
    }
}

// Shipped in the executable so help also works offline and in the release ZIP.
fn content(topic: usize) -> &'static [&'static str] {
    match topic {
        0 => &[
            "# Worker stats, Git Bash and Wrangler in one window.",
            "Start with the demo, then connect your account. Configuration lives in a TOML file.",
            "",
            "# 1. Try it locally",
            "Install Git for Windows and use a terminal such as Windows Terminal. Add Node.js and Wrangler for CLI commands and live logs.",
            "$ ./cftui.exe --demo",
            "Demo metrics, deployments and logs are samples. Bash and HTTP requests still work for real.",
            "",
            "# 2. Point it at your project",
            "Copy cftui.example.toml to cftui.toml in the folder you launch from. Set account_id to your 32-character Cloudflare account ID and project_dir to an existing folder.",
            "  account_id = \"your-account-id\"",
            "  project_dir = 'C:\\path\\to\\your\\app'",
            "  shell = 'C:\\Program Files\\Git\\bin\\bash.exe'",
            "  refresh_seconds = 60",
            "Refresh defaults to 60 seconds; values below 15 are raised to 15. Restart cftui after editing configuration.",
            "",
            "# 3. Connect the dashboard",
            "Create an API token in Cloudflare's dashboard: My Profile > API Tokens. Scope it to your account with Account Analytics: Read. Add Workers Scripts: Read for deployment details.",
            "Set the token in the Git Bash window you launch cftui from:",
            "$ export CFTUI_API_TOKEN='your-token'",
            "$ ./cftui.exe",
            "Keep the token out of cftui.toml. CFTUI_ACCOUNT_ID can override account_id. .env files are not loaded automatically.",
            "",
            "# 4. Sign Wrangler in separately",
            "In the embedded Bash panel, use your existing Wrangler login or CLOUDFLARE_API_TOKEN. CFTUI_API_TOKEN is for the dashboard and is removed from embedded Bash.",
            "$ wrangler login",
            "Project-local Wrangler also works through npx --no-install wrangler. Use that prefix if wrangler isn't on PATH.",
            "",
            "# Saved projects",
            "Add one entry per project to cftui.toml, then restart. F7 switches projects; project_dir is always available as Default.",
            "  [[projects]]",
            "  name = \"My app\"",
            "  path = 'C:\\path\\to\\another\\app'",
            "  environment = \"production\"",
            "Each project keeps its own Bash session. The environment is a label and $CFTUI_PROJECT_ENV in Bash; add --env to your Wrangler commands yourself.",
            "$ wrangler dev --env \"$CFTUI_PROJECT_ENV\"",
            "",
            "# Launch options (in your outer terminal)",
            "$ ./cftui.exe --config 'C:/path/cftui.toml'",
            "$ ./cftui.exe --project 'C:/path/your-app'",
            "Use --shell PATH to choose another Bash executable. CLI project/shell options override the config. --help lists all options.",
        ],
        1 => &[
            "# Focus decides where typing goes",
            "Click a panel or press F6 to focus it. Normal keys go straight to Bash when Bash has focus. Dashboard shortcuts work outside Bash and text entry.",
            "",
            "# App controls",
            "Alt+H    Open/close help from any panel or text entry",
            "?        Open help with the dashboard focused",
            "F1-F4    Toggle Workers, D1, Resources, Bash",
            "F5       Refresh metrics and deployment details",
            "F6       Focus the next visible panel",
            "F7       Project picker; arrows select, Enter switches",
            "F8       Hide the focused panel",
            "F9       Restore all four main panels",
            "F10      Quit, including from help",
            "F11      Expand/restore the focused panel",
            "F12      Save layout and project now",
            "Mouse    Click to focus; [+] expand, [-] restore, [x] hide",
            "",
            "# Dashboard layouts",
            "1 / 2 / 3    Overview / Debugging / expanded Bash",
            "[ / ]        Shrink/grow Bash by 5 percentage points",
            "q            Quit with the dashboard focused",
            "",
            "# Workers and D1",
            "Tab / Left / Right    Switch Workers and D1 lists",
            "Up / Down             Select a resource",
            "/                     Search; Enter applies, Esc clears",
            "s                     Cycle the resource sort order",
            "Enter                 Open selected Worker details",
            "L                     Start/restart selected Worker's logs",
            "Esc                   Return to the resource list",
            "",
            "# Live logs",
            "/        Search events; Enter applies, Esc clears",
            "e        Toggle errors only",
            "Space    Pause/resume the displayed events",
            "f        Follow incoming events",
            "Up/Down  Select an event; scroll JSON when expanded",
            "Enter    Expand/collapse event JSON",
            "x / L    Stop / restart the tail",
            "Esc      Close event detail, then return to resources",
            "",
            "# HTTP inspector",
            "c             Open/expand HTTP; close when focused",
            "u             Edit URL; Ctrl+U clears while editing",
            "Enter / r     Send GET; Enter also submits an edited URL",
            "Tab / Left / Right    Switch headers and body",
            "Up / Down     Scroll one line",
            "PgUp / PgDn   Scroll ten lines; Home returns to the top",
            "Esc           Finish URL editing, then close the panel",
            "",
            "# Bash",
            "Ctrl+C                  Interrupt the running command",
            "Shift+PgUp / PgDn       Scroll terminal history",
            "Paste                   Paste into the focused input",
            "F6 / Alt+H / F10         Leave Bash / help / quit",
            "",
            "# This guide",
            "Tab / Shift+Tab or Left / Right    Change section",
            "Up / Down, PgUp / PgDn, wheel      Scroll",
            "Home / End                        First / last page",
            "Esc / ? / Alt+H                   Close and resume",
        ],
        2 => &[
            "# Find the Worker that's having a bad day",
            "Focus Resources. Use Tab for Workers, / to narrow the list, s to change sorting and arrows to select. Enter opens its metrics and hourly graph; Esc returns to the list.",
            "F5 refreshes. Deployment status at the top shows serving versions and when they went out; it doesn't establish Worker health.",
            "",
            "# Watch the next invocation",
            "Select a Worker and press L. Make a request to it, then look for the new event. Live tails show new invocations, not old ones.",
            "Use e for errors only, / to search and Enter for event JSON. Space freezes the view while the tail keeps receiving events. f follows new events; x stops the tail. L starts a fresh tail and clears the previous events.",
            "Switching to another project stops the current tail. Logs stay in memory, bounded to 32 MiB across live and paused views; older events can be dropped. Event details are capped at 32 KiB.",
            "",
            "# Check the endpoint beside its logs",
            "Press c from the dashboard, type an http:// or https:// URL and press Enter. Inspect status, header/total timings and headers. Tab switches to the body; JSON is formatted when possible.",
            "GET only. Redirects are shown without following them, so inspect Location. Responses have a 15-second timeout and a 256 KiB body cap; longer bodies are marked as truncated.",
            "Use F6 to reach another visible panel while HTTP is expanded, or [-] to restore the layout. Requests are real even in demo mode.",
            "",
            "# Run Wrangler without leaving the dashboard",
            "Focus Bash with F6 or a click. Run your usual commands, change folders or interrupt with Ctrl+C. Use Shift+PageUp/PageDown for terminal history.",
            "F7 switches saved projects. Returning restores that project's Bash session, including its current directory. Hiding Bash keeps it running; quitting closes every session.",
            "The project environment is not an automatic Wrangler target. For environment-specific commands, pass --env explicitly.",
            "",
            "# Keep the layout you use",
            "Try 1 for Overview, 2 for Debugging or 3 for Bash. Use [+] to give a panel the space, [x] to hide it, and F1-F4 to bring a main panel back. F9 resets to the main panels.",
            "Layout and last project save on exit or with F12, beside your config in its .state.toml file. Opening help keeps focus and text entry where you left them.",
            "",
            "# What the numbers mean",
            "Totals cover a rolling 24 hours; graphs show completed UTC hours. Cloudflare analytics may be sampled or delayed. Unavailable means missing data, not zero traffic.",
            "Git status refreshes every five seconds without fetching. Ahead/behind counts use your locally known upstream. Deployment details refresh every minute.",
        ],
        _ => &[
            "# Can't start cftui?",
            "Try --demo from your outer terminal. The demo bypasses Cloudflare credentials, but still needs Bash and a valid project folder. Startup errors appear in the terminal before the UI opens.",
            "Bash not found: install Git for Windows or set shell in the config / --shell PATH. The default is C:\\Program Files\\Git\\bin\\bash.exe.",
            "Project folder does not exist: check project_dir, every saved project's path and any --project override. All configured folders must exist, even if not selected.",
            "Invalid configuration: use the example TOML, check spelling and quote Windows paths with single quotes. Unknown settings are rejected. Restart after edits.",
            "",
            "# Dashboard data unavailable?",
            "Check the panel's error. Confirm the 32-character account ID, CFTUI_API_TOKEN and the token's account scope and Account Analytics: Read permission. Workers Scripts: Read enables deployment details.",
            "Export the token in the same outer terminal used to start cftui. .env is not read, and setting CFTUI_API_TOKEN in embedded Bash does not configure the dashboard.",
            "Run this in your outer terminal to print metrics and dataset warnings:",
            "$ ./cftui.exe --check",
            "An unavailable dataset can also reflect your account's analytics access. A quiet graph is different from an unavailable one.",
            "",
            "# Wrangler or logs won't connect?",
            "In Bash, check wrangler --version and wrangler whoami. Use wrangler login or CLOUDFLARE_API_TOKEN for Wrangler. Its credentials are separate from the dashboard token.",
            "For a project-local install, use npx --no-install wrangler. Check the selected project's folder. Live tails use the dashboard account and selected deployed Worker name, including any environment suffix.",
            "No events: trigger a new invocation, check / search and e errors-only filters, and resume with Space if paused. x stops the tail; L restarts it.",
            "",
            "# HTTP request failed?",
            "Use an http:// or https:// URL and confirm the service is running. Localhost is this computer. Connection, DNS, TLS and timeout failures appear in the HTTP panel.",
            "A 3xx response is visible as-is: inspect Location. A 4xx/5xx is still a response; inspect headers and body. The inspector sends GET without custom headers or a request body.",
            "",
            "# Keys seem to do nothing?",
            "Check the focused panel. Bash receives normal typing, so press F6 or click Resources for dashboard shortcuts. URL and search entry own typed text until Enter or Esc. Alt+H opens help from either.",
            "F9 restores the main panels. Enlarge the terminal to at least 50 columns by 15 rows. Alt+H and F10 are reserved by cftui; use Ctrl+C to interrupt Bash commands.",
            "",
            "# Check the terminal itself",
            "$ ./cftui.exe --check-shell",
            "Run from your outer terminal to check Bash input, output and resizing. --help lists launch options. Windows is the supported setup; macOS has not been verified.",
        ],
    }
}

pub fn lines(topic: usize, width: u16, heading: Style, command: Style) -> Vec<Line<'static>> {
    let width = usize::from(width.max(1));
    let mut lines = Vec::new();
    for source in content(topic) {
        let (text, style) = if let Some(text) = source.strip_prefix("# ") {
            (text, heading)
        } else if source.starts_with("$ ") || source.starts_with("  ") {
            (*source, command)
        } else {
            (*source, Style::default())
        };
        // All guide copy is ASCII. Wrap once so scroll bounds match rendered rows.
        let mut rest = text;
        while rest.len() > width {
            let end = rest[..=width]
                .rfind(' ')
                .filter(|end| *end > 0)
                .unwrap_or(width);
            lines.push(Line::styled(rest[..end].to_owned(), style));
            rest = rest[end..].trim_start();
        }
        lines.push(Line::styled(rest.to_owned(), style));
    }
    lines
}
