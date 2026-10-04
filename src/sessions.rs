use crate::{config::Project, shell::Shell};
use anyhow::Result;
use std::path::Path;

pub struct Sessions {
    pub projects: Vec<Project>,
    shells: Vec<Option<Shell>>,
    pub active: usize,
}
impl Sessions {
    pub fn new(projects: Vec<Project>, active: usize, executable: &Path) -> Result<Self> {
        let shells = (0..projects.len()).map(|_| None).collect();
        let mut sessions = Self {
            projects,
            shells,
            active: 0,
        };
        sessions.switch(active, executable)?;
        Ok(sessions)
    }
    pub fn switch(&mut self, index: usize, executable: &Path) -> Result<()> {
        if self.shells[index].is_none() {
            let mut shell = Shell::start(executable, &self.projects[index].path)?;
            shell.send(
                format!(
                    "export CFTUI_PROJECT_ENV={}\r",
                    crate::logs::quote(&self.projects[index].environment)
                )
                .as_bytes(),
            )?;
            self.shells[index] = Some(shell);
        }
        self.active = index;
        Ok(())
    }
    pub fn drain(&mut self) -> Result<()> {
        for shell in self.shells.iter_mut().flatten() {
            shell.drain()?;
        }
        Ok(())
    }
    pub fn shell(&mut self) -> &mut Shell {
        self.shells[self.active].as_mut().unwrap()
    }
}
