use crate::recipes::{
    effective_recipe_with_magic, parse_magic_comments, resolve_root_path, CompileRecipe,
    RecipeStep,
};
use crate::symbols::MATH_LIBRARIES;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::task::JoinSet;

#[derive(Debug, Clone)]
pub enum RenderedPageData {
    Png(Vec<u8>),
    Rgba {
        width: u32,
        height: u32,
        bytes: Vec<u8>,
    },
}

#[derive(Debug, Clone)]
pub struct RenderedPage {
    pub page_number: usize, // 1-based
    pub data: RenderedPageData,
}

#[derive(Debug, Clone, Copy)]
pub struct PdfSyncTarget {
    pub page: usize,  // 1-based
    pub v_ratio: f32, // 0.0 (top) .. 1.0 (bottom) on the page
}

#[derive(Debug, Clone)]
pub struct ReverseSyncResult {
    pub file_path: PathBuf,
    pub line: usize, // 1-based
}

#[derive(Debug, Clone)]
pub struct CompileDiagnostic {
    pub line: Option<usize>, // 1-based
    pub title: String,
    pub explanation: String,
    pub raw_excerpt: String,
}

#[derive(Debug, Clone)]
pub struct CompileResult {
    pub success: bool,
    pub pdf_path: Option<PathBuf>,
    pub total_pages: usize,
    pub page_aspect_ratio: f32,
    pub page_width_pts: f32,
    pub page_height_pts: f32,
    pub initial_pages: Vec<RenderedPage>,
    pub sync_target: Option<PdfSyncTarget>,
    pub diagnostics: Vec<CompileDiagnostic>,
    pub preview_note: Option<String>,
    pub log: String,
    pub elapsed_secs: f32,
}

/// Checks via a single `kpsewhich` call which LaTeX packages in `MATH_LIBRARIES`
/// are actually installed in the user's TeX Live distribution.
pub async fn detect_installed_packages() -> Option<HashSet<String>> {
    let sty_names: Vec<String> = MATH_LIBRARIES
        .iter()
        .map(|lib| format!("{}.sty", lib.package))
        .collect();

    let output = Command::new("kpsewhich")
        .args(&sty_names)
        .output()
        .await
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut installed = HashSet::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(stem) = Path::new(trimmed).file_stem().and_then(|s| s.to_str()) {
            installed.insert(stem.to_string());
        }
    }
    Some(installed)
}

/// Deterministic 64-bit FNV-1a hash used to fingerprint preambles and live buffers.
pub fn text_fingerprint(s: &str) -> u64 {
    fnv1a_64(s.as_bytes())
}

fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Splits a LaTeX source into `(preamble, body_with_exact_line_numbers)` at `\begin{document}`.
/// Replacing the `N` preamble lines with `N` `%` comment lines ensures all compiler errors and
/// SyncTeX forward/reverse line numbers remain 100% identical to the original `.tex` file.
fn split_preamble_and_body(source: &str) -> Option<(String, String)> {
    let lines: Vec<&str> = source.lines().collect();
    let mut begin_idx = None;
    for (idx, line) in lines.iter().enumerate() {
        let active = line.split('%').next().unwrap_or("");
        if active.contains("\\begin{document}") {
            begin_idx = Some(idx);
            break;
        }
    }
    let begin_idx = begin_idx?;
    if begin_idx == 0 {
        return None;
    }

    let preamble = lines[..begin_idx].join("\n");
    if !preamble.contains("\\documentclass")
        || preamble.contains("\\endofdump")
        || preamble.contains("\\write18")
    {
        return None;
    }

    let mut body = String::with_capacity(source.len());
    for _ in 0..begin_idx {
        body.push_str("%\n");
    }
    for line in &lines[begin_idx..] {
        body.push_str(line);
        body.push('\n');
    }
    Some((preamble, body))
}

/// Ensures a precompiled binary format `.build/preamble_<hash>.fmt` exists for `preamble`.
/// Built once per unique preamble via `pdflatex -ini` (~0.8s), then reused across all manual
/// and live compiles to skip reloading `article.cls`, `amsmath`, `tikz`, etc.
async fn ensure_preamble_fmt(
    parent_dir: &Path,
    build_dir: &Path,
    preamble: &str,
) -> Option<String> {
    let hash = fnv1a_64(preamble.as_bytes());
    let fmt_name = format!("preamble_{hash:016x}");
    let fmt_file = build_dir.join(format!("{fmt_name}.fmt"));
    if fmt_file.exists() {
        return Some(fmt_name);
    }

    // Remove outdated preamble caches when the user modifies their preamble.
    if let Ok(mut entries) = tokio::fs::read_dir(build_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name();
            let s = name.to_string_lossy();
            if s.starts_with("preamble_") && !s.starts_with(&fmt_name) {
                let _ = tokio::fs::remove_file(entry.path()).await;
            }
        }
    }

    let fmt_tex = build_dir.join(format!("{fmt_name}.tex"));
    let fmt_tex_rel = format!(".build/{fmt_name}.tex");
    let dump_src = format!("{preamble}\n\\dump\n");
    tokio::fs::write(&fmt_tex, dump_src).await.ok()?;

    let job_arg = format!("-jobname={fmt_name}");
    let out = Command::new("pdflatex")
        .current_dir(parent_dir)
        .kill_on_drop(true)
        .args([
            "-ini",
            "-interaction=batchmode",
            "-output-directory=.build",
            &job_arg,
            "&pdflatex",
            &fmt_tex_rel,
        ])
        .output()
        .await
        .ok()?;

    if out.status.success() && fmt_file.exists() {
        Some(fmt_name)
    } else {
        let _ = tokio::fs::remove_file(&fmt_file).await;
        None
    }
}

/// Asynchronously precompiles the document's preamble into `.build/preamble_<hash>.fmt`
/// right when a file is opened, so even the very first compile is accelerated.
pub async fn preload_preamble_fmt(tex_path: PathBuf, contents: String) {
    let Some(parent_dir) = tex_path.parent() else {
        return;
    };
    let Some((preamble, _)) = split_preamble_and_body(&contents) else {
        return;
    };
    let build_dir = parent_dir.join(".build");
    if tokio::fs::create_dir_all(&build_dir).await.is_err() {
        return;
    }
    let _ = ensure_preamble_fmt(parent_dir, &build_dir, &preamble).await;
}

const AUX_EXTENSIONS: &[&str] = &["aux", "toc", "out", "lof", "lot", "nav", "snm", "vrb"];

/// Returns a RAM-disk live build directory in `/dev/shm` when available (eliminating disk I/O
/// while typing), falling back to `<project>/.build/live` otherwise.
#[cfg(unix)]
fn current_uid() -> u32 {
    extern "C" {
        fn getuid() -> u32;
    }
    unsafe { getuid() }
}

fn live_dir_for(parent_dir: &Path) -> PathBuf {
    let shm = Path::new("/dev/shm");
    if shm.is_dir() {
        let canon = std::fs::canonicalize(parent_dir).unwrap_or_else(|_| parent_dir.to_path_buf());
        let hash = fnv1a_64(canon.to_string_lossy().as_bytes());
        #[cfg(unix)]
        let name = format!("feathertex_live_{}_{hash:016x}", current_uid());
        #[cfg(not(unix))]
        let name = format!("feathertex_live_{hash:016x}");
        shm.join(name)
    } else {
        parent_dir.join(".build").join("live")
    }
}

/// Atomically mirrors the built PDF from `.build/<stem>.pdf` to `<project_dir>/<stem>.pdf`
/// so users always see their compiled PDF right beside their `.tex` file (like a standard editor),
/// while keeping auxiliary files tidy inside `.build/`.
fn mirror_pdf_to_project_dir(build_pdf: &Path, project_dir: &Path, file_name: &std::ffi::OsStr) {
    if !build_pdf.exists() {
        return;
    }
    let user_pdf = project_dir.join(file_name);
    if let Ok(bytes) = std::fs::read(build_pdf) {
        let _ = crate::document::write_file_atomic_sync(&user_pdf, &bytes);
    }
}

/// Atomically moves `src` to `dst`, transparently falling back to copy + sibling-temp atomic
/// rename when crossing filesystems (e.g., `/dev/shm` tmpfs -> disk `.build/`).
fn move_file_atomic_or_copy(src: &Path, dst: &Path) -> std::io::Result<()> {
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    let tmp = dst.with_file_name(format!(
        ".{}.promote_tmp",
        dst.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("feathertex")
    ));
    std::fs::copy(src, &tmp)?;
    std::fs::rename(&tmp, dst)?;
    let _ = std::fs::remove_file(src);
    Ok(())
}

/// Seeds `live_dir` with newer/missing `.aux`, `.toc`, `.out`, etc. from `.build/` so
/// background compiles avoid redundant 2nd `pdflatex` passes for existing cross-references.
fn sync_aux_files_to_live(build_dir: &Path, live_dir: &Path, stem: &str) {
    for ext in AUX_EXTENSIONS {
        let src = build_dir.join(format!("{stem}.{ext}"));
        if !src.exists() {
            continue;
        }
        let dst = live_dir.join(format!("{stem}.{ext}"));
        let should_copy = match (std::fs::metadata(&src), std::fs::metadata(&dst)) {
            (Ok(s_meta), Ok(d_meta)) => s_meta
                .modified()
                .ok()
                .zip(d_meta.modified().ok())
                .is_none_or(|(sm, dm)| sm > dm),
            (Ok(_), Err(_)) => true,
            _ => false,
        };
        if should_copy {
            let _ = std::fs::copy(&src, &dst);
        }
    }
}

async fn run_tex_engine_pass(
    engine: &str,
    parent_dir: &Path,
    file_name: &std::ffi::OsStr,
    shell_escape: bool,
) -> Result<std::process::Output, String> {
    let mut cmd = Command::new(engine);
    cmd.current_dir(parent_dir)
        .kill_on_drop(true)
        .args([
            "-synctex=-1",
            "-interaction=batchmode",
            "-file-line-error",
            "-output-directory=.build",
        ]);
    if shell_escape {
        cmd.arg("-shell-escape");
    }
    cmd.arg(file_name)
        .output()
        .await
        .map_err(|e| {
            format!("Impossibile avviare {engine} ({e}). Verifica che TeX Live sia nel PATH.")
        })
}

async fn run_bibtex_step(
    parent_dir: &Path,
    build_dir: &Path,
    stem: &str,
) -> Result<std::process::Output, String> {
    let parent_str = format!("{}:", parent_dir.display());
    Command::new("bibtex")
        .current_dir(build_dir)
        .kill_on_drop(true)
        .env("BIBINPUTS", &parent_str)
        .env("BSTINPUTS", &parent_str)
        .arg(stem)
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "bibtex non è installato nel sistema. Installa texlive-binextra / texlive-core."
                    .to_string()
            } else {
                format!("Impossibile avviare bibtex ({e}).")
            }
        })
}

async fn run_biber_step(parent_dir: &Path, stem: &str) -> Result<std::process::Output, String> {
    let parent_str = format!("{}:", parent_dir.display());
    Command::new("biber")
        .current_dir(parent_dir)
        .kill_on_drop(true)
        .env("BIBINPUTS", &parent_str)
        .args(["--output-directory=.build", stem])
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "biber non è installato nel sistema. Installalo con: sudo pacman -S biber (oppure usa la ricetta pdfLaTeX + BibTeX)."
                    .to_string()
            } else {
                format!("Impossibile avviare biber ({e}).")
            }
        })
}

