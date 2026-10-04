use crate::{
    cloudflare::Snapshot,
    config, context, deployment, help, logs, navigation,
    panels::{Panel, Panels},
    request,
};

pub(crate) struct App {
    pub(crate) snapshot: Option<Snapshot>,
    pub(crate) demo: bool,
    pub(crate) panels: Panels,
    pub(crate) tab: usize,
    pub(crate) offset: usize,
    pub(crate) refreshing: bool,
    pub(crate) account: String,
    pub(crate) browser: navigation::Browser,
    pub(crate) logs: logs::Logs,
    pub(crate) projects: Vec<config::Project>,
    pub(crate) project: usize,
    pub(crate) project_picker: Option<usize>,
    pub(crate) message: String,
    pub(crate) help: Option<help::Guide>,
    pub(crate) request: request::Inspector,
    pub(crate) context: context::Status,
    pub(crate) deployment: deployment::Status,
}
impl App {
    pub(crate) fn refresh_completed(&mut self, snapshot: Snapshot) {
        self.browser.invalidate();
        self.browser.reconcile(&snapshot, self.tab);
        self.snapshot = Some(snapshot);
        self.refreshing = false;
    }

    // The runtime attempts the shell switch first. Failed switches only change the message.
    pub(crate) fn project_switch_completed(
        &mut self,
        index: usize,
        result: anyhow::Result<()>,
    ) -> bool {
        if let Err(error) = result {
            self.message = format!("Could not switch project: {error}");
            return false;
        }
        if index != self.project {
            self.logs.stop();
        }
        self.project = index;
        self.context = context::Status::default();
        self.message = format!(
            "Project: {} · separate Bash session",
            self.projects[index].name
        );
        self.panels.visible[Panel::Bash.index()] = true;
        self.panels.focus(Panel::Bash);
        true
    }

    pub(crate) fn show_worker_details(&mut self) {
        self.browser.worker = self.browser.selected.clone();
        if self.browser.worker.is_some() {
            self.show_worker_view(navigation::View::Details);
        }
    }

    pub(crate) fn logs_started(&mut self, worker: String) {
        self.browser.worker = Some(worker);
        self.show_worker_view(navigation::View::Logs);
    }

    fn show_worker_view(&mut self, view: navigation::View) {
        self.browser.view = view;
        self.panels.visible[Panel::Resources.index()] = true;
        self.panels.focus(Panel::Resources);
        self.offset = 0;
    }

    pub(crate) fn switch_resource_tab(&mut self) {
        self.tab = 1 - self.tab;
        self.browser.selected = None;
        self.offset = 0;
        if let Some(snapshot) = &self.snapshot {
            self.browser.reconcile(snapshot, self.tab);
        }
    }

    pub(crate) fn new(demo: bool, account: String, projects: Vec<config::Project>) -> Self {
        Self {
            snapshot: None,
            demo,
            panels: Panels::default(),
            tab: 0,
            offset: 0,
            refreshing: true,
            account,
            browser: navigation::Browser::default(),
            logs: logs::Logs::default(),
            projects,
            project: 0,
            project_picker: None,
            message: String::new(),
            help: None,
            request: request::Inspector::default(),
            context: context::Status::default(),
            deployment: deployment::Status::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        App::new(
            true,
            String::new(),
            vec![
                config::Project {
                    name: "First".into(),
                    path: ".".into(),
                    environment: String::new(),
                },
                config::Project {
                    name: "Second".into(),
                    path: ".".into(),
                    environment: String::new(),
                },
            ],
        )
    }

    #[test]
    fn refresh_preserves_selection_and_replaces_cached_rows() {
        let mut app = app();
        app.refresh_completed(Snapshot::demo());
        app.browser.selected = Some("public-site".into());
        // Populate the row cache before replacing and reordering the snapshot.
        app.browser.rows(app.snapshot.as_ref().unwrap(), app.tab);
        let mut next = Snapshot::demo();
        next.workers.as_mut().unwrap().reverse();
        app.refreshing = true;
        app.refresh_completed(next);
        assert!(!app.refreshing);
        assert_eq!(app.browser.selected.as_deref(), Some("public-site"));
        let rows = app.browser.rows(app.snapshot.as_ref().unwrap(), 0);
        assert_eq!(rows[0].name, "api-production");
        drop(rows);

        let mut next = Snapshot::demo();
        next.workers
            .as_mut()
            .unwrap()
            .retain(|row| row.name != "public-site");
        app.refresh_completed(next);
        assert_eq!(app.browser.selected.as_deref(), Some("api-production"));
    }

    #[test]
    fn failed_project_switch_preserves_context_logs_and_focus() {
        let mut app = app();
        app.context.git = "main*".into();
        app.logs.status = "Following api".into();
        app.panels.visible[Panel::Bash.index()] = false;
        app.panels.focus(Panel::Resources);
        assert!(!app.project_switch_completed(1, Err(anyhow::anyhow!("shell unavailable"))));
        assert_eq!(app.project, 0);
        assert_eq!(app.context.git, "main*");
        assert_eq!(app.logs.status, "Following api");
        assert!(!app.panels.visible[Panel::Bash.index()]);
        assert_eq!(app.panels.focused, Panel::Resources);
        assert!(app.message.contains("shell unavailable"));
    }

    #[test]
    fn successful_project_switch_resets_context_and_stops_logs_only_when_changed() {
        let mut app = app();
        app.logs.status = "Following api".into();
        assert!(app.project_switch_completed(0, Ok(())));
        assert_eq!(app.logs.status, "Following api");
        app.context.git = "main*".into();
        app.panels.visible[Panel::Bash.index()] = false;
        assert!(app.project_switch_completed(1, Ok(())));
        assert_eq!(app.project, 1);
        assert_eq!(app.context.git, context::Status::default().git);
        assert_ne!(app.logs.status, "Following api");
        assert!(app.panels.visible[Panel::Bash.index()]);
        assert_eq!(app.panels.focused, Panel::Bash);
    }

    #[test]
    fn worker_views_and_resource_tabs_keep_selection_and_scroll_consistent() {
        let mut app = app();
        app.refresh_completed(Snapshot::demo());
        app.browser.selected = Some("public-site".into());
        app.offset = 12;
        app.panels.visible[Panel::Resources.index()] = false;
        app.show_worker_details();
        assert_eq!(app.browser.worker.as_deref(), Some("public-site"));
        assert!(app.browser.view == navigation::View::Details);
        assert_eq!(app.offset, 0);
        assert!(app.panels.visible[Panel::Resources.index()]);
        assert_eq!(app.panels.focused, Panel::Resources);
        app.offset = 7;
        app.logs_started("api-production".into());
        assert!(app.browser.view == navigation::View::Logs);
        assert_eq!(app.browser.worker.as_deref(), Some("api-production"));
        assert_eq!(app.offset, 0);
        app.switch_resource_tab();
        assert_eq!(app.tab, 1);
        assert_eq!(app.browser.selected.as_deref(), Some("demo-app-db"));
    }
}
