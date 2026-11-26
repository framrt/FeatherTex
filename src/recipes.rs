use crate::recent::config_dir;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipeStep {
    PdfLatex,
    LuaLatex,
    XeLatex,
    BibTex,
    Biber,
    MakeGlossaries,
    MakeIndex,
    LatexMkPdf,
    LatexMkLua,
    LatexMkXe,
}

impl RecipeStep {
    pub const ALL: &[RecipeStep] = &[
        RecipeStep::PdfLatex,
        RecipeStep::LuaLatex,
        RecipeStep::XeLatex,
        RecipeStep::BibTex,
        RecipeStep::Biber,
        RecipeStep::MakeGlossaries,
        RecipeStep::MakeIndex,
        RecipeStep::LatexMkPdf,
        RecipeStep::LatexMkLua,
        RecipeStep::LatexMkXe,
    ];

    pub fn id(self) -> &'static str {
        match self {
            RecipeStep::PdfLatex => "pdflatex",
            RecipeStep::LuaLatex => "lualatex",
            RecipeStep::XeLatex => "xelatex",
            RecipeStep::BibTex => "bibtex",
            RecipeStep::Biber => "biber",
            RecipeStep::MakeGlossaries => "makeglossaries",
            RecipeStep::MakeIndex => "makeindex",
            RecipeStep::LatexMkPdf => "latexmk_pdf",
            RecipeStep::LatexMkLua => "latexmk_lua",
            RecipeStep::LatexMkXe => "latexmk_xe",
        }
    }

    pub fn from_id(s: &str) -> Option<Self> {
        match s.trim() {
            "pdflatex" => Some(RecipeStep::PdfLatex),
            "lualatex" => Some(RecipeStep::LuaLatex),
            "xelatex" => Some(RecipeStep::XeLatex),
            "bibtex" => Some(RecipeStep::BibTex),
            "biber" => Some(RecipeStep::Biber),
            "makeglossaries" => Some(RecipeStep::MakeGlossaries),
            "makeindex" => Some(RecipeStep::MakeIndex),
            "latexmk_pdf" => Some(RecipeStep::LatexMkPdf),
            "latexmk_lua" => Some(RecipeStep::LatexMkLua),
            "latexmk_xe" => Some(RecipeStep::LatexMkXe),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RecipeStep::PdfLatex => "pdfLaTeX",
            RecipeStep::LuaLatex => "LuaLaTeX",
            RecipeStep::XeLatex => "XeLaTeX",
            RecipeStep::BibTex => "BibTeX",
            RecipeStep::Biber => "Biber",
            RecipeStep::MakeGlossaries => "MakeGlossaries",
            RecipeStep::MakeIndex => "MakeIndex",
            RecipeStep::LatexMkPdf => "latexmk (pdf)",
            RecipeStep::LatexMkLua => "latexmk (lua)",
            RecipeStep::LatexMkXe => "latexmk (xe)",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            RecipeStep::PdfLatex => "pdf",
            RecipeStep::LuaLatex => "lua",
            RecipeStep::XeLatex => "xe",
            RecipeStep::BibTex => "bibtex",
            RecipeStep::Biber => "biber",
            RecipeStep::MakeGlossaries => "gloss",
            RecipeStep::MakeIndex => "index",
            RecipeStep::LatexMkPdf => "latexmk",
            RecipeStep::LatexMkLua => "lmk-lua",
            RecipeStep::LatexMkXe => "lmk-xe",
        }
    }

    pub fn is_tex_engine(self) -> bool {
        matches!(
            self,
            RecipeStep::PdfLatex
                | RecipeStep::LuaLatex
                | RecipeStep::XeLatex
                | RecipeStep::LatexMkPdf
                | RecipeStep::LatexMkLua
                | RecipeStep::LatexMkXe
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileRecipe {
    pub id: String,
    pub name: String,
    pub steps: Vec<RecipeStep>,
    pub builtin: bool,
}

impl CompileRecipe {
    pub fn steps_summary(&self) -> String {
        self.steps
            .iter()
            .map(|s| s.label())
            .collect::<Vec<_>>()
            .join(" → ")
    }

    pub fn is_fast_single_pdflatex(&self) -> bool {
        self.steps.len() == 1 && self.steps[0] == RecipeStep::PdfLatex
    }
}

pub fn builtin_recipes() -> Vec<CompileRecipe> {
    vec![
        CompileRecipe {
            id: "pdflatex".to_string(),
            name: "pdfLaTeX".to_string(),
            steps: vec![RecipeStep::PdfLatex],
            builtin: true,
        },
        CompileRecipe {
            id: "pdflatex_bibtex".to_string(),
            name: "pdfLaTeX + BibTeX".to_string(),
            steps: vec![
                RecipeStep::PdfLatex,
                RecipeStep::BibTex,
                RecipeStep::PdfLatex,
                RecipeStep::PdfLatex,
            ],
            builtin: true,
        },
        CompileRecipe {
            id: "pdflatex_biber".to_string(),
            name: "pdfLaTeX + Biber".to_string(),
            steps: vec![
                RecipeStep::PdfLatex,
                RecipeStep::Biber,
                RecipeStep::PdfLatex,
                RecipeStep::PdfLatex,
            ],
            builtin: true,
        },
        CompileRecipe {
            id: "lualatex".to_string(),
            name: "LuaLaTeX".to_string(),
            steps: vec![RecipeStep::LuaLatex],
            builtin: true,
        },
        CompileRecipe {
            id: "xelatex".to_string(),
            name: "XeLaTeX".to_string(),
            steps: vec![RecipeStep::XeLatex],
            builtin: true,
        },
        CompileRecipe {
            id: "latexmk_pdf".to_string(),
            name: "latexmk (Auto)".to_string(),
            steps: vec![RecipeStep::LatexMkPdf],
            builtin: true,
        },
        CompileRecipe {
            id: "glossaries_bibtex".to_string(),
            name: "Glossari + BibTeX".to_string(),
            steps: vec![
                RecipeStep::PdfLatex,
                RecipeStep::BibTex,
                RecipeStep::MakeGlossaries,
                RecipeStep::PdfLatex,
                RecipeStep::PdfLatex,
            ],
            builtin: true,
        },
    ]
}

#[derive(Debug, Clone)]
pub struct RecipeConfig {
    pub active_recipe_id: String,
    pub shell_escape: bool,
    pub custom_recipes: Vec<CompileRecipe>,
}

impl Default for RecipeConfig {
    fn default() -> Self {
        Self {
            active_recipe_id: "pdflatex".to_string(),
            shell_escape: false,
            custom_recipes: Vec::new(),
        }
    }
}

impl RecipeConfig {
    pub fn all_recipes(&self) -> Vec<CompileRecipe> {
        let mut all = builtin_recipes();
        all.extend(self.custom_recipes.iter().cloned());
        all
    }

    pub fn active_recipe(&self) -> CompileRecipe {
        let all = self.all_recipes();
        all.iter()
            .find(|r| r.id == self.active_recipe_id)
            .cloned()
            .unwrap_or_else(|| builtin_recipes().into_iter().next().unwrap())
    }

    pub fn add_custom_recipe(
        &mut self,
        name: String,
        steps: Vec<RecipeStep>,
    ) -> Option<String> {
        let clean_name: String = name
            .replace(['|', '\r', '\n'], " ")
            .trim()
            .to_string();
        if clean_name.is_empty() || steps.is_empty() {
            return None;
        }

        let existing = self.all_recipes();
        let mut n = 1usize;
        let id = loop {
            let cand = format!("custom_{n}");
            if !existing.iter().any(|r| r.id == cand) {
                break cand;
            }
            n += 1;
        };

        self.custom_recipes.push(CompileRecipe {
            id: id.clone(),
            name: clean_name,
            steps,
            builtin: false,
        });
        self.active_recipe_id = id.clone();
        Some(id)
    }

    pub fn remove_custom_recipe(&mut self, id: &str) -> bool {
        let before = self.custom_recipes.len();
        self.custom_recipes.retain(|r| r.id != id);
        let removed = self.custom_recipes.len() < before;
        if removed && self.active_recipe_id == id {
            self.active_recipe_id = "pdflatex".to_string();
        }
        removed
    }
}

fn storage_path() -> Option<PathBuf> {
    Some(config_dir()?.join("recipes.txt"))
}

pub fn serialize_config(cfg: &RecipeConfig) -> String {
    let mut out = String::new();
    out.push_str(&format!("active={}\n", cfg.active_recipe_id));
    out.push_str(&format!("shell_escape={}\n", cfg.shell_escape));
    for r in &cfg.custom_recipes {
        let safe_name = r.name.replace(['|', '\r', '\n'], " ");
        let steps_str = r
            .steps
            .iter()
            .map(|s| s.id())
            .collect::<Vec<_>>()
            .join(",");
        out.push_str(&format!("custom={}|{}|{}\n", r.id, safe_name, steps_str));
    }
    out
}

pub fn parse_config(raw: &str) -> RecipeConfig {
    let mut cfg = RecipeConfig::default();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(val) = trimmed.strip_prefix("active=") {
            let v = val.trim();
            if !v.is_empty() {
                cfg.active_recipe_id = v.to_string();
            }
        } else if let Some(val) = trimmed.strip_prefix("shell_escape=") {
            cfg.shell_escape = matches!(val.trim(), "true" | "1" | "yes");
        } else if let Some(val) = trimmed.strip_prefix("custom=") {
            let mut parts = val.splitn(3, '|');
            let (Some(id), Some(name), Some(steps_raw)) =
                (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let id = id.trim();
            let name = name.trim();
            if id.is_empty() || name.is_empty() {
                continue;
            }
            let steps: Vec<RecipeStep> = steps_raw
                .split(',')
                .filter_map(RecipeStep::from_id)
                .collect();
            if steps.is_empty() {
                continue;
            }
            if !cfg.custom_recipes.iter().any(|r| r.id == id) {
                cfg.custom_recipes.push(CompileRecipe {
                    id: id.to_string(),
                    name: name.to_string(),
                    steps,
                    builtin: false,
                });
            }
        }
    }
    // Ensure active_recipe_id points to a known recipe.
    let valid = cfg
        .all_recipes()
        .iter()
        .any(|r| r.id == cfg.active_recipe_id);
    if !valid {
        cfg.active_recipe_id = "pdflatex".to_string();
    }
    cfg
}

pub fn load_recipe_config() -> RecipeConfig {
    let Some(path) = storage_path() else {
        return RecipeConfig::default();
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return RecipeConfig::default();
    };
    parse_config(&raw)
}

pub fn save_recipe_config(cfg: &RecipeConfig) {
    let Some(path) = storage_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, serialize_config(cfg));
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MagicDirectives {
    pub root: Option<String>,
    pub program: Option<String>,
    pub bib_program: Option<String>,
}

fn normalize_program(raw: &str) -> Option<String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "pdflatex" => Some("pdflatex".to_string()),
        "lualatex" => Some("lualatex".to_string()),
        "xelatex" => Some("xelatex".to_string()),
        "latexmk" => Some("latexmk".to_string()),
        _ => None,
    }
}