async fn run_makeglossaries_step(
    parent_dir: &Path,
    stem: &str,
) -> Result<std::process::Output, String> {
    Command::new("makeglossaries")
        .current_dir(parent_dir)
        .kill_on_drop(true)
        .args(["-d", ".build", stem])
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "makeglossaries non è installato nel sistema (pacchetto texlive-binextra)."
                    .to_string()
            } else {
                format!("Impossibile avviare makeglossaries ({e}).")
            }
        })
}

async fn run_makeindex_step(build_dir: &Path, stem: &str) -> Result<std::process::Output, String> {
    let idx_name = format!("{stem}.idx");
    Command::new("makeindex")
        .current_dir(build_dir)
        .kill_on_drop(true)
        .arg(&idx_name)
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "makeindex non è installato nel sistema.".to_string()
            } else {
                format!("Impossibile avviare makeindex ({e}).")
            }
        })
}

async fn run_latexmk_step(
    engine_flag: &str,
    parent_dir: &Path,
    file_name: &std::ffi::OsStr,
    shell_escape: bool,
) -> Result<std::process::Output, String> {
    let mut cmd = Command::new("latexmk");
    cmd.current_dir(parent_dir)
        .kill_on_drop(true)
        .args([
            engine_flag,
            "-synctex=-1",
            "-interaction=batchmode",
            "-file-line-error",
            "-outdir=.build",
        ]);
    if shell_escape {
        cmd.arg("-shell-escape");
    }
    cmd.arg(file_name).output().await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "latexmk non è installato nel sistema. Installalo con: sudo pacman -S texlive-binextra"
                .to_string()
        } else {
            format!("Impossibile avviare latexmk ({e}).")
        }
    })
}

async fn run_pdflatex_with_fmt(
    parent_dir: &Path,
    out_dir_arg: &str,
    fmt_name: &str,
    stem: &str,
    body_tex_arg: &str,
) -> Result<std::process::Output, String> {
    let out_arg = format!("-output-directory={out_dir_arg}");
    let fmt_arg = format!("-fmt=.build/{fmt_name}");
    let job_arg = format!("-jobname={stem}");
    Command::new("pdflatex")
        .current_dir(parent_dir)
        .kill_on_drop(true)
        .args([
            "-synctex=-1",
            "-interaction=batchmode",
            "-file-line-error",
            &out_arg,
            &fmt_arg,
            &job_arg,
            body_tex_arg,
        ])
        .output()
        .await
        .map_err(|e| {
            format!("Impossibile avviare pdflatex ({e}). Verifica che TeX Live sia nel PATH.")
        })
}

async fn run_pdflatex_live_pass(
    parent_dir: &Path,
    out_dir_arg: &str,
    stem: &str,
    live_tex_arg: &str,
) -> Result<std::process::Output, String> {
    let out_arg = format!("-output-directory={out_dir_arg}");
    let job_arg = format!("-jobname={stem}");
    Command::new("pdflatex")
        .current_dir(parent_dir)
        .kill_on_drop(true)
        .args([
            "-synctex=-1",
            "-interaction=batchmode",
            "-file-line-error",
            &out_arg,
            &job_arg,
            live_tex_arg,
        ])
        .output()
        .await
        .map_err(|e| {
            format!("Impossibile avviare pdflatex ({e}). Verifica che TeX Live sia nel PATH.")
        })
}

