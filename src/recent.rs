use std::path::{Path, PathBuf};

const MAX_RECENT: usize = 8;

pub fn config_dir() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("feathertex"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return Some(PathBuf::from(home).join(".config").join("feathertex"));
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        return Some(PathBuf::from(profile).join(".feathertex"));
    }
    None
}

fn storage_path() -> Option<PathBuf> {
    Some(config_dir()?.join("recent.txt"))
}

pub fn autosave_dir() -> Option<PathBuf> {
    Some(config_dir()?.join("autosave"))
}

/// Lists `.tex`, `.bib`, `.sty`, `.cls` files inside `dir` sorted alphabetically.
pub fn list_folder_tex_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "tex" | "bib" | "sty" | "cls"))
                    .unwrap_or(false)
        })
        .collect();
    files.sort_by(|a, b| {
        a.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase()
            .cmp(
                &b.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_lowercase(),
            )
    });
    files
}

pub fn load_recent() -> Vec<PathBuf> {
    let Some(path) = storage_path() else {
        return Vec::new();
    };
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };

    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .take(MAX_RECENT)
        .collect()
}

pub fn record_recent(files: &mut Vec<PathBuf>, new_path: PathBuf) {
    files.retain(|p| p != &new_path);
    files.insert(0, new_path);
    files.truncate(MAX_RECENT);

    if let Some(store) = storage_path() {
        if let Some(parent) = store.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let body = files
            .iter()
            .filter_map(|p| p.to_str())
            .collect::<Vec<_>>()
            .join("\n");
        let _ = std::fs::write(store, body);
    }
}
