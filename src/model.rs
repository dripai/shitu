use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppEntry {
    pub id: String,
    pub name: String,
    pub group: String,
    pub launch_path: PathBuf,
    pub source: AppSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AppSource {
    StartMenu,
    Desktop,
}

impl AppEntry {
    pub fn matches(&self, query: &str) -> bool {
        if query.trim().is_empty() {
            return true;
        }
        let query = query.to_lowercase();
        self.name.to_lowercase().contains(&query) || self.group.to_lowercase().contains(&query)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{AppEntry, AppSource};

    #[test]
    fn matches_name_and_group_case_insensitively() {
        let app = AppEntry {
            id: "id".to_owned(),
            name: "Visual Studio Code".to_owned(),
            group: "Development".to_owned(),
            launch_path: PathBuf::from("code.exe"),
            source: AppSource::StartMenu,
        };

        assert!(app.matches("visual"));
        assert!(app.matches("development"));
        assert!(app.matches(""));
        assert!(!app.matches("browser"));
    }
}