/// Asynchronous shadow live compilation:
/// - Waits `debounce_ms` so rapid keystrokes coalesce before spawning `pdflatex`.
/// - Uses the cached precompiled preamble `.fmt` when available (skipping package reloads).
/// - Compiles into RAM (`/dev/shm` when available) without touching the user's `.tex` or visible `.pdf`.
/// - Skips PDF metadata, SyncTeX, and page rasterization during background typing (done on promote
///   in ~15 ms via in-process Poppler+Cairo).
pub async fn compile_latex_live(
    tex_path: PathBuf,
    unsaved_contents: String,
    debounce_ms: u64,
) -> Result<CompileResult, String> {
    if debounce_ms > 0 {
        tokio::time::sleep(Duration::from_millis(debounce_ms)).await;
    }

    let started = Instant::now();
    let parent_dir = tex_path
        .parent()
        .ok_or_else(|| "Impossibile determinare la cartella del file".to_string())?;
    let stem = tex_path
        .file_stem()
        .ok_or_else(|| "Nome base del file non valido".to_string())?
        .to_string_lossy()
        .to_string();

    let build_dir = parent_dir.join(".build");
    let live_dir = live_dir_for(parent_dir);
    tokio::fs::create_dir_all(&build_dir)
        .await
        .map_err(|e| format!("Impossibile creare .build: {e}"))?;
    tokio::fs::create_dir_all(&live_dir)
        .await
        .map_err(|e| format!("Impossibile creare directory live: {e}"))?;

    let canon_parent = std::fs::canonicalize(parent_dir).unwrap_or_else(|_| parent_dir.to_path_buf());
    let _ = tokio::fs::write(live_dir.join(".origin"), canon_parent.to_string_lossy().as_bytes()).await;
    sync_aux_files_to_live(&build_dir, &live_dir, &stem);

    let live_tex_path = live_dir.join(format!("{stem}.tex"));
    let live_dir_str = live_dir.to_string_lossy().to_string();
    let live_tex_str = live_tex_path.to_string_lossy().to_string();

    let fmt_opt = match split_preamble_and_body(&unsaved_contents) {
        Some((preamble, body)) => match ensure_preamble_fmt(parent_dir, &build_dir, &preamble).await
        {
            Some(fmt_name) => {
                tokio::fs::write(&live_tex_path, body)
                    .await
                    .map_err(|e| format!("Impossibile scrivere file temporaneo live: {e}"))?;
                Some(fmt_name)
            }
            None => {
                tokio::fs::write(&live_tex_path, &unsaved_contents)
                    .await
                    .map_err(|e| format!("Impossibile scrivere file temporaneo live: {e}"))?;
                None
            }
        },
        None => {
            tokio::fs::write(&live_tex_path, &unsaved_contents)
                .await
                .map_err(|e| format!("Impossibile scrivere file temporaneo live: {e}"))?;
            None
        }
    };

    let mut output = match &fmt_opt {
        Some(fmt) => {
            run_pdflatex_with_fmt(parent_dir, &live_dir_str, fmt, &stem, &live_tex_str).await?
        }
        None => run_pdflatex_live_pass(parent_dir, &live_dir_str, &stem, &live_tex_str).await?,
    };
    let log_file = live_dir.join(format!("{stem}.log"));

    if output.status.success() {
        if let Some(pass1_log) = read_log_lossy(&log_file).await {
            if log_requests_rerun(&pass1_log) {
                output = match &fmt_opt {
                    Some(fmt) => {
                        run_pdflatex_with_fmt(parent_dir, &live_dir_str, fmt, &stem, &live_tex_str)
                            .await?
                    }
                    None => {
                        run_pdflatex_live_pass(parent_dir, &live_dir_str, &stem, &live_tex_str)
                            .await?
                    }
                };
            }
        }
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let live_pdf = live_dir.join(format!("{stem}.pdf"));
    // Keep the compiled PDF staged in `live_dir` WITHOUT overwriting `.build/<stem>.pdf`
    // so the user's visible PDF stays untouched until they press Compila / Ctrl+B.
    let pdf_path = if output.status.success() && live_pdf.exists() {
        Some(live_pdf)
    } else {
        None
    };

    let mut diagnostics = Vec::new();
    let mut log_text = String::new();
    if !output.status.success() {
        let raw_log = read_log_lossy(&log_file).await;
        let combined = match &raw_log {
            Some(r) => format!("{r}\n{stdout}"),
            None => stdout.to_string(),
        };
        diagnostics = match &raw_log {
            Some(r) => {
                let from_log = parse_diagnostics(r);
                if from_log.is_empty() {
                    parse_diagnostics(&stdout)
                } else {
                    from_log
                }
            }
            None => parse_diagnostics(&stdout),
        };
        if let Some(hint) = diagnose_missing_packages(&combined) {
            log_text.push_str(&hint);
            log_text.push_str("\n\n");
        }
        if let Some(raw_log) = raw_log {
            let extracted = extract_errors(&raw_log);
            if !extracted.is_empty() {
                log_text.push_str("=== Errori rilevati da pdflatex ===\n");
                log_text.push_str(&extracted);
                log_text.push_str("\n\n");
            }
            log_text.push_str("=== Log completo pdflatex ===\n");
            log_text.push_str(raw_log.trim());
        }
    }

    if !stdout.trim().is_empty() && log_text.is_empty() {
        log_text.push_str(stdout.trim());
    }
    if !stderr.trim().is_empty() {
        if !log_text.is_empty() {
            log_text.push('\n');
        }
        log_text.push_str(stderr.trim());
    }

    Ok(CompileResult {
        success: output.status.success(),
        pdf_path,
        total_pages: 0,
        page_aspect_ratio: 1.4142,
        page_width_pts: 595.28,
        page_height_pts: 841.89,
        initial_pages: Vec::new(),
        sync_target: None,
        diagnostics,
        preview_note: None,
        log: if log_text.trim().is_empty() {
            "Compilazione completata.".to_string()
        } else {
            log_text
        },
        elapsed_secs: started.elapsed().as_secs_f32(),
    })
}

/// Atomically promotes a staged live `.pdf`, `.synctex`, and auxiliary files (`.aux`, `.toc`, ...)
/// from the live directory (`/dev/shm` or `.build/live/`) into the project's `.build/` folder.
pub fn promote_staged_live_result(mut result: CompileResult) -> Result<CompileResult, String> {
    if !result.success {
        return Ok(result);
    }
    let Some(live_pdf) = result.pdf_path.clone() else {
        return Ok(result);
    };
    let Some(file_name) = live_pdf.file_name() else {
        return Ok(result);
    };
    let Some(live_dir) = live_pdf.parent() else {
        return Ok(result);
    };

    let build_dir = if let Ok(origin) = std::fs::read_to_string(live_dir.join(".origin")) {
        let trimmed = origin.trim();
        if !trimmed.is_empty() {
            PathBuf::from(trimmed).join(".build")
        } else {
            live_dir
                .parent()
                .ok_or_else(|| "Cartella .build non trovata".to_string())?
                .to_path_buf()
        }
    } else {
        live_dir
            .parent()
            .ok_or_else(|| "Cartella .build non trovata".to_string())?
            .to_path_buf()
    };

    std::fs::create_dir_all(&build_dir)
        .map_err(|e| format!("Impossibile creare .build per la promozione: {e}"))?;

    let final_pdf = build_dir.join(file_name);
    if live_pdf.exists() {
        move_file_atomic_or_copy(&live_pdf, &final_pdf)
            .map_err(|e| format!("Impossibile promuovere il PDF pre-compilato: {e}"))?;
        if let Some(proj_dir) = build_dir.parent() {
            mirror_pdf_to_project_dir(&final_pdf, proj_dir, file_name);
        }
    }

    if let Some(stem) = live_pdf.file_stem().and_then(|s| s.to_str()) {
        let live_synctex = live_dir.join(format!("{stem}.synctex"));
        let final_synctex = build_dir.join(format!("{stem}.synctex"));
        if live_synctex.exists() {
            let _ = move_file_atomic_or_copy(&live_synctex, &final_synctex);
        }
        for ext in AUX_EXTENSIONS {
            let live_aux = live_dir.join(format!("{stem}.{ext}"));
            if live_aux.exists() {
                let final_aux = build_dir.join(format!("{stem}.{ext}"));
                let _ = std::fs::copy(&live_aux, &final_aux);
            }
        }
    }

    result.pdf_path = Some(final_pdf);
    Ok(result)
}

/// Promotes a staged live build into `.build/` and immediately populates metadata, SyncTeX
/// target, and visible page textures (in ~15-25 ms using in-process Poppler+Cairo).
pub async fn finalize_promoted_live_result(
    result: CompileResult,
    tex_path: PathBuf,
    sync_line: usize,
    fallback_page: usize,
    target_width_px: u32,
) -> Result<CompileResult, String> {
    let mut promoted = promote_staged_live_result(result)?;
    if !promoted.success {
        return Ok(promoted);
    }
    let Some(pdf) = promoted.pdf_path.clone() else {
        return Ok(promoted);
    };

    match pdf_metadata(&pdf).await {
        Ok((pages_count, aspect, width_pts, height_pts)) if pages_count > 0 => {
            let sync_target =
                synctex_forward_search(&tex_path, &pdf, sync_line, height_pts, pages_count).await;
            let anchor_page = sync_target
                .map(|t| t.page)
                .unwrap_or(fallback_page)
                .clamp(1, pages_count);
            let start = anchor_page.saturating_sub(1).max(1);
            let end = (anchor_page + 1).min(pages_count);
            let to_render: Vec<usize> = (start..=end).collect();
            let target_h = ((target_width_px as f32) * aspect).round().max(100.0) as u32;

            promoted.total_pages = pages_count;
            promoted.page_aspect_ratio = aspect;
            promoted.page_width_pts = width_pts;
            promoted.page_height_pts = height_pts;
            promoted.sync_target = sync_target;

            match render_pages_parallel(pdf, to_render, target_width_px, target_h).await {
                Ok(rendered) => {
                    promoted.initial_pages = rendered;
                    promoted.preview_note = None;
                }
                Err(err) => {
                    promoted.initial_pages.clear();
                    promoted.preview_note = Some(err);
                }
            }
        }
        Ok(_) => {}
        Err(err) => {
            promoted.preview_note = Some(err);
        }
    }

    Ok(promoted)
}

/// Reads a `pdflatex` `.log` file tolerating invalid UTF-8 sequences (e.g., when pdfTeX
/// hard-wraps a log line at 79 bytes in the middle of a multi-byte accented UTF-8 character).
async fn read_log_lossy(path: &Path) -> Option<String> {
    tokio::fs::read(path)
        .await
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

fn log_requests_rerun(log: &str) -> bool {
    log.contains("Rerun to get")
        || log.contains("Label(s) may have changed")
        || log.contains("Table widths have changed")
        || (log.contains("No file ") && (log.contains(".toc.") || log.contains(".aux.")))
}

fn append_tex_failure_logs(
    log_text: &mut String,
    engine_name: &str,
    raw_log: Option<&str>,
    stdout: &str,
    stderr: &str,
    diagnostics: &mut Vec<CompileDiagnostic>,
) {
    let combined = match raw_log {
        Some(r) => format!("{r}\n{stdout}"),
        None => stdout.to_string(),
    };
    let parsed = match raw_log {
        Some(r) => {
            let from_log = parse_diagnostics(r);
            if from_log.is_empty() {
                parse_diagnostics(stdout)
            } else {
                from_log
            }
        }
        None => parse_diagnostics(stdout),
    };
    diagnostics.extend(parsed);

    if let Some(hint) = diagnose_missing_packages(&combined) {
        log_text.push_str(&hint);
        log_text.push_str("\n\n");
    }
    if let Some(r) = raw_log {
        let extracted = extract_errors(r);
        if !extracted.is_empty() {
            log_text.push_str(&format!("=== Errori rilevati da {engine_name} ===\n"));
            log_text.push_str(&extracted);
            log_text.push_str("\n\n");
        }
        log_text.push_str(&format!("=== Log completo {engine_name} ===\n"));
        log_text.push_str(r.trim());
        log_text.push('\n');
    } else if !stdout.trim().is_empty() {
        log_text.push_str(stdout.trim());
        log_text.push('\n');
    }
    if !stderr.trim().is_empty() {
        log_text.push_str(stderr.trim());
        log_text.push('\n');
    }
}

fn log_needs_bibliography(log: &str, stem: &str) -> bool {
    let missing_bbl = format!("No file {stem}.bbl.");
    log.contains(&missing_bbl)
        || log.contains("Citation `")
        || log.contains("Citation '")
        || log.contains("Please (re)run Biber")
        || log.contains("Please (re)run BibTeX")
}

pub async fn compile_with_recipe(
    tex_path: PathBuf,
    master_override: Option<PathBuf>,
    recipe: CompileRecipe,
    shell_escape: bool,
    sync_line: usize,
    fallback_page: usize,
    target_width_px: u32,
) -> Result<CompileResult, String> {
    let started = Instant::now();

    let origin_source = read_log_lossy(&tex_path).await.unwrap_or_default();
    let magic = parse_magic_comments(&origin_source);
    let compile_tex_path = if magic.root.is_some() {
        resolve_root_path(&tex_path, &magic)
    } else if let Some(master) = master_override.filter(|p| p.is_file()) {
        std::fs::canonicalize(&master).unwrap_or(master)
    } else {
        tex_path.clone()
    };
    let effective_recipe = effective_recipe_with_magic(&recipe, &magic);

    let parent_dir = compile_tex_path
        .parent()
        .ok_or_else(|| "Impossibile determinare la cartella del file".to_string())?;

    let file_name = compile_tex_path
        .file_name()
        .ok_or_else(|| "Nome file non valido".to_string())?;

    let stem = compile_tex_path
        .file_stem()
        .ok_or_else(|| "Nome base del file non valido".to_string())?
        .to_string_lossy()
        .to_string();

    let build_dir = parent_dir.join(".build");
    tokio::fs::create_dir_all(&build_dir)
        .await
        .map_err(|e| format!("Impossibile creare la cartella .build: {e}"))?;

    let steps = if effective_recipe.steps.is_empty() {
        vec![RecipeStep::PdfLatex]
    } else {
        effective_recipe.steps.clone()
    };
    let is_single_tex_pass = steps.len() == 1
        && matches!(
            steps[0],
            RecipeStep::PdfLatex | RecipeStep::LuaLatex | RecipeStep::XeLatex
        );

    let body_tex_path = build_dir.join(format!("{stem}_body.tex"));
    let body_tex_rel = format!(".build/{stem}_body.tex");
    let mut prepared_fmt: Option<Option<String>> = None;
    let mut last_used_body_path: Option<PathBuf> = None;

    let log_file = build_dir.join(format!("{stem}.log"));
    let blg_file = build_dir.join(format!("{stem}.blg"));
    let mut diagnostics = Vec::new();
    let mut log_text = String::new();
    if compile_tex_path != tex_path {
        log_text.push_str(&format!(
            "=== !TeX root attivo: {} ===\n",
            compile_tex_path.display()
        ));
    }

    let mut pipeline_ok = true;
    let total_steps = steps.len();

    for (idx, step) in steps.iter().copied().enumerate() {
        if !log_text.is_empty() && !log_text.ends_with('\n') {
            log_text.push('\n');
        }
        log_text.push_str(&format!(
            "=== [{}/{}] {} ===\n",
            idx + 1,
            total_steps,
            step.label()
        ));

        match step {
            RecipeStep::PdfLatex => {
                let fmt_opt = if shell_escape {
                    None
                } else {
                    if prepared_fmt.is_none() {
                        let root_src = read_log_lossy(&compile_tex_path).await;
                        let computed = match root_src
                            .as_deref()
                            .and_then(split_preamble_and_body)
                        {
                            Some((preamble, body)) => {
                                match ensure_preamble_fmt(parent_dir, &build_dir, &preamble).await {
                                    Some(fmt_name) => {
                                        if tokio::fs::write(&body_tex_path, body).await.is_ok() {
                                            Some(fmt_name)
                                        } else {
                                            None
                                        }
                                    }
                                    None => None,
                                }
                            }
                            None => None,
                        };
                        prepared_fmt = Some(computed);
                    }
                    prepared_fmt.as_ref().and_then(|o| o.as_ref()).cloned()
                };

                let mut output = match &fmt_opt {
                    Some(fmt) => {
                        last_used_body_path = Some(body_tex_path.clone());
                        run_pdflatex_with_fmt(parent_dir, ".build", fmt, &stem, &body_tex_rel).await?
                    }
                    None => {
                        last_used_body_path = None;
                        run_tex_engine_pass("pdflatex", parent_dir, file_name, shell_escape).await?
                    }
                };

                if is_single_tex_pass && output.status.success() {
                    if let Some(pass1_log) = read_log_lossy(&log_file).await {
                        let mut did_auto_bib = false;
                        if log_needs_bibliography(&pass1_log, &stem) {
                            let bcf_path = build_dir.join(format!("{stem}.bcf"));
                            let aux_path = build_dir.join(format!("{stem}.aux"));
                            if bcf_path.exists() {
                                if let Ok(bib_out) = run_biber_step(parent_dir, &stem).await {
                                    if bib_out.status.success() {
                                        log_text.push_str("=== [auto] Biber ===\n");
                                        did_auto_bib = true;
                                    }
                                }
                            } else if let Some(aux_txt) = read_log_lossy(&aux_path).await {
                                if aux_txt.contains("\\bibdata{") && aux_txt.contains("\\citation{")
                                {
                                    if let Ok(bib_out) =
                                        run_bibtex_step(parent_dir, &build_dir, &stem).await
                                    {
                                        if bib_out.status.success() {
                                            log_text.push_str("=== [auto] BibTeX ===\n");
                                            did_auto_bib = true;
                                        }
                                    }
                                }
                            }
                        }

                        if did_auto_bib {
                            for _ in 0..2 {
                                output = match &fmt_opt {
                                    Some(fmt) => {
                                        run_pdflatex_with_fmt(
                                            parent_dir,
                                            ".build",
                                            fmt,
                                            &stem,
                                            &body_tex_rel,
                                        )
                                        .await?
                                    }
                                    None => {
                                        run_tex_engine_pass(
                                            "pdflatex",
                                            parent_dir,
                                            file_name,
                                            shell_escape,
                                        )
                                        .await?
                                    }
                                };
                                if !output.status.success() {
                                    break;
                                }
                            }
                        } else if log_requests_rerun(&pass1_log) {
                            output = match &fmt_opt {
                                Some(fmt) => {
                                    run_pdflatex_with_fmt(
                                        parent_dir,
                                        ".build",
                                        fmt,
                                        &stem,
                                        &body_tex_rel,
                                    )
                                    .await?
                                }
                                None => {
                                    run_tex_engine_pass(
                                        "pdflatex",
                                        parent_dir,
                                        file_name,
                                        shell_escape,
                                    )
                                    .await?
                                }
                            };
                        }
                    }
                }

                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                if output.status.success() {
                    if !stdout.trim().is_empty() {
                        log_text.push_str(stdout.trim());
                        log_text.push('\n');
                    }
                } else {
                    let raw_log = read_log_lossy(&log_file).await;
                    append_tex_failure_logs(
                        &mut log_text,
                        "pdflatex",
                        raw_log.as_deref(),
                        &stdout,
                        &stderr,
                        &mut diagnostics,
                    );
                    pipeline_ok = false;
                    break;
                }
            }
            RecipeStep::LuaLatex | RecipeStep::XeLatex => {
                last_used_body_path = None;
                let engine = if step == RecipeStep::LuaLatex {
                    "lualatex"
                } else {
                    "xelatex"
                };
                let mut output =
                    run_tex_engine_pass(engine, parent_dir, file_name, shell_escape).await?;
                if is_single_tex_pass && output.status.success() {
                    if let Some(pass1_log) = read_log_lossy(&log_file).await {
                        if log_requests_rerun(&pass1_log) {
                            output =
                                run_tex_engine_pass(engine, parent_dir, file_name, shell_escape)
                                    .await?;
                        }
                    }
                }

                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                if output.status.success() {
                    if !stdout.trim().is_empty() {
                        log_text.push_str(stdout.trim());
                        log_text.push('\n');
                    }
                } else {
                    let raw_log = read_log_lossy(&log_file).await;
                    append_tex_failure_logs(
                        &mut log_text,
                        engine,
                        raw_log.as_deref(),
                        &stdout,
                        &stderr,
                        &mut diagnostics,
                    );
                    pipeline_ok = false;
                    break;
                }
            }
            RecipeStep::BibTex => {
                let output = run_bibtex_step(parent_dir, &build_dir, &stem).await?;
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let blg_raw = read_log_lossy(&blg_file).await.unwrap_or_default();
                let combined_bib = if stdout.trim().is_empty() {
                    blg_raw.clone()
                } else {
                    stdout.to_string()
                };

                if !combined_bib.trim().is_empty() {
                    log_text.push_str(combined_bib.trim());
                    log_text.push('\n');
                }
                if !stderr.trim().is_empty() {
                    log_text.push_str(stderr.trim());
                    log_text.push('\n');
                }

                if !output.status.success() {
                    let only_missing_bib = (combined_bib.contains("I found no \\citation commands")
                        || combined_bib.contains("I found no \\bibdata command")
                        || combined_bib.contains("I found no \\bibstyle command"))
                        && !combined_bib.contains("error message");
                    if only_missing_bib {
                        log_text.push_str(
                            "(Nota: nessuna citazione o \\bibliography trovata nel documento — passo BibTeX ignorato.)\n",
                        );
                    } else {
                        diagnostics.push(CompileDiagnostic {
                            line: None,
                            title: "Errore BibTeX nella bibliografia".to_string(),
                            explanation: "Controlla i file .bib, le chiavi \\cite{...} e lo \\bibliographystyle{...}.".to_string(),
                            raw_excerpt: combined_bib
                                .lines()
                                .take(8)
                                .collect::<Vec<_>>()
                                .join("\n"),
                        });
                        pipeline_ok = false;
                        break;
                    }
                }
            }
            RecipeStep::Biber => {
                let output = run_biber_step(parent_dir, &stem).await?;
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stdout.trim().is_empty() {
                    log_text.push_str(stdout.trim());
                    log_text.push('\n');
                }
                if !stderr.trim().is_empty() {
                    log_text.push_str(stderr.trim());
                    log_text.push('\n');
                }

                if !output.status.success() {
                    let combined_biber = format!("{stdout}\n{stderr}");
                    let title = if combined_biber.contains(".bcf")
                        && combined_biber.contains("not found")
                    {
                        "Biber: file .bcf non trovato (usa \\usepackage{biblatex})".to_string()
                    } else {
                        "Errore Biber nella bibliografia".to_string()
                    };
                    diagnostics.push(CompileDiagnostic {
                        line: None,
                        title,
                        explanation: "Verifica che il preambolo usi biblatex, che il file .bib sia valido e che non ci siano errori di sintassi.".to_string(),
                        raw_excerpt: combined_biber
                            .lines()
                            .take(8)
                            .collect::<Vec<_>>()
                            .join("\n"),
                    });
                    pipeline_ok = false;
                    break;
                }
            }
            RecipeStep::MakeGlossaries => {
                let output = run_makeglossaries_step(parent_dir, &stem).await?;
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stdout.trim().is_empty() {
                    log_text.push_str(stdout.trim());
                    log_text.push('\n');
                }
                if !stderr.trim().is_empty() {
                    log_text.push_str(stderr.trim());
                    log_text.push('\n');
                }
                if !output.status.success() {
                    let combined = format!("{stdout}\n{stderr}");
                    if combined.contains("doesn't have a .glo file")
                        || combined.contains("Nothing to do")
                    {
                        log_text.push_str("(Nessun glossario attivo trovato — passo saltato.)\n");
                    } else {
                        diagnostics.push(CompileDiagnostic {
                            line: None,
                            title: "Errore makeglossaries".to_string(),
                            explanation:
                                "Controlla le voci \\newglossaryentry / \\makeglossaries nel documento."
                                    .to_string(),
                            raw_excerpt: combined.lines().take(6).collect::<Vec<_>>().join("\n"),
                        });
                        pipeline_ok = false;
                        break;
                    }
                }
            }
            RecipeStep::MakeIndex => {
                let idx_path = build_dir.join(format!("{stem}.idx"));
                if !idx_path.exists() {
                    log_text.push_str("(Nessun file .idx trovato — passo MakeIndex saltato.)\n");
                    continue;
                }
                let output = run_makeindex_step(&build_dir, &stem).await?;
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stdout.trim().is_empty() {
                    log_text.push_str(stdout.trim());
                    log_text.push('\n');
                }
                if !stderr.trim().is_empty() {
                    log_text.push_str(stderr.trim());
                    log_text.push('\n');
                }
                if !output.status.success() {
                    diagnostics.push(CompileDiagnostic {
                        line: None,
                        title: "Errore MakeIndex".to_string(),
                        explanation: "Controlla i comandi \\makeindex / \\index{} nel sorgente."
                            .to_string(),
                        raw_excerpt: format!("{stdout}\n{stderr}")
                            .lines()
                            .take(6)
                            .collect::<Vec<_>>()
                            .join("\n"),
                    });
                    pipeline_ok = false;
                    break;
                }
            }
            RecipeStep::LatexMkPdf | RecipeStep::LatexMkLua | RecipeStep::LatexMkXe => {
                last_used_body_path = None;
                let flag = match step {
                    RecipeStep::LatexMkLua => "-lualatex",
                    RecipeStep::LatexMkXe => "-xelatex",
                    _ => "-pdf",
                };
                let output =
                    run_latexmk_step(flag, parent_dir, file_name, shell_escape).await?;
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                if output.status.success() {
                    if !stdout.trim().is_empty() {
                        log_text.push_str(stdout.trim());
                        log_text.push('\n');
                    }
                } else {
                    let raw_log = read_log_lossy(&log_file).await;
                    append_tex_failure_logs(
                        &mut log_text,
                        "latexmk",
                        raw_log.as_deref(),
                        &stdout,
                        &stderr,
                        &mut diagnostics,
                    );
                    pipeline_ok = false;
                    break;
                }
            }
        }
    }

    let sync_tex_path: &Path = if compile_tex_path != tex_path {
        tex_path.as_path()
    } else if let Some(ref body) = last_used_body_path {
        body.as_path()
    } else {
        compile_tex_path.as_path()
    };

    let pdf_candidate = build_dir.join(format!("{stem}.pdf"));
    let pdf_path = if pipeline_ok && pdf_candidate.exists() {
        if let Some(pdf_fname) = pdf_candidate.file_name() {
            mirror_pdf_to_project_dir(&pdf_candidate, parent_dir, pdf_fname);
        }
        Some(pdf_candidate)
    } else {
        None
    };

    let mut sync_target = None;
    let (
        total_pages,
        page_aspect_ratio,
        page_width_pts,
        page_height_pts,
        initial_pages,
        preview_note,
    ) = match &pdf_path {
        Some(pdf) => match pdf_metadata(pdf).await {
            Ok((pages_count, aspect, width_pts, height_pts)) if pages_count > 0 => {
                sync_target =
                    synctex_forward_search(sync_tex_path, pdf, sync_line, height_pts, pages_count)
                        .await;
                let anchor_page = sync_target
                    .map(|t| t.page)
                    .unwrap_or(fallback_page)
                    .clamp(1, pages_count);
                let start = anchor_page.saturating_sub(1).max(1);
                let end = (anchor_page + 1).min(pages_count);
                let to_render: Vec<usize> = (start..=end).collect();
                let target_h = ((target_width_px as f32) * aspect).round().max(100.0) as u32;
                match render_pages_parallel(pdf.clone(), to_render, target_width_px, target_h).await
                {
                    Ok(rendered) => (pages_count, aspect, width_pts, height_pts, rendered, None),
                    Err(err) => (
                        pages_count,
                        aspect,
                        width_pts,
                        height_pts,
                        Vec::new(),
                        Some(err),
                    ),
                }
            }
            Ok(_) => (0, 1.4142, 595.28, 841.89, Vec::new(), None),
            Err(err) => (0, 1.4142, 595.28, 841.89, Vec::new(), Some(err)),
        },
        None => (0, 1.4142, 595.28, 841.89, Vec::new(), None),
    };

    Ok(CompileResult {
        success: pipeline_ok,
        pdf_path,
        total_pages,
        page_aspect_ratio,
        page_width_pts,
        page_height_pts,
        initial_pages,
        sync_target,
        diagnostics,
        preview_note,
        log: if log_text.trim().is_empty() {
            "Compilazione completata senza output.".to_string()
        } else {
            log_text
        },
        elapsed_secs: started.elapsed().as_secs_f32(),
    })
}

/// Forward SyncTeX search: maps a 1-based line in `tex_path` to `(page, v_ratio)` in `pdf_path`.
/// Automatically checks `.build/<stem>_body.tex` and `.build/live/<file_name>` when compiled
/// with precompiled `.fmt` or shadow live compilation.
pub async fn synctex_forward_search(
    tex_path: &Path,
    pdf_path: &Path,
    line_1based: usize,
    page_height_pts: f32,
    total_pages: usize,
) -> Option<PdfSyncTarget> {
    if let Some(hit) =
        synctex_forward_query(tex_path, pdf_path, line_1based, page_height_pts, total_pages).await
    {
        return Some(hit);
    }
    let parent = tex_path.parent()?;
    if let Some(stem) = tex_path.file_stem().and_then(|s| s.to_str()) {
        let body_candidate = parent.join(".build").join(format!("{stem}_body.tex"));
        if body_candidate != tex_path && body_candidate.exists() {
            if let Some(hit) = synctex_forward_query(
                &body_candidate,
                pdf_path,
                line_1based,
                page_height_pts,
                total_pages,
            )
            .await
            {
                return Some(hit);
            }
        }
    }
    let file_name = tex_path.file_name()?;
    let shm_live_candidate = live_dir_for(parent).join(file_name);
    if shm_live_candidate != tex_path && shm_live_candidate.exists() {
        if let Some(hit) = synctex_forward_query(
            &shm_live_candidate,
            pdf_path,
            line_1based,
            page_height_pts,
            total_pages,
        )
        .await
        {
            return Some(hit);
        }
    }
    let live_candidate = parent.join(".build").join("live").join(file_name);
    if live_candidate != tex_path && live_candidate != shm_live_candidate && live_candidate.exists() {
        return synctex_forward_query(
            &live_candidate,
            pdf_path,
            line_1based,
            page_height_pts,
            total_pages,
        )
        .await;
    }
    None
}

async fn synctex_forward_query(
    tex_path: &Path,
    pdf_path: &Path,
    line_1based: usize,
    page_height_pts: f32,
    total_pages: usize,
) -> Option<PdfSyncTarget> {
    let parent = pdf_path
        .parent()
        .and_then(|p| p.parent())
        .or_else(|| tex_path.parent())?;
    let input_spec = format!("{}:1:{}", line_1based.max(1), tex_path.display());

    let out = Command::new("synctex")
        .current_dir(parent)
        .args(["view", "-i", &input_spec, "-o"])
        .arg(pdf_path)
        .output()
        .await
        .ok()?;

    if !out.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut page: Option<usize> = None;
    let mut v_pts: Option<f32> = None;

    for line in stdout.lines() {
        if page.is_none() {
            if let Some(rest) = line.strip_prefix("Page:") {
                if let Ok(p) = rest.trim().parse::<usize>() {
                    if p >= 1 && p <= total_pages {
                        page = Some(p);
                    }
                }
            }
        } else if v_pts.is_none() {
            if let Some(rest) = line.strip_prefix("v:") {
                if let Ok(v) = rest.trim().parse::<f32>() {
                    v_pts = Some(v);
                    break;
                }
            }
        }
    }

    let page = page?;
    let v_ratio = v_pts
        .map(|v| (v / page_height_pts.max(100.0)).clamp(0.0, 1.0))
        .unwrap_or(0.0);

    Some(PdfSyncTarget { page, v_ratio })
}

/// Maps shadow `.build/live/<name>.tex` or `.build/<stem>_body.tex` paths back to the real
/// `<project_dir>/<stem>.tex` file so reverse SyncTeX always opens the user's source file.
fn remap_live_tex_path(path: PathBuf) -> PathBuf {
    let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
        return path;
    };
    let Some(parent) = path.parent() else {
        return path;
    };

    // Case 0: RAM-disk live directory `/dev/shm/feathertex_live_<hash>/<name>.tex` with `.origin`
    if let Ok(origin) = std::fs::read_to_string(parent.join(".origin")) {
        let trimmed = origin.trim();
        if !trimmed.is_empty() {
            let candidate = Path::new(trimmed).join(file_name);
            if candidate.exists() {
                return candidate;
            }
        }
    }

    let parent_name = parent.file_name().and_then(|n| n.to_str());

    // Case 1: `<project>/.build/live/<name>.tex`
    if parent_name == Some("live") {
        if let Some(build_dir) = parent.parent() {
            if build_dir.file_name().and_then(|n| n.to_str()) == Some(".build") {
                if let Some(project_dir) = build_dir.parent() {
                    let candidate = project_dir.join(file_name);
                    if candidate.exists() {
                        return candidate;
                    }
                }
            }
        }
    }

    // Case 2: `<project>/.build/<stem>_body.tex`
    if parent_name == Some(".build") {
        if let Some(stem) = file_name.strip_suffix("_body.tex") {
            if let Some(project_dir) = parent.parent() {
                let candidate = project_dir.join(format!("{stem}.tex"));
                if candidate.exists() {
                    return candidate;
                }
            }
        }
    }

    path
}

/// Reverse SyncTeX search: maps a clicked `(page, x_pts, y_pts)` in `pdf_path` back to
/// the originating `.tex` file and 1-based line number.
pub async fn synctex_reverse_search(
    pdf_path: &Path,
    page: usize,
    x_pts: f32,
    y_pts: f32,
) -> Option<ReverseSyncResult> {
    let spec = format!(
        "{}:{:.2}:{:.2}:{}",
        page.max(1),
        x_pts.max(0.0),
        y_pts.max(0.0),
        pdf_path.display()
    );

    let mut cmd = Command::new("synctex");
    if let Some(work_dir) = pdf_path.parent().and_then(|p| p.parent()) {
        cmd.current_dir(work_dir);
    }

    let out = cmd.args(["edit", "-o", &spec]).output().await.ok()?;
    if !out.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut res = parse_synctex_edit_output(&stdout)?;
    if let Ok(canon) = std::fs::canonicalize(&res.file_path) {
        res.file_path = canon;
    }
    res.file_path = remap_live_tex_path(res.file_path);
    Some(res)
}

fn parse_synctex_edit_output(stdout: &str) -> Option<ReverseSyncResult> {
    let mut input: Option<PathBuf> = None;
    let mut line: Option<usize> = None;

    for raw in stdout.lines() {
        if input.is_none() {
            if let Some(rest) = raw.strip_prefix("Input:") {
                let p = rest.trim();
                if !p.is_empty() {
                    input = Some(PathBuf::from(p));
                }
            }
        } else if line.is_none() {
            if let Some(rest) = raw.strip_prefix("Line:") {
                if let Ok(l) = rest.trim().parse::<usize>() {
                    line = Some(l.max(1));
                    break;
                }
            }
        }
    }

    Some(ReverseSyncResult {
        file_path: input?,
        line: line?,
    })
}

/// Parses pdflatex log output into clear, human-readable Italian diagnostics with line numbers.
pub fn parse_diagnostics(raw_log: &str) -> Vec<CompileDiagnostic> {
    let lines: Vec<&str> = raw_log.lines().collect();
    let mut out: Vec<CompileDiagnostic> = Vec::new();
    let mut seen: HashSet<(Option<usize>, String)> = HashSet::new();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim_end();

        // Format A: `-file-line-error` in user's .tex -> `./file.tex:42: Undefined control sequence.`
        // Format B: `-file-line-error` in a system .sty/.cls -> `/usr/.../babel.sty:4330: Package babel Error: ...`
        //           (do NOT treat the .sty line number as a line in the user's .tex file!)
        // Format C: classic TeX `! Error message`
        let parsed_entry: Option<(Option<usize>, String, usize)> =
            if let Some((line_no, msg)) = parse_tex_file_line_error(line) {
                Some((Some(line_no), msg.to_string(), i))
            } else if let Some(msg) = parse_package_file_line_error(line) {
                Some((None, msg.to_string(), i))
            } else if let Some(msg) = line.strip_prefix("! ") {
                Some((None, msg.trim().to_string(), i))
            } else {
                None
            };

        if let Some((mut line_no, mut msg, start_idx)) = parsed_entry {
            // Skip generic trailing stop messages if we already captured the actual error.
            if (msg.contains("Emergency stop") || msg.contains("Fatal error occurred"))
                && !out.is_empty()
            {
                i += 1;
                continue;
            }

            // Rejoin wrapped pdflatex error lines (pdflatex wraps log lines at 79 columns, e.g.
            // `Package babel Error: Un\nknown option 'italian'.`).
            let mut look_start = start_idx + 1;
            if start_idx + 1 < lines.len() {
                let next_raw = lines[start_idx + 1].trim_end();
                if line.len() >= 78
                    && !next_raw.is_empty()
                    && !next_raw.starts_with('!')
                    && !next_raw.starts_with("l.")
                    && !next_raw.starts_with('(')
                    && parse_tex_file_line_error(next_raw).is_none()
                    && parse_package_file_line_error(next_raw).is_none()
                {
                    msg.push_str(next_raw);
                    look_start = start_idx + 2;
                }
            }

            // Look ahead for `l.<N> <context>` and offending command.
            // For missing-file prompts (`! LaTeX Error: File ...`), `l.<N>` can appear ~10 lines
            // later after interactive prompt text (`Type X to quit...`, `Enter file name:`).
            let is_from_package = parse_package_file_line_error(line).is_some();
            let look_limit = if msg.contains("not found") { 14 } else { 7 };
            let look_end = (look_start + look_limit).min(lines.len());
            let mut context_lines = Vec::new();
            let mut offending_cmd: Option<String> = None;

            for j in look_start..look_end {
                let next = lines[j].trim();
                if next.starts_with('!') || parse_package_file_line_error(next).is_some() {
                    break;
                }
                if let Some((lno, sub_msg)) = parse_tex_file_line_error(next) {
                    if (sub_msg.contains("Emergency stop") || sub_msg.contains("Fatal error"))
                        && line_no.is_none()
                        && !is_from_package
                    {
                        line_no = Some(lno);
                        continue;
                    }
                    break;
                }
                if next.is_empty() {
                    if msg.contains("not found") && line_no.is_none() {
                        continue;
                    }
                    break;
                }
                if let Some((lno, snippet)) = parse_l_dot_line(next) {
                    if line_no.is_none() && !is_from_package {
                        line_no = Some(lno);
                    }
                    if offending_cmd.is_none() {
                        offending_cmd = extract_last_latex_cmd(snippet);
                    }
                    context_lines.push(next.to_string());
                } else if !next.starts_with("Type ")
                    && !next.starts_with("See the ")
                    && !next.starts_with("For immediate")
                    && !next.starts_with("or enter new name")
                    && !next.starts_with("Enter file name")
                    && !next.starts_with("<read")
                    && context_lines.len() < 3
                {
                    context_lines.push(next.to_string());
                }
            }

            let (title, explanation) = humanize_error(&msg, offending_cmd.as_deref());
            let key = (line_no, title.clone());
            if seen.insert(key) {
                let raw_excerpt = if context_lines.is_empty() {
                    msg
                } else {
                    format!("{msg}\n{}", context_lines.join("\n"))
                };
                out.push(CompileDiagnostic {
                    line: line_no,
                    title,
                    explanation,
                    raw_excerpt,
                });
            }
            i += 1;
        } else {
            i += 1;
        }
    }

    out
}

