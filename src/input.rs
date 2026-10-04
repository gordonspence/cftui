use crate::{
    navigation::View,
    panels::{Panel, Panels},
    App,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
pub enum Action {
    None,
    Quit,
    Shell(KeyEvent),
    Refresh,
    Save,
    Switch(usize),
    Tail(String),
    SendRequest,
}
pub fn key(app: &mut App, key: KeyEvent) -> Action {
    if key.code == KeyCode::F(10) {
        return Action::Quit;
    }
    if matches!(key.code, KeyCode::Char('h' | 'H')) && key.modifiers.contains(KeyModifiers::ALT) {
        app.help = if app.help.is_some() {
            None
        } else {
            Some(crate::help::Guide::default())
        };
        return Action::None;
    }
    if let Some(guide) = &mut app.help {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') => app.help = None,
            KeyCode::Left | KeyCode::BackTab => {
                guide.select(guide.topic + crate::help::TOPICS.len() - 1)
            }
            KeyCode::Right | KeyCode::Tab => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    guide.select(guide.topic + crate::help::TOPICS.len() - 1);
                } else {
                    guide.select(guide.topic + 1);
                }
            }
            KeyCode::Up => guide.scroll_by(-1),
            KeyCode::Down => guide.scroll_by(1),
            KeyCode::PageUp => guide.scroll_by(-i32::from(guide.page_rows)),
            KeyCode::PageDown => guide.scroll_by(i32::from(guide.page_rows)),
            KeyCode::Home => guide.scroll = 0,
            KeyCode::End => guide.scroll = guide.max_scroll,
            _ => {}
        }
        return Action::None;
    }
    // Modal input owns text, including q, so it never leaks into Bash or exits the app.
    if let Some(index) = &mut app.project_picker {
        match key.code {
            KeyCode::Esc => app.project_picker = None,
            KeyCode::Up => *index = index.saturating_sub(1),
            KeyCode::Down => *index = (*index + 1).min(app.projects.len() - 1),
            KeyCode::Enter => {
                let i = *index;
                app.project_picker = None;
                return Action::Switch(i);
            }
            _ => {}
        }
        return Action::None;
    }
    if app.request.editing {
        match key.code {
            KeyCode::Enter => {
                app.request.editing = false;
                return Action::SendRequest;
            }
            KeyCode::Esc => app.request.editing = false,
            KeyCode::Backspace => {
                app.request.url.pop();
            }
            KeyCode::Char('u')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                app.request.url.clear()
            }
            KeyCode::Char(c)
                if !key.modifiers.intersects(
                    crossterm::event::KeyModifiers::CONTROL | crossterm::event::KeyModifiers::ALT,
                ) && app.request.url.chars().count() < 4096 =>
            {
                app.request.url.push(c)
            }
            _ => {}
        }
        return Action::None;
    }
    if app.browser.editing {
        let text = if app.browser.view == View::Logs {
            &mut app.logs.filter
        } else {
            &mut app.browser.filter
        };
        match key.code {
            KeyCode::Enter => app.browser.editing = false,
            KeyCode::Esc => {
                text.clear();
                app.browser.editing = false;
            }
            KeyCode::Backspace => {
                text.pop();
            }
            KeyCode::Char(c)
                if !key.modifiers.intersects(
                    crossterm::event::KeyModifiers::CONTROL | crossterm::event::KeyModifiers::ALT,
                ) && text.chars().count() < 256 =>
            {
                text.push(c);
            }
            _ => {}
        }
        if let Some(snapshot) = &app.snapshot {
            app.browser.reconcile(snapshot, app.tab);
        }
        app.logs.selected = 0;
        app.offset = 0;
        return Action::None;
    }
    match key.code {
        KeyCode::F(1..=4) => {
            if let KeyCode::F(n) = key.code {
                app.panels.toggle(Panel::ALL[(n - 1) as usize]);
            }
        }
        KeyCode::F(5) => return Action::Refresh,
        KeyCode::F(6) => app.panels.next(),
        KeyCode::F(7) => app.project_picker = Some(app.project),
        KeyCode::F(8) => app.panels.close(app.panels.focused),
        KeyCode::F(9) => app.panels.restore(),
        KeyCode::F(11) => app.panels.zoom(app.panels.focused),
        KeyCode::F(12) => return Action::Save,
        _ if app.panels.shell_input() => return Action::Shell(key),
        KeyCode::Char('c') => {
            if app.panels.focused == Panel::Request {
                app.panels.close(Panel::Request);
            } else {
                app.panels.zoom(Panel::Request);
                app.request.editing = app.request.url.is_empty();
            }
        }
        _ if app.panels.focused == Panel::Request => match key.code {
            KeyCode::Char('u') => app.request.editing = true,
            KeyCode::Char('r') | KeyCode::Enter => return Action::SendRequest,
            KeyCode::Tab | KeyCode::Left | KeyCode::Right => {
                app.request.body_tab = !app.request.body_tab;
                app.request.scroll = 0;
            }
            KeyCode::Up => app.request.scroll = app.request.scroll.saturating_sub(1),
            KeyCode::Down => app.request.scroll = app.request.scroll.saturating_add(1),
            KeyCode::PageUp => app.request.scroll = app.request.scroll.saturating_sub(10),
            KeyCode::PageDown => app.request.scroll = app.request.scroll.saturating_add(10),
            KeyCode::Home => app.request.scroll = 0,
            KeyCode::Esc => app.panels.close(Panel::Request),
            KeyCode::Char('?') => app.help = Some(crate::help::Guide::shortcuts()),
            KeyCode::Char('q') => return Action::Quit,
            _ => {}
        },
        KeyCode::Char('?') => app.help = Some(crate::help::Guide::shortcuts()),
        KeyCode::Char('q') => return Action::Quit,
        KeyCode::Char('1') => app.panels = Panels::default(),
        KeyCode::Char('2') => {
            app.panels = Panels {
                visible: [true, false, true, true],
                shell_percent: 45,
                ..Panels::default()
            }
        }
        KeyCode::Char('3') => {
            app.panels = Panels::default();
            app.panels.zoom(Panel::Bash);
        }
        KeyCode::Char('[') => {
            app.panels.shell_percent = app.panels.shell_percent.saturating_sub(5).max(15)
        }
        KeyCode::Char(']') => app.panels.shell_percent = (app.panels.shell_percent + 5).min(70),
        KeyCode::Char('/') => {
            app.browser.editing = true;
            app.panels.visible[2] = true;
            app.panels.focus(Panel::Resources);
        }
        KeyCode::Esc => {
            if app.browser.view == View::Logs && app.logs.expanded {
                app.logs.expanded = false;
            } else {
                app.browser.view = View::List;
                app.browser.worker = None;
            }
            app.offset = 0;
        }
        KeyCode::Char('s') if app.browser.view == View::List => {
            app.browser.sort = app.browser.sort.next()
        }
        KeyCode::Tab | KeyCode::Left | KeyCode::Right if app.browser.view == View::List => {
            app.switch_resource_tab();
        }
        KeyCode::Char('l') | KeyCode::Char('L') => {
            let worker = if app.browser.view == View::List && app.tab == 0 {
                app.browser.selected.clone()
            } else {
                app.browser.worker.clone()
            };
            if let Some(worker) = worker {
                return Action::Tail(worker);
            }
            app.message = "Select a Worker first".into();
        }
        KeyCode::Char('e') if app.browser.view == View::Logs => {
            app.logs.errors_only = !app.logs.errors_only;
            app.logs.selected = 0;
        }
        KeyCode::Char(' ') if app.browser.view == View::Logs => app.logs.pause(),
        KeyCode::Char('f') if app.browser.view == View::Logs => {
            app.logs.follow = true;
        }
        KeyCode::Char('x') if app.browser.view == View::Logs => app.logs.stop(),
        KeyCode::Enter if app.browser.view == View::Logs => {
            app.logs.expanded = !app.logs.expanded;
            app.offset = 0;
        }
        KeyCode::Enter if app.tab == 0 => {
            app.show_worker_details();
        }
        KeyCode::Up | KeyCode::Down => {
            let down = key.code == KeyCode::Down;
            if app.browser.view == View::Logs {
                app.logs.follow = false;
                if app.logs.expanded {
                    app.offset = if down {
                        app.offset.saturating_add(1)
                    } else {
                        app.offset.saturating_sub(1)
                    };
                } else {
                    let last = app.logs.rows().len().saturating_sub(1);
                    app.logs.selected = if down {
                        (app.logs.selected + 1).min(last)
                    } else {
                        app.logs.selected.saturating_sub(1)
                    };
                }
            } else if let Some(snapshot) = &app.snapshot {
                app.browser.step(snapshot, app.tab, down);
            }
        }
        _ => {}
    }
    if let Some(snapshot) = &app.snapshot {
        app.browser.reconcile(snapshot, app.tab);
    }
    Action::None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    fn app() -> App {
        let mut app = App::new(
            true,
            String::new(),
            vec![crate::config::Project {
                name: "Default".into(),
                path: ".".into(),
                environment: String::new(),
            }],
        );
        app.snapshot = Some(crate::cloudflare::Snapshot::demo());
        app.browser.reconcile(app.snapshot.as_ref().unwrap(), 0);
        app
    }
    fn press(app: &mut App, code: KeyCode) -> Action {
        key(app, KeyEvent::new(code, KeyModifiers::NONE))
    }
    #[test]
    fn request_url_input_does_not_leak_to_shell_and_tabs_switch() {
        let mut app = app();
        press(&mut app, KeyCode::Char('c'));
        assert_eq!(app.panels.focused, Panel::Request);
        assert!(app.request.editing);
        for c in "http://localhost/path?q=hello".chars() {
            assert!(matches!(press(&mut app, KeyCode::Char(c)), Action::None));
        }
        assert!(matches!(
            press(&mut app, KeyCode::Enter),
            Action::SendRequest
        ));
        assert!(!app.request.editing);
        press(&mut app, KeyCode::Tab);
        assert!(app.request.body_tab);
        press(&mut app, KeyCode::Esc);
        assert!(!app.panels.request_visible);
    }
    #[test]
    fn search_details_and_logs_use_selected_worker() {
        let mut app = app();
        press(&mut app, KeyCode::Char('/'));
        for c in "public".chars() {
            assert!(matches!(press(&mut app, KeyCode::Char(c)), Action::None));
        }
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Enter);
        assert!(app.browser.view == View::Details);
        assert_eq!(app.browser.worker.as_deref(), Some("public-site"));
        assert!(
            matches!(press(&mut app,KeyCode::Char('l')),Action::Tail(name) if name=="public-site")
        );
        press(&mut app, KeyCode::Esc);
        assert!(app.browser.view == View::List);
    }
    #[test]
    fn modal_and_search_typing_never_forward_to_shell_or_quit() {
        let mut app = app();
        app.panels.focus(Panel::Bash);
        assert!(matches!(
            press(&mut app, KeyCode::Char('q')),
            Action::Shell(_)
        ));
        press(&mut app, KeyCode::F(7));
        assert!(matches!(press(&mut app, KeyCode::Char('q')), Action::None));
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::F(6));
        press(&mut app, KeyCode::Char('/'));
        assert!(matches!(press(&mut app, KeyCode::Char('q')), Action::None));
        assert_eq!(app.browser.filter, "q");
        press(&mut app, KeyCode::Esc);
        assert!(app.browser.filter.is_empty());
    }
    #[test]
    fn help_owns_input_and_resumes_bash_and_text_entry() {
        let mut app = app();
        app.panels.focus(Panel::Bash);
        let help_key = KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT);
        assert!(matches!(key(&mut app, help_key), Action::None));
        assert!(app.help.is_some());
        for code in [
            KeyCode::Char('q'),
            KeyCode::Enter,
            KeyCode::F(6),
            KeyCode::F(7),
        ] {
            assert!(matches!(press(&mut app, code), Action::None));
            assert!(app.help.is_some());
        }
        assert_eq!(app.panels.focused, Panel::Bash);
        assert!(app.project_picker.is_none());
        assert!(matches!(press(&mut app, KeyCode::F(10)), Action::Quit));
        press(&mut app, KeyCode::Esc);
        assert!(matches!(
            press(&mut app, KeyCode::Char('q')),
            Action::Shell(_)
        ));

        app.panels.focus(Panel::Resources);
        press(&mut app, KeyCode::Char('/'));
        app.browser.filter = "public".into();
        key(&mut app, help_key);
        press(&mut app, KeyCode::Char('q'));
        press(&mut app, KeyCode::Enter);
        key(&mut app, help_key);
        assert!(app.browser.editing);
        assert_eq!(app.browser.filter, "public");
        press(&mut app, KeyCode::Esc);

        press(&mut app, KeyCode::Char('c'));
        app.request.url = "http://localhost/health".into();
        key(&mut app, help_key);
        assert!(matches!(press(&mut app, KeyCode::Enter), Action::None));
        press(&mut app, KeyCode::Esc);
        assert!(app.request.editing);
        assert_eq!(app.request.url, "http://localhost/health");
        assert!(matches!(
            press(&mut app, KeyCode::Enter),
            Action::SendRequest
        ));
    }
    #[test]
    fn question_mark_opens_keys_and_help_navigation_never_changes_resources() {
        let mut app = app();
        let selected = app.browser.selected.clone();
        press(&mut app, KeyCode::Char('?'));
        assert_eq!(app.help.as_ref().unwrap().topic, 1);
        press(&mut app, KeyCode::BackTab);
        assert_eq!(app.help.as_ref().unwrap().topic, 0);
        press(&mut app, KeyCode::Left);
        assert_eq!(app.help.as_ref().unwrap().topic, 3);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.help.as_ref().unwrap().topic, 0);
        app.help.as_mut().unwrap().fit(100, 10);
        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.help.as_ref().unwrap().scroll, 9);
        press(&mut app, KeyCode::End);
        assert_eq!(app.help.as_ref().unwrap().scroll, 90);
        press(&mut app, KeyCode::Up);
        assert_eq!(app.help.as_ref().unwrap().scroll, 89);
        press(&mut app, KeyCode::Home);
        assert_eq!(app.help.as_ref().unwrap().scroll, 0);
        assert_eq!(app.browser.selected, selected);
        assert_eq!(app.tab, 0);
        press(&mut app, KeyCode::Char('?'));
        assert!(app.help.is_none());
    }
}