fn normalize_bib_program(raw: &str) -> Option<String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "bibtex" => Some("bibtex".to_string()),
        "biber" => Some("biber".to_string()),
        _ => None,
    }
}

pub fn parse_magic_comments(source: &str) -> MagicDirectives {
    let mut out = MagicDirectives::default();
    for line in source.lines().take(30) {
        let trimmed = line.trim_start();
        let Some(after_pct) = trimmed.strip_prefix('%') else {
            continue;
        };
        let rest = after_pct.trim_start();
        let lower = rest.to_ascii_lowercase();

        if let Some(after_tag) = lower
            .strip_prefix("!tex")
            .or_else(|| lower.strip_prefix("!bib"))
        {
            let is_bib = lower.starts_with("!bib");
            // Recover original case slice matching `after_tag` length
            let orig_after = &rest[4..];
            let Some(eq_idx) = after_tag.find('=') else {
                continue;
            };
            let key = after_tag[..eq_idx].trim();
            let raw_val = orig_after[eq_idx + 1..].trim();
            let val = raw_val
                .trim_matches('"')
                .trim_matches('\'')
                .trim();
            if val.is_empty() {
                continue;
            }
            if is_bib {
                if matches!(key, "program" | "ts-program") && out.bib_program.is_none() {
                    out.bib_program = normalize_bib_program(val);
                }
            } else {
                match key {
                    "root" if out.root.is_none() => {
                        out.root = Some(val.to_string());
                    }
                    "program" | "ts-program" if out.program.is_none() => {
                        out.program = normalize_program(val);
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

pub fn resolve_root_path(current_tex: &Path, directives: &MagicDirectives) -> PathBuf {
    let Some(root_str) = directives.root.as_deref() else {
        return current_tex.to_path_buf();
    };
    let root_p = Path::new(root_str);
    let candidate = if root_p.is_absolute() {
        root_p.to_path_buf()
    } else if let Some(parent) = current_tex.parent() {
        parent.join(root_p)
    } else {
        root_p.to_path_buf()
    };
    if candidate.is_file() {
        std::fs::canonicalize(&candidate).unwrap_or(candidate)
    } else {
        current_tex.to_path_buf()
    }
}

pub fn effective_recipe_with_magic(
    base: &CompileRecipe,
    directives: &MagicDirectives,
) -> CompileRecipe {
    let mut result = base.clone();

    if let Some(prog) = directives.program.as_deref() {
        if prog == "latexmk" {
            let lmk_step = if result.steps.contains(&RecipeStep::LuaLatex)
                || result.steps.contains(&RecipeStep::LatexMkLua)
            {
                RecipeStep::LatexMkLua
            } else if result.steps.contains(&RecipeStep::XeLatex)
                || result.steps.contains(&RecipeStep::LatexMkXe)
            {
                RecipeStep::LatexMkXe
            } else {
                RecipeStep::LatexMkPdf
            };
            result.steps = vec![lmk_step];
            result.name = format!("{} (magic)", lmk_step.label());
        } else {
            let engine_step = match prog {
                "lualatex" => RecipeStep::LuaLatex,
                "xelatex" => RecipeStep::XeLatex,
                _ => RecipeStep::PdfLatex,
            };
            let mut replaced = false;
            for step in &mut result.steps {
                if step.is_tex_engine() {
                    *step = engine_step;
                    replaced = true;
                }
            }
            if !replaced {
                result.steps.insert(0, engine_step);
            }
            if result.steps.len() == 1 {
                result.name = engine_step.label().to_string();
            }
        }
    }

    if let Some(bib) = directives.bib_program.as_deref() {
        let bib_step = match bib {
            "biber" => RecipeStep::Biber,
            _ => RecipeStep::BibTex,
        };
        let has_latexmk = result.steps.iter().any(|s| {
            matches!(
                s,
                RecipeStep::LatexMkPdf | RecipeStep::LatexMkLua | RecipeStep::LatexMkXe
            )
        });
        if !has_latexmk {
            let has_bib = result
                .steps
                .iter()
                .any(|s| matches!(s, RecipeStep::BibTex | RecipeStep::Biber));
            if has_bib {
                for step in &mut result.steps {
                    if matches!(step, RecipeStep::BibTex | RecipeStep::Biber) {
                        *step = bib_step;
                    }
                }
            } else {
                let engine = result
                    .steps
                    .iter()
                    .copied()
                    .find(|s| s.is_tex_engine())
                    .unwrap_or(RecipeStep::PdfLatex);
                result.steps = vec![engine, bib_step, engine, engine];
                result.name = format!("{} + {}", engine.label(), bib_step.label());
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_magic_comments_case_insensitively() {
        let src = "\
% !TeX root = ../main.tex
% !tex TS-program = LuaLaTeX
% !BIB program = biber
\\section{Intro}
";
        let d = parse_magic_comments(src);
        assert_eq!(d.root.as_deref(), Some("../main.tex"));
        assert_eq!(d.program.as_deref(), Some("lualatex"));
        assert_eq!(d.bib_program.as_deref(), Some("biber"));

        let plain = "\\documentclass{article}\n\\begin{document}\nHi\n\\end{document}\n";
        assert_eq!(parse_magic_comments(plain), MagicDirectives::default());
    }

    #[test]
    fn config_round_trip_preserves_custom_recipes_and_flags() {
        let mut cfg = RecipeConfig::default();
        cfg.shell_escape = true;
        let id = cfg
            .add_custom_recipe(
                "Tesi | Completa".to_string(),
                vec![
                    RecipeStep::LuaLatex,
                    RecipeStep::Biber,
                    RecipeStep::MakeGlossaries,
                    RecipeStep::LuaLatex,
                ],
            )
            .expect("custom recipe added");

        let serialized = serialize_config(&cfg);
        let parsed = parse_config(&serialized);
        assert!(parsed.shell_escape);
        assert_eq!(parsed.active_recipe_id, id);
        assert_eq!(parsed.custom_recipes.len(), 1);
        assert_eq!(parsed.custom_recipes[0].name, "Tesi   Completa");
        assert_eq!(
            parsed.custom_recipes[0].steps,
            vec![
                RecipeStep::LuaLatex,
                RecipeStep::Biber,
                RecipeStep::MakeGlossaries,
                RecipeStep::LuaLatex,
            ]
        );
    }

    #[test]
    fn effective_recipe_applies_magic_overrides() {
        let base = builtin_recipes()
            .into_iter()
            .find(|r| r.id == "pdflatex")
            .unwrap();
        let d = MagicDirectives {
            root: None,
            program: Some("xelatex".to_string()),
            bib_program: Some("biber".to_string()),
        };
        let eff = effective_recipe_with_magic(&base, &d);
        assert_eq!(
            eff.steps,
            vec![
                RecipeStep::XeLatex,
                RecipeStep::Biber,
                RecipeStep::XeLatex,
                RecipeStep::XeLatex
            ]
        );
    }

    #[test]
    fn remove_custom_recipe_resets_active_to_pdflatex() {
        let mut cfg = RecipeConfig::default();
        let id = cfg
            .add_custom_recipe("My Recipe".to_string(), vec![RecipeStep::XeLatex])
            .unwrap();
        assert_eq!(cfg.active_recipe_id, id);
        assert!(cfg.remove_custom_recipe(&id));
        assert_eq!(cfg.active_recipe_id, "pdflatex");
        assert!(cfg.custom_recipes.is_empty());
    }
}