fn parse_tex_file_line_error(line: &str) -> Option<(usize, &str)> {
    let tex_idx = line.find(".tex:")?;
    let after_tex = &line[tex_idx + ".tex:".len()..];
    let colon = after_tex.find(':')?;
    let line_no: usize = after_tex[..colon].trim().parse().ok()?;
    let msg = after_tex[colon + 1..].trim();
    if msg.is_empty() {
        return None;
    }
    Some((line_no, msg))
}

fn parse_package_file_line_error(line: &str) -> Option<&str> {
    let ext_pos = line
        .find(".sty:")
        .map(|i| i + ".sty:".len())
        .or_else(|| line.find(".cls:").map(|i| i + ".cls:".len()))?;
    let after_ext = &line[ext_pos..];
    let colon = after_ext.find(':')?;
    let _line_no: usize = after_ext[..colon].trim().parse().ok()?;
    let msg = after_ext[colon + 1..].trim();
    if msg.is_empty() {
        return None;
    }
    Some(msg)
}

fn parse_l_dot_line(line: &str) -> Option<(usize, &str)> {
    let rest = line.strip_prefix("l.")?;
    let digits_end = rest
        .bytes()
        .take_while(|b| b.is_ascii_digit())
        .count();
    if digits_end == 0 {
        return None;
    }
    let line_no: usize = rest[..digits_end].parse().ok()?;
    let snippet = rest[digits_end..].trim();
    Some((line_no, snippet))
}

