use crate::recent::config_dir;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct UserSettings {
    pub dark_mode: bool,
    pub editor_font_size: f32,
    pub auto_close: bool,
    pub live_compile: bool,
    pub pdf_magnifier: bool,
    pub spell_lang: String,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            dark_mode: false,
            editor_font_size: 16.0,
            auto_close: true,
            live_compile: true,
            pdf_magnifier: false,
            spell_lang: "it_IT".to_string(),
        }
    }
}

fn settings_path() -> Option<PathBuf> {
    Some(config_dir()?.join("settings.txt"))
}

pub fn serialize_settings(s: &UserSettings) -> String {
    format!(
        "dark_mode={}\neditor_font_size={:.1}\nauto_close={}\nlive_compile={}\npdf_magnifier={}\nspell_lang={}\n",
        s.dark_mode,
        s.editor_font_size,
        s.auto_close,
        s.live_compile,
        s.pdf_magnifier,
        s.spell_lang
    )
}

pub fn parse_settings(raw: &str) -> UserSettings {
    let mut out = UserSettings::default();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(v) = trimmed.strip_prefix("dark_mode=") {
            out.dark_mode = matches!(v.trim(), "true" | "1" | "yes");
        } else if let Some(v) = trimmed.strip_prefix("editor_font_size=") {
            if let Ok(sz) = v.trim().parse::<f32>() {
                out.editor_font_size = sz.clamp(11.0, 30.0);
            }
        } else if let Some(v) = trimmed.strip_prefix("auto_close=") {
            out.auto_close = matches!(v.trim(), "true" | "1" | "yes");
        } else if let Some(v) = trimmed.strip_prefix("live_compile=") {
            out.live_compile = matches!(v.trim(), "true" | "1" | "yes");
        } else if let Some(v) = trimmed.strip_prefix("pdf_magnifier=") {
            out.pdf_magnifier = matches!(v.trim(), "true" | "1" | "yes");
        } else if let Some(v) = trimmed.strip_prefix("spell_lang=") {
            let lang = v.trim();
            if !lang.is_empty() {
                out.spell_lang = lang.to_string();
            }
        }
    }
    out
}

pub fn load_settings() -> UserSettings {
    let Some(path) = settings_path() else {
        return UserSettings::default();
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return UserSettings::default();
    };
    parse_settings(&raw)
}

pub fn save_settings(s: &UserSettings) {
    let Some(path) = settings_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, serialize_settings(s));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_preserves_values_and_clamps_font() {
        let s = UserSettings {
            dark_mode: true,
            editor_font_size: 21.0,
            auto_close: false,
            live_compile: false,
            pdf_magnifier: true,
            spell_lang: "en_US".to_string(),
        };
        let txt = serialize_settings(&s);
        let parsed = parse_settings(&txt);
        assert_eq!(parsed, s);

        let clamped = parse_settings("editor_font_size=99.0\n");
        assert_eq!(clamped.editor_font_size, 30.0);
    }
}

