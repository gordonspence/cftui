use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Panel {
    Workers,
    D1,
    Resources,
    Bash,
    Request,
}
impl Panel {
    pub const ALL: [Self; 5] = [
        Self::Workers,
        Self::D1,
        Self::Resources,
        Self::Bash,
        Self::Request,
    ];
    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Panels {
    pub visible: [bool; 4],
    pub focused: Panel,
    pub expanded: Option<Panel>,
    pub shell_percent: u16,
    pub request_visible: bool,
}
impl Default for Panels {
    fn default() -> Self {
        Self {
            visible: [true; 4],
            focused: Panel::Resources,
            expanded: None,
            shell_percent: 33,
            request_visible: false,
        }
    }
}
impl Panels {
    pub fn is_visible(&self, panel: Panel) -> bool {
        if panel == Panel::Request {
            self.request_visible
        } else {
            self.visible[panel.index()]
        }
    }
    fn set_visible(&mut self, panel: Panel, value: bool) {
        if panel == Panel::Request {
            self.request_visible = value;
        } else {
            self.visible[panel.index()] = value;
        }
    }
    pub fn focus(&mut self, panel: Panel) {
        if self.is_visible(panel) {
            self.focused = panel;
            if self.expanded.is_some() {
                self.expanded = Some(panel);
            }
        }
    }
    pub fn next(&mut self) {
        for step in 1..=5 {
            let panel = Panel::ALL[(self.focused.index() + step) % 5];
            if self.is_visible(panel) {
                self.focus(panel);
                break;
            }
        }
    }
    pub fn toggle(&mut self, panel: Panel) {
        if self.is_visible(panel) {
            self.close(panel);
        } else {
            self.set_visible(panel, true);
            self.focus(panel);
        }
    }
    pub fn close(&mut self, panel: Panel) {
        self.set_visible(panel, false);
        if self.expanded == Some(panel) {
            self.expanded = None;
        }
        if self.focused == panel {
            self.next();
        }
    }
    pub fn zoom(&mut self, panel: Panel) {
        self.set_visible(panel, true);
        self.focused = panel;
        self.expanded = if self.expanded == Some(panel) {
            None
        } else {
            Some(panel)
        };
    }
    pub fn restore(&mut self) {
        *self = Self::default();
    }
    pub fn shell_input(&self) -> bool {
        self.visible[Panel::Bash.index()] && self.focused == Panel::Bash
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn close_zoom_and_restore_leave_no_invisible_focus() {
        let mut panels = Panels::default();
        panels.zoom(Panel::Bash);
        panels.close(Panel::Bash);
        assert_eq!(panels.expanded, None);
        assert!(!panels.shell_input());
        for panel in Panel::ALL {
            panels.close(panel);
        }
        panels.next();
        assert!(!panels.shell_input());
        panels.toggle(Panel::Bash);
        assert!(panels.shell_input());
        panels.restore();
        assert_eq!(panels.visible, [true; 4]);
    }
    #[test]
    fn cycling_skips_hidden_panels_and_moves_zoom() {
        let mut panels = Panels::default();
        panels.close(Panel::Bash);
        panels.zoom(Panel::Resources);
        panels.next();
        assert_eq!(panels.focused, Panel::Workers);
        assert_eq!(panels.expanded, Some(Panel::Workers));
        panels.zoom(Panel::Workers);
        assert_eq!(panels.expanded, None);
    }
}