fn extract_last_latex_cmd(snippet: &str) -> Option<String> {
    let pos = snippet.rfind('\\')?;
    let rest = &snippet[pos + 1..];
    let cmd_len = rest
        .bytes()
        .take_while(|b| b.is_ascii_alphabetic())
        .count();
    if cmd_len == 0 {
        return None;
    }
    Some(format!("\\{}", &rest[..cmd_len]))
}

fn humanize_error(msg: &str, offending_cmd: Option<&str>) -> (String, String) {
    if msg.contains("Undefined control sequence") {
        let title = match offending_cmd {
            Some(cmd) => format!("Comando sconosciuto: {cmd}"),
            None => "Comando LaTeX sconosciuto".to_string(),
        };
        return (
            title,
            "Verifica l'ortografia del comando oppure aggiungi la libreria \\usepackage{...} che lo definisce."
                .to_string(),
        );
    }

    if let Some(start) = msg.find("File `") {
        let rest = &msg[start + "File `".len()..];
        if let Some(end) = rest.find('\'') {
            let file = &rest[..end];
            if let Some(pkg) = file.strip_suffix(".sty") {
                let group = suggest_texlive_group(pkg);
                return (
                    format!("Libreria mancante: {file}"),
                    format!(
                        "Il pacchetto '{pkg}' non è installato in TeX Live. Su Arch Linux installalo con: sudo pacman -S {group}"
                    ),
                );
            }
            return (
                format!("File non trovato: {file}"),
                "Verifica che il file esista nel percorso indicato e che il nome sia corretto."
                    .to_string(),
            );
        }
    }

    if msg.contains("Package babel Error: Unknown option") {
        if let Some(start) = msg.find("Unknown option '") {
            let rest = &msg[start + "Unknown option '".len()..];
            if let Some(end) = rest.find('\'') {
                let lang = &rest[..end];
                return (
                    format!("Lingua babel '{lang}' non installata"),
                    format!(
                        "Installa il supporto lingua con: sudo pacman -S texlive-lang{lang} (oppure commenta \\usepackage[{lang}]{{babel}})."
                    ),
                );
            }
        }
    }

    if msg.contains("Missing $ inserted") {
        return (
            "Manca il delimitatore matematico $".to_string(),
            "Hai usato un comando o simbolo matematico (es. _, ^, \\alpha, \\frac) fuori da $ ... $."
                .to_string(),
        );
    }

    if msg.contains("Extra }, or forgotten $")
        || msg.contains("Missing } inserted")
        || msg.contains("Too many }'s")
    {
        return (
            "Parentesi graffe { } o $ non bilanciati".to_string(),
            "Controlla che ogni parentesi { abbia la sua } corrispondente e che la formula matematica sia chiusa."
                .to_string(),
        );
    }

    if msg.contains("ended by \\end{") {
        return (
            format!("Ambiente chiuso in modo errato ({msg})"),
            "Il nome dentro \\end{...} non coincide con l'ultimo \\begin{...} aperto. Controlla gli ambienti annidati."
                .to_string(),
        );
    }

    if msg.contains("Environment ") && msg.contains(" undefined") {
        return (
            format!("Ambiente non definito: {msg}"),
            "Controlla il nome dell'ambiente oppure aggiungi il pacchetto richiesto nel preambolo (es. amsmath, amsthm, tikz)."
                .to_string(),
        );
    }

    if msg.contains("Extra alignment tab") {
        return (
            "Troppi simboli '&' nella riga".to_string(),
            "Controlla il numero di colonne nella tabella/equazione e verifica di usare \\\\ (doppio backslash) a fine riga."
                .to_string(),
        );
    }

    if msg.contains("Misplaced alignment tab character &") {
        return (
            "Carattere '&' fuori da una tabella o align".to_string(),
            "Nel testo normale il simbolo '&' va scritto come \\&.".to_string(),
        );
    }

    if msg.contains("Double subscript") || msg.contains("Double superscript") {
        return (
            "Doppio pedice (_) o apice (^) consecutivo".to_string(),
            "Raggruppa gli indici con le graffe, ad esempio x_{a_b} oppure x^{2^n}.".to_string(),
        );
    }

    if msg.contains("Runaway argument") {
        return (
            "Argomento non chiuso (probabile '}' mancante)".to_string(),
            "Una parentesi graffa '{' aperta in precedenza non è stata chiusa prima della fine del paragrafo."
                .to_string(),
        );
    }

    (
        msg.to_string(),
        "Controlla la riga evidenziata nell'editor o apri il log tecnico per i dettagli completi."
            .to_string(),
    )
}

