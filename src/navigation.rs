use crate::cloudflare::{MetricRow, Snapshot};

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum View {
    #[default]
    List,
    Details,
    Logs,
}
#[derive(Default, Clone, Copy)]
pub enum Sort {
    Name,
    #[default]
    Activity,
    Errors,
    Rate,
}
impl Sort {
    pub fn next(self) -> Self {
        match self {
            Self::Name => Self::Activity,
            Self::Activity => Self::Errors,
            Self::Errors => Self::Rate,
            Self::Rate => Self::Name,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Activity => "activity",
            Self::Errors => "errors / writes",
            Self::Rate => "error rate / write ratio",
        }
    }
}
#[derive(Default)]
pub struct Browser {
    pub filter: String,
    pub editing: bool,
    pub sort: Sort,
    pub selected: Option<String>,
    pub view: View,
    pub worker: Option<String>,
}
impl Browser {
    pub fn rows<'a>(&self, snapshot: &'a Snapshot, tab: usize) -> Vec<&'a MetricRow> {
        let source = if tab == 0 {
            &snapshot.workers
        } else {
            &snapshot.databases
        };
        let filter = self.filter.to_lowercase();
        let mut rows = source
            .iter()
            .flatten()
            .filter(|row| row.name.to_lowercase().contains(&filter))
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| {
            let order = match self.sort {
                Sort::Name => a.name.cmp(&b.name),
                Sort::Activity => b.first.cmp(&a.first),
                Sort::Errors => b.second.cmp(&a.second),
                Sort::Rate => ((b.second as u128) * (a.first.max(1) as u128))
                    .cmp(&((a.second as u128) * (b.first.max(1) as u128))),
            };
            order.then_with(|| a.name.cmp(&b.name))
        });
        rows
    }
    pub fn reconcile(&mut self, snapshot: &Snapshot, tab: usize) {
        let rows = self.rows(snapshot, tab);
        if !rows.iter().any(|r| Some(&r.name) == self.selected.as_ref()) {
            self.selected = rows.first().map(|r| r.name.clone());
        }
    }
    pub fn step(&mut self, snapshot: &Snapshot, tab: usize, down: bool) {
        let rows = self.rows(snapshot, tab);
        let i = rows
            .iter()
            .position(|r| Some(&r.name) == self.selected.as_ref())
            .unwrap_or(0);
        let next = if down {
            (i + 1).min(rows.len().saturating_sub(1))
        } else {
            i.saturating_sub(1)
        };
        self.selected = rows.get(next).map(|r| r.name.clone());
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_sort_and_refresh_preserve_resource_identity() {
        let snapshot = Snapshot::demo();
        let mut b = Browser::default();
        b.reconcile(&snapshot, 0);
        assert_eq!(b.selected.as_deref(), Some("api-production"));
        b.sort = Sort::Name;
        b.reconcile(&snapshot, 0);
        assert_eq!(b.selected.as_deref(), Some("api-production"));
        b.filter = "PUBLIC".into();
        b.reconcile(&snapshot, 0);
        assert_eq!(b.rows(&snapshot, 0).len(), 1);
        assert_eq!(b.selected.as_deref(), Some("public-site"));
        b.filter = "no match".into();
        b.reconcile(&snapshot, 0);
        assert!(b.selected.is_none());
        b.step(&snapshot, 0, true);
        assert!(b.selected.is_none());
    }
}
