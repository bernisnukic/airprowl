use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub struct NameStore {
    path: PathBuf,
    names: HashMap<String, String>,
}

impl NameStore {
    pub fn load(path: &Path) -> Self {
        let names = fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            path: path.to_path_buf(),
            names,
        }
    }

    pub fn get(&self, id: &str) -> Option<&str> {
        self.names.get(id).map(|s| s.as_str())
    }

    pub fn set(&mut self, id: &str, name: &str) {
        self.names.insert(id.to_string(), name.to_string());
        self.save();
    }

    pub fn remove(&mut self, id: &str) {
        self.names.remove(id);
        self.save();
    }

    fn save(&self) {
        let tmp = self.path.with_extension("json.tmp");
        if let Ok(data) = serde_json::to_string_pretty(&self.names) {
            if fs::write(&tmp, data.as_bytes()).is_ok() {
                let _ = fs::rename(&tmp, &self.path);
            }
        }
    }
}