/// Renders a requested batch of 1-based PDF pages in parallel at exact physical display pixels
/// `(target_width_px, target_height_px)`.
/// Prefers `pdftocairo` (Cairo + FreeType glyph rasterizer, same stack as Linux browsers/Evince,
/// default antialiasing without `-antialias best` so Type 1 `cm-super` fonts stay intact) streaming
/// PNG directly to stdout, with automatic fallback to `pdftoppm`.
pub async fn render_pdf_page_batch(
    pdf_path: PathBuf,
    pages: Vec<usize>,
    generation: u64,
    target_width_px: u32,
    target_height_px: u32,
) -> Result<(u64, Vec<RenderedPage>), String> {
    let rendered =
        render_pages_parallel(pdf_path, pages, target_width_px, target_height_px).await?;
    Ok((generation, rendered))
}

async fn render_pages_parallel(
    pdf_path: PathBuf,
    pages: Vec<usize>,
    target_width_px: u32,
    target_height_px: u32,
) -> Result<Vec<RenderedPage>, String> {
    #[cfg(target_os = "linux")]
    {
        let p = pdf_path.clone();
        let req_pages = pages.clone();
        if let Ok(Ok(rendered)) = tokio::task::spawn_blocking(move || {
            crate::poppler_ffi::render_pages_sync(&p, &req_pages, target_width_px, target_height_px)
        })
        .await
        {
            return Ok(rendered);
        }
    }

    let mut set = JoinSet::new();

    for page_num in pages {
        let pdf_owned = pdf_path.clone();
        set.spawn(async move {
            render_single_page(pdf_owned, page_num, target_width_px, target_height_px).await
        });
    }

    let mut out = Vec::new();
    while let Some(res) = set.join_next().await {
        match res {
            Ok(Ok(page)) => out.push(page),
            Ok(Err(e)) => return Err(e),
            Err(e) => return Err(format!("Errore worker rendering PDF: {e}")),
        }
    }

    out.sort_by_key(|p| p.page_number);
    Ok(out)
}

async fn render_single_page(
    pdf_path: PathBuf,
    page_number: usize,
    target_width_px: u32,
    target_height_px: u32,
) -> Result<RenderedPage, String> {
    let target_w = target_width_px.clamp(240, 2800);
    let target_h = target_height_px.clamp(240, 4000);
    let w_str = target_w.to_string();
    let h_str = target_h.to_string();
    let p_str = page_number.to_string();

    // Primary: `pdftocairo -png -singlefile` (Cairo + FreeType, exact 1:1 physical pixels,
    // zero temp files). Note: intentionally uses default Cairo antialiasing (never `-antialias best`,
    // which triggers a Poppler/Cairo bug on Type 1 `cm-super` fonts).
    if let Ok(output) = Command::new("pdftocairo")
        .args([
            "-png",
            "-singlefile",
            "-scale-to-x",
            &w_str,
            "-scale-to-y",
            &h_str,
            "-f",
            &p_str,
            "-l",
            &p_str,
        ])
        .arg(&pdf_path)
        .arg("-")
        .output()
        .await
    {
        if output.status.success() && output.stdout.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Ok(RenderedPage {
                page_number,
                data: RenderedPageData::Png(output.stdout),
            });
        }
    }

    // Fallback: `pdftoppm` raw PPM P6 via stdout.
    let output = Command::new("pdftoppm")
        .args([
            "-scale-to-x",
            &w_str,
            "-scale-to-y",
            &h_str,
            "-aa",
            "yes",
            "-aaVector",
            "yes",
            "-f",
            &p_str,
            "-l",
            &p_str,
        ])
        .arg(&pdf_path)
        .output()
        .await
        .map_err(|_| {
            "Anteprima interna non disponibile: installa 'poppler' (pdftocairo/pdftoppm) oppure usa 'Esterno'."
                .to_string()
        })?;

    if !output.status.success() {
        return Err(format!(
            "Errore rendering pagina {page_number}: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let (width, height, bytes) = parse_ppm_p6_to_rgba(&output.stdout, false)?;
    Ok(RenderedPage {
        page_number,
        data: RenderedPageData::Rgba {
            width,
            height,
            bytes,
        },
    })
}

fn parse_ppm_p6_to_rgba(
    data: &[u8],
    downsample_2x: bool,
) -> Result<(u32, u32, Vec<u8>), String> {
    let mut idx = 0usize;
    let len = data.len();

    let next_token = |idx: &mut usize| -> Option<&[u8]> {
        while *idx < len {
            let b = data[*idx];
            if b.is_ascii_whitespace() {
                *idx += 1;
            } else if b == b'#' {
                while *idx < len && data[*idx] != b'\n' {
                    *idx += 1;
                }
            } else {
                break;
            }
        }
        let start = *idx;
        while *idx < len && !data[*idx].is_ascii_whitespace() && data[*idx] != b'#' {
            *idx += 1;
        }
        if *idx > start {
            Some(&data[start..*idx])
        } else {
            None
        }
    };

    let magic = next_token(&mut idx).ok_or_else(|| "Header PPM mancante".to_string())?;
    if magic != b"P6" {
        return Err("Formato PPM non supportato (atteso P6)".to_string());
    }

    let w_tok = next_token(&mut idx).ok_or_else(|| "Larghezza PPM mancante".to_string())?;
    let h_tok = next_token(&mut idx).ok_or_else(|| "Altezza PPM mancante".to_string())?;
    let max_tok = next_token(&mut idx).ok_or_else(|| "Maxval PPM mancante".to_string())?;

    let width: u32 = std::str::from_utf8(w_tok)
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| "Larghezza PPM non valida".to_string())?;
    let height: u32 = std::str::from_utf8(h_tok)
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| "Altezza PPM non valida".to_string())?;
    let _maxval: u32 = std::str::from_utf8(max_tok)
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| "Maxval PPM non valido".to_string())?;

    if idx < len && data[idx].is_ascii_whitespace() {
        idx += 1;
    }

    let pixel_count = (width as usize).saturating_mul(height as usize);
    let expected_rgb = pixel_count.saturating_mul(3);
    if len.saturating_sub(idx) < expected_rgb {
        return Err("Dati PPM troncati".to_string());
    }

    let rgb = &data[idx..idx + expected_rgb];

    if downsample_2x && width >= 2 && height >= 2 {
        let out_w = (width / 2) as usize;
        let out_h = (height / 2) as usize;
        let row_stride = (width as usize) * 3;
        let mut rgba = vec![255u8; out_w * out_h * 4];

        for oy in 0..out_h {
            let r0 = (oy * 2) * row_stride;
            let r1 = r0 + row_stride;
            let dst_row = oy * out_w * 4;
            for ox in 0..out_w {
                let c0 = ox * 6;
                let c1 = c0 + 3;
                let d = dst_row + ox * 4;
                for ch in 0..3 {
                    let sum = rgb[r0 + c0 + ch] as u16
                        + rgb[r0 + c1 + ch] as u16
                        + rgb[r1 + c0 + ch] as u16
                        + rgb[r1 + c1 + ch] as u16;
                    rgba[d + ch] = ((sum + 2) >> 2) as u8;
                }
            }
        }
        return Ok((out_w as u32, out_h as u32, rgba));
    }

    let mut rgba = vec![255u8; pixel_count * 4];
    for (src, dst) in rgb.chunks_exact(3).zip(rgba.chunks_exact_mut(4)) {
        dst[0] = src[0];
        dst[1] = src[1];
        dst[2] = src[2];
    }

    Ok((width, height, rgba))
}

async fn pdf_metadata(pdf_path: &Path) -> Result<(usize, f32, f32, f32), String> {
    #[cfg(target_os = "linux")]
    {
        let p = pdf_path.to_path_buf();
        if let Ok(Ok(meta)) =
            tokio::task::spawn_blocking(move || crate::poppler_ffi::pdf_metadata_sync(&p)).await
        {
            return Ok(meta);
        }
    }

    let out = Command::new("pdfinfo")
        .arg(pdf_path)
        .output()
        .await
        .map_err(|_| {
            "Anteprima interna non disponibile: installa 'poppler' (pdfinfo/pdftoppm) oppure usa 'Esterno'."
                .to_string()
        })?;

    if !out.status.success() {
        return Err(format!(
            "Errore pdfinfo: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let mut pages = 0usize;
    let mut aspect = 1.4142_f32;
    let mut width_pts = 595.28_f32;
    let mut height_pts = 841.89_f32;

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Pages:") {
            if let Ok(n) = rest.trim().parse::<usize>() {
                pages = n;
            }
        } else if let Some(rest) = line.strip_prefix("Page size:") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 3 && parts[1] == "x" {
                if let (Ok(w), Ok(h)) = (parts[0].parse::<f32>(), parts[2].parse::<f32>()) {
                    if w > 1.0 && h > 1.0 {
                        aspect = h / w;
                        width_pts = w;
                        height_pts = h;
                    }
                }
            }
        }
    }

    Ok((pages, aspect, width_pts, height_pts))
}

fn suggest_texlive_group(sty: &str) -> &'static str {
    if let Some(lib) = MATH_LIBRARIES.iter().find(|l| l.package == sty) {
        return lib.texlive_group;
    }
    match sty {
        "pgf" | "tikz" | "tikz-cd" | "pgfplots" => "texlive-pictures",
        "stmaryrd" | "dsfont" | "mathrsfs" | "bbm" => "texlive-fontsextra",
        "physics" | "siunitx" | "ebproof" | "bussproofs" | "algorithm2e" | "algpseudocode"
        | "esint" | "braket" | "tensor" | "derivative" | "diffcoeff" | "nicematrix" => {
            "texlive-mathscience"
        }
        _ => "texlive-latexextra",
    }
}

fn diagnose_missing_packages(log: &str) -> Option<String> {
    let mut missing_items: Vec<String> = Vec::new();
    let mut arch_groups: Vec<String> = Vec::new();

    for line in log.lines() {
        if let Some(start) = line.find("File `") {
            let rest = &line[start + "File `".len()..];
            if let Some(end) = rest.find('\'') {
                let filename = &rest[..end];
                if let Some(pkg) = filename.strip_suffix(".sty") {
                    let group = suggest_texlive_group(pkg);
                    let entry = format!("• Pacchetto mancante '{pkg}.sty' (gruppo: {group})");
                    if !missing_items.contains(&entry) {
                        missing_items.push(entry);
                    }
                    if !arch_groups.iter().any(|g| g == group) {
                        arch_groups.push(group.to_string());
                    }
                }
            }
        }

        if line.contains("Package babel Error: Unknown option") {
            if let Some(start) = line.find("Unknown option '") {
                let rest = &line[start + "Unknown option '".len()..];
                if let Some(end) = rest.find('\'') {
                    let lang = &rest[..end];
                    let group = format!("texlive-lang{lang}");
                    let entry = format!("• Lingua babel '{lang}' mancante (gruppo: {group})");
                    if !missing_items.contains(&entry) {
                        missing_items.push(entry);
                    }
                    if !arch_groups.iter().any(|g| g == &group) {
                        arch_groups.push(group);
                    }
                }
            }
        }
    }

    if missing_items.is_empty() {
        return None;
    }

    let mut out = String::from("=== Suggerimento pacchetti TeX Live mancanti ===\n");
    for item in &missing_items {
        out.push_str(item);
        out.push('\n');
    }
    if !arch_groups.is_empty() {
        out.push_str(&format!(
            "\nSu Arch Linux puoi installarli con:\n  sudo pacman -S {}",
            arch_groups.join(" ")
        ));
    }
    Some(out)
}

pub fn open_pdf_external(pdf_path: &Path) {
    let preferred = user_facing_pdf_path(pdf_path);
    let target = if preferred.exists() {
        preferred.as_path()
    } else {
        pdf_path
    };

    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(target).spawn();

    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(target).spawn();

    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("rundll32.exe")
        .arg("url.dll,FileProtocolHandler")
        .arg(target)
        .spawn();
}

pub fn open_url_external(url: &str) {
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();

    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();

    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("rundll32.exe")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .spawn();
}

/// Returns `<project_dir>/<name>.pdf` when given `.build/<name>.pdf`.
pub fn user_facing_pdf_path(pdf_path: &Path) -> PathBuf {
    if let (Some(fname), Some(build_dir)) = (pdf_path.file_name(), pdf_path.parent()) {
        if build_dir.file_name().and_then(|n| n.to_str()) == Some(".build") {
            if let Some(proj_dir) = build_dir.parent() {
                return proj_dir.join(fname);
            }
        }
    }
    pdf_path.to_path_buf()
}

pub async fn export_pdf_as(
    source_pdf: PathBuf,
    default_name: String,
    default_dir: Option<PathBuf>,
) -> Result<PathBuf, String> {
    let mut dialog = rfd::AsyncFileDialog::new()
        .set_title("Salva PDF")
        .add_filter("Documento PDF", &["pdf"])
        .set_file_name(&default_name);
    if let Some(dir) = default_dir {
        if dir.is_dir() {
            dialog = dialog.set_directory(dir);
        }
    }
    let handle = dialog
        .save_file()
        .await
        .ok_or_else(|| "Esportazione PDF annullata".to_string())?;
    let dest = handle.path().to_path_buf();

    let preferred = user_facing_pdf_path(&source_pdf);
    let src = if preferred.exists() { preferred } else { source_pdf };

    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let bytes =
            std::fs::read(&src).map_err(|e| format!("Impossibile leggere il PDF: {e}"))?;
        crate::document::write_file_atomic_sync(&dest, &bytes)
            .map_err(|e| format!("Errore salvataggio PDF: {e}"))?;
        Ok(())
    })
    .await
    .map_err(|e| format!("Errore worker salvataggio PDF: {e}"))??;

    Ok(handle.path().to_path_buf())
}

pub async fn clean_build_dir(tex_path: PathBuf) -> Result<String, String> {
    let parent_dir = tex_path
        .parent()
        .ok_or_else(|| "Cartella del file non trovata".to_string())?;
    let build_dir = parent_dir.join(".build");

    if build_dir.is_symlink() {
        return Err(
            "La cartella .build è un collegamento simbolico: pulizia annullata per sicurezza."
                .to_string(),
        );
    }

    if build_dir.exists() {
        tokio::fs::remove_dir_all(&build_dir)
            .await
            .map_err(|e| format!("Errore pulizia .build: {e}"))?;
    }
    let shm_live = live_dir_for(parent_dir);
    if shm_live != build_dir.join("live") && shm_live.exists() {
        let _ = tokio::fs::remove_dir_all(&shm_live).await;
    }
    Ok("Cartella .build pulita".to_string())
}

fn extract_errors(raw_log: &str) -> String {
    let lines: Vec<&str> = raw_log.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        if line.starts_with('!') || line.contains(".tex:") {
            out.push(line);
            if i + 1 < lines.len() {
                out.push(lines[i + 1]);
            }
            if i + 2 < lines.len() && lines[i + 2].starts_with("l.") {
                out.push(lines[i + 2]);
            }
            out.push("");
            i += 2;
        } else {
            i += 1;
        }
    }

    out.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ppm_p6_into_rgba() {
        let mut ppm = b"P6\n# comment\n2 1\n255\n".to_vec();
        ppm.extend_from_slice(&[10, 20, 30, 40, 50, 60]);
        let (w, h, rgba) = parse_ppm_p6_to_rgba(&ppm, false).expect("valid ppm");
        assert_eq!((w, h), (2, 1));
        assert_eq!(rgba, vec![10, 20, 30, 255, 40, 50, 60, 255]);

        // 2x2 supersampled downsample check
        let mut ppm2 = b"P6\n2 2\n255\n".to_vec();
        ppm2.extend_from_slice(&[
            10, 20, 30, 30, 40, 50, // row 0
            50, 60, 70, 70, 80, 90, // row 1
        ]);
        let (w2, h2, rgba2) = parse_ppm_p6_to_rgba(&ppm2, true).expect("downsampled ppm");
        assert_eq!((w2, h2), (1, 1));
        assert_eq!(rgba2, vec![40, 50, 60, 255]);
    }

    #[test]
    fn parses_human_readable_diagnostics_with_line_numbers() {
        let sample_log = r#"
/usr/share/texmf-dist/tex/generic/babel/babel.sty:4330: Package babel Error: Un
known option 'italian'.

./doc.tex:42: Undefined control sequence.
l.42 $x + \nonexistent
                       + y$
! Missing $ inserted.
<inserted text>
                $
l.58 a_1 + b_2
! LaTeX Error: File `stmaryrd.sty' not found.

Type X to quit or <RETURN> to proceed,
or enter new name. (Default extension: sty)

Enter file name:
./MFI.tex:15: Emergency stop.
<read *>

l.15 \usepackage
"#;
        let diags = parse_diagnostics(sample_log);
        assert_eq!(diags.len(), 4);
        assert_eq!(diags[0].line, None);
        assert!(diags[0].title.contains("italian"));
        assert_eq!(diags[1].line, Some(42));
        assert!(diags[1].title.contains("\\nonexistent"));
        assert_eq!(diags[2].line, Some(58));
        assert!(diags[2].title.contains("$"));
        assert_eq!(diags[3].line, Some(15));
        assert!(diags[3].title.contains("stmaryrd.sty"));
    }

    #[test]
    fn parses_synctex_reverse_search_output() {
        let sample = "\
This is SyncTeX command line utility, version 1.5
SyncTeX result begin
Output:.build/MFI.pdf
Input:/home/user/./MFI.tex
Line:342
Column:-1
Offset:0
Context:
SyncTeX result end
";
        let res = parse_synctex_edit_output(sample).expect("parsed reverse sync");
        assert_eq!(res.file_path, PathBuf::from("/home/user/./MFI.tex"));
        assert_eq!(res.line, 342);
    }

    #[test]
    fn remaps_shadow_live_tex_path_back_to_source() {
        let dir = std::env::temp_dir().join("feathertex_live_remap_test");
        let live_dir = dir.join(".build").join("live");
        let _ = std::fs::create_dir_all(&live_dir);
        let real = dir.join("notes.tex");
        let shadow = live_dir.join("notes.tex");
        std::fs::write(&real, "").unwrap();
        std::fs::write(&shadow, "").unwrap();

        assert_eq!(remap_live_tex_path(shadow), real);
        let body = dir.join(".build").join("notes_body.tex");
        std::fs::write(&body, "").unwrap();
        assert_eq!(remap_live_tex_path(body), real);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn splits_preamble_preserving_exact_body_line_numbers() {
        let src = "\\documentclass{article}\n\\usepackage{amsmath}\n\\begin{document}\nHello\n\\end{document}\n";
        let (pre, body) = split_preamble_and_body(src).expect("valid split");
        assert!(pre.contains("\\usepackage{amsmath}"));
        let body_lines: Vec<&str> = body.lines().collect();
        assert_eq!(body_lines[0], "%");
        assert_eq!(body_lines[1], "%");
        assert_eq!(body_lines[2], "\\begin{document}");
        assert_eq!(body_lines[3], "Hello");
    }

    #[test]
    fn promotes_staged_live_pdf_atomically_into_build_dir() {
        let dir = std::env::temp_dir().join("feathertex_promote_test");
        let build = dir.join(".build");
        let live = build.join("live");
        let _ = std::fs::create_dir_all(&live);
        let live_pdf = live.join("doc.pdf");
        let live_sync = live.join("doc.synctex");
        std::fs::write(&live_pdf, b"%PDF-new").unwrap();
        std::fs::write(&live_sync, b"SyncTeX").unwrap();

        let staged = CompileResult {
            success: true,
            pdf_path: Some(live_pdf.clone()),
            total_pages: 1,
            page_aspect_ratio: 1.4142,
            page_width_pts: 595.28,
            page_height_pts: 841.89,
            initial_pages: Vec::new(),
            sync_target: None,
            diagnostics: Vec::new(),
            preview_note: None,
            log: String::new(),
            elapsed_secs: 0.4,
        };

        let promoted = promote_staged_live_result(staged).expect("promoted");
        let expected_pdf = build.join("doc.pdf");
        assert_eq!(promoted.pdf_path, Some(expected_pdf.clone()));
        assert_eq!(std::fs::read(&expected_pdf).unwrap(), b"%PDF-new");
        assert!(build.join("doc.synctex").exists());
        assert!(!live_pdf.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remaps_origin_marker_and_promotes_aux_across_directories() {
        let base = std::env::temp_dir().join("feathertex_origin_promote_test");
        let proj = base.join("project");
        let shm_like = base.join("ram_live");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::create_dir_all(&shm_like).unwrap();

        let real_tex = proj.join("thesis.tex");
        std::fs::write(&real_tex, "\\documentclass{article}").unwrap();
        std::fs::write(shm_like.join(".origin"), proj.to_string_lossy().as_bytes()).unwrap();
        let live_tex = shm_like.join("thesis.tex");
        std::fs::write(&live_tex, "%").unwrap();
        assert_eq!(remap_live_tex_path(live_tex), real_tex);

        let live_pdf = shm_like.join("thesis.pdf");
        let live_sync = shm_like.join("thesis.synctex");
        let live_aux = shm_like.join("thesis.aux");
        std::fs::write(&live_pdf, b"%PDF-ram").unwrap();
        std::fs::write(&live_sync, b"SyncTeX-ram").unwrap();
        std::fs::write(&live_aux, b"\\relax").unwrap();

        let staged = CompileResult {
            success: true,
            pdf_path: Some(live_pdf),
            total_pages: 0,
            page_aspect_ratio: 1.4142,
            page_width_pts: 595.28,
            page_height_pts: 841.89,
            initial_pages: Vec::new(),
            sync_target: None,
            diagnostics: Vec::new(),
            preview_note: None,
            log: String::new(),
            elapsed_secs: 0.2,
        };

        let promoted = promote_staged_live_result(staged).expect("promoted from origin");
        let build = proj.join(".build");
        assert_eq!(promoted.pdf_path, Some(build.join("thesis.pdf")));
        assert_eq!(std::fs::read(build.join("thesis.pdf")).unwrap(), b"%PDF-ram");
        assert!(build.join("thesis.synctex").exists());
        assert!(build.join("thesis.aux").exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn poppler_ffi_reads_metadata_and_renders_rgba_page() {
        if !crate::poppler_ffi::is_available() {
            return;
        }
        let dir = std::env::temp_dir().join("feathertex_poppler_ffi_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let tex = dir.join("mini.tex");
        std::fs::write(
            &tex,
            "\\documentclass{article}\n\\begin{document}\nHello $x^2$.\n\\end{document}\n",
        )
        .unwrap();

        let status = std::process::Command::new("pdflatex")
            .current_dir(&dir)
            .args(["-interaction=batchmode", "mini.tex"])
            .status();
        if status.is_ok_and(|s| s.success()) {
            let pdf = dir.join("mini.pdf");
            let (pages, aspect, w_pts, h_pts) =
                crate::poppler_ffi::pdf_metadata_sync(&pdf).expect("poppler metadata");
            assert_eq!(pages, 1);
            assert!(w_pts > 400.0 && h_pts > 600.0 && aspect > 1.0);

            let rendered =
                crate::poppler_ffi::render_pages_sync(&pdf, &[1], 320, 452).expect("poppler render");
            assert_eq!(rendered.len(), 1);
            match &rendered[0].data {
                RenderedPageData::Rgba {
                    width,
                    height,
                    bytes,
                } => {
                    assert_eq!((*width, *height), (320, 452));
                    assert_eq!(bytes.len(), 320 * 452 * 4);
                    // Background pixel at top-left should be opaque white.
                    assert_eq!(&bytes[0..4], &[255, 255, 255, 255]);
                }
                _ => panic!("expected direct RGBA buffer from poppler_ffi"),
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_non_utf8_log_lossily_and_extracts_sigma_missing_dollar_line() {
        let dir = std::env::temp_dir().join("feathertex_non_utf8_log_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log_path = dir.join("doc.log");

        // Simulate pdfTeX splitting a 2-byte UTF-8 accented char ('è' = 0xC3 0xA8) across a 79-byte wrap
        let mut raw = b"Overfull \\hbox: propriet\xc3\n\xa8 del sistema\n".to_vec();
        raw.extend_from_slice(
            b"./MFI.tex:42: Missing $ inserted.\n<inserted text>\n                $\nl.42 Testo con \\sigma qui.\n",
        );
        std::fs::write(&log_path, raw).unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let log_str = rt
            .block_on(read_log_lossy(&log_path))
            .expect("lossy log read must succeed even with split UTF-8 bytes");
        let diags = parse_diagnostics(&log_str);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].line, Some(42));
        assert!(diags[0].title.contains('$'));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compiles_bibliography_recipe_with_bibtex_and_tex_root() {
        if std::process::Command::new("pdflatex").arg("--version").output().is_err()
            || std::process::Command::new("bibtex").arg("--version").output().is_err()
        {
            return;
        }

        let dir = std::env::temp_dir().join("feathertex_bib_recipe_test");
        let _ = std::fs::remove_dir_all(&dir);
        let chap_dir = dir.join("chapters");
        std::fs::create_dir_all(&chap_dir).unwrap();

        let bib_path = dir.join("refs.bib");
        std::fs::write(
            &bib_path,
            "@article{knuth1984,\n  author={Donald E. Knuth},\n  title={Literate Programming},\n  journal={The Computer Journal},\n  year={1984}\n}\n",
        )
        .unwrap();

        let main_path = dir.join("main.tex");
        std::fs::write(
            &main_path,
            "\\documentclass{article}\n\\begin{document}\n\\input{chapters/cap1}\n\\bibliographystyle{plain}\n\\bibliography{refs}\n\\end{document}\n",
        )
        .unwrap();

        let cap1_path = chap_dir.join("cap1.tex");
        std::fs::write(
            &cap1_path,
            "% !TeX root = ../main.tex\nCitiamo Knuth~\\cite{knuth1984}.\n",
        )
        .unwrap();

        let bib_recipe = crate::recipes::builtin_recipes()
            .into_iter()
            .find(|r| r.id == "pdflatex_bibtex")
            .unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let res = rt
            .block_on(compile_with_recipe(
                cap1_path.clone(),
                None,
                bib_recipe,
                false,
                2,
                1,
                500,
            ))
            .expect("bib recipe compilation result");

        assert!(res.success, "expected recipe success, log:\n{}", res.log);
        assert!(res.pdf_path.as_ref().is_some_and(|p| p.exists()));
        assert!(res.log.contains("=== [2/4] BibTeX ==="));
        assert!(res.log.contains("=== !TeX root attivo:"));
        assert!(dir.join(".build").join("main.bbl").exists());

        // Also verify default single-step `pdfLaTeX` auto-runs BibTeX + pinned `master_override`
        let _ = std::fs::remove_dir_all(dir.join(".build"));
        let cap2_path = chap_dir.join("cap2.tex");
        std::fs::write(&cap2_path, " Capitolo senza magic comment.\n").unwrap();
        let default_recipe = crate::recipes::builtin_recipes().into_iter().next().unwrap();
        let auto_res = rt
            .block_on(compile_with_recipe(
                cap2_path,
                Some(main_path.clone()),
                default_recipe,
                false,
                1,
                1,
                500,
            ))
            .expect("auto bib + master override");
        assert!(auto_res.success, "auto bib failed:\n{}", auto_res.log);
        assert!(auto_res.log.contains("=== [auto] BibTeX ==="));
        assert!(dir.join(".build").join("main.bbl").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compiles_with_lualatex_recipe() {
        if std::process::Command::new("lualatex").arg("--version").output().is_err() {
            return;
        }
        let dir = std::env::temp_dir().join("feathertex_lualatex_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let tex = dir.join("lua_doc.tex");
        std::fs::write(
            &tex,
            "\\documentclass{article}\n\\begin{document}\nCiao da LuaLaTeX!\n\\end{document}\n",
        )
        .unwrap();

        let lua_recipe = crate::recipes::builtin_recipes()
            .into_iter()
            .find(|r| r.id == "lualatex")
            .unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let res = rt
            .block_on(compile_with_recipe(tex, None, lua_recipe, false, 3, 1, 500))
            .expect("lualatex result");

        assert!(res.success, "expected lualatex success, log:\n{}", res.log);
        assert!(res.log.contains("=== [1/1] LuaLaTeX ==="));
        assert!(res.pdf_path.as_ref().is_some_and(|p| p.exists()));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
