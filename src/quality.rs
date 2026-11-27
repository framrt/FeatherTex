use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use tokio::io::AsyncWrite;
use tokio::process::Command;

async fn write_and_close_stdin(mut stdin: tokio::process::ChildStdin, mut buf: &[u8]) {
    while !buf.is_empty() {
        match std::future::poll_fn(|cx| Pin::new(&mut stdin).poll_write(cx, buf)).await {
            Ok(0) | Err(_) => break,
            Ok(n) => buf = &buf[n..],
        }
    }
    let _ = std::future::poll_fn(|cx| Pin::new(&mut stdin).poll_shutdown(cx)).await;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WordCountStats {
    pub body_words: usize,
    pub header_words: usize,
    pub caption_words: usize,
    pub inline_math: usize,
    pub display_math: usize,
}

impl WordCountStats {
    pub fn total_words(self) -> usize {
        self.body_words + self.header_words + self.caption_words
    }
}

/// Parses a `texcount -brief -total -1` output line such as `120+8+4 (2/0/15/3) Total`.
pub fn parse_texcount_brief(stdout: &str) -> Option<WordCountStats> {
    for raw in stdout.lines().rev() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let first_tok = line.split_whitespace().next()?;
        let mut plus_parts = first_tok.split('+');
        let body: usize = plus_parts.next()?.parse().ok()?;
        let headers: usize = plus_parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let captions: usize = plus_parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);

        let mut inline_math = 0usize;
        let mut display_math = 0usize;
        if let (Some(lp), Some(rp)) = (line.find('('), line.find(')')) {
            if rp > lp {
                let nums: Vec<usize> = line[lp + 1..rp]
                    .split('/')
                    .map(|s| s.trim().parse().unwrap_or(0))
                    .collect();
                if nums.len() >= 4 {
                    inline_math = nums[2];
                    display_math = nums[3];
                }
            }
        }

        return Some(WordCountStats {
            body_words: body,
            header_words: headers,
            caption_words: captions,
            inline_math,
            display_math,
        });
    }
    None
}

/// Runs `texcount` asynchronously (either across multi-file `\input` when `root_file` is clean on
/// disk, or piped from memory for live unsaved edits).
pub async fn run_texcount(
    root_file: Option<PathBuf>,
    use_disk_inc: bool,
    unsaved_text: String,
) -> Option<WordCountStats> {
    if use_disk_inc {
        if let Some(ref path) = root_file {
            if let Some(parent) = path.parent() {
                if let Ok(out) = Command::new("texcount")
                    .current_dir(parent)
                    .kill_on_drop(true)
                    .args(["-inc", "-brief", "-total", "-1"])
                    .arg(path)
                    .output()
                    .await
                {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    if let Some(stats) = parse_texcount_brief(&stdout) {
                        return Some(stats);
                    }
                }
            }
        }
    }

    let mut child = Command::new("texcount")
        .kill_on_drop(true)
        .args(["-brief", "-total", "-1", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    if let Some(stdin) = child.stdin.take() {
        write_and_close_stdin(stdin, unsaved_text.as_bytes()).await;
    }
    let out = child.wait_with_output().await.ok()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    parse_texcount_brief(&stdout)
}

// ============================================================================
// Typographic Linter (`chktex`)
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintWarning {
    pub line: usize,
    pub col: usize,
    pub code: usize,
    pub title: String,
    pub detail: String,
}

fn humanize_chktex_warning(code: usize, raw_msg: &str) -> (String, String) {
    match code {
        1 => (
            "Comando terminato da spazio".to_string(),
            "Controlla lo spazio dopo la macro o usa {} se necessario.".to_string(),
        ),
        2 => (
            "Spazio indivisibile '~' consigliato".to_string(),
            "Usa '~' prima di \\cite, \\ref o \\eqref per evitare andate a capo isolate (es. Teorema~\\ref{...}).".to_string(),
        ),
        3 => (
            "Parentesi graffe attorno a apice/pedice".to_string(),
            "Racchiudi espressioni in ^ o _ tra graffe esplicite (es. x_{i}).".to_string(),
        ),
        6 => (
            "Manca spazio dopo punteggiatura/corsivo".to_string(),
            "Verifica la spaziatura o la correzione del corsivo (\\/).".to_string(),
        ),
        8 => (
            "Lunghezza del trattino non tipografica".to_string(),
            "Usa '--' per intervalli di numeri/pagine (es. 10--20) e '---' per incisi.".to_string(),
        ),
        11 | 12 | 13 => (
            "Spaziatura inter-frase (\\@)".to_string(),
            "Usa '\\ ' dopo abbreviazioni (es. 'e.g.\\ ') o '\\@.' dopo maiuscole a fine frase.".to_string(),
        ),
        17 | 9 | 10 => (
            "Delimitatori non bilanciati".to_string(),
            raw_msg.to_string(),
        ),
        18 => (
            "Virgolette dritte \"...\" usate nel testo".to_string(),
            "In LaTeX usa ``...'' oppure \\enquote{...} (pacchetto csquotes) invece di \"...\".".to_string(),
        ),
        24 => (
            "Posizione di \\label prima di \\caption".to_string(),
            "Nelle figure/tabelle inserisci sempre \\label{...} DOPO \\caption{...}.".to_string(),
        ),
        26 => (
            "Spazio prima della punteggiatura".to_string(),
            "Rimuovi lo spazio superfluo prima di ',', '.', ';', ':' o '?'.".to_string(),
        ),
        36 | 37 => (
            "Spazio attorno alle parentesi".to_string(),
            raw_msg.to_string(),
        ),
        44 => (
            "Regola orizzontale \\hline sconsigliata".to_string(),
            "Per pubblicazioni scientifiche preferisci \\toprule, \\midrule e \\bottomrule (pacchetto booktabs).".to_string(),
        ),
        _ => (
            format!("Avviso tipografico #{code}"),
            raw_msg.to_string(),
        ),
    }
}

pub fn parse_chktex_output(stdout: &str) -> Vec<LintWarning> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for raw in stdout.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(5, ':');
        let (Some(l_s), Some(c_s), Some(n_s), Some(_kind), Some(msg)) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            continue;
        };
        let Ok(line_no) = l_s.trim().parse::<usize>() else {
            continue;
        };
        let col_no = c_s.trim().parse::<usize>().unwrap_or(1);
        let code = n_s.trim().parse::<usize>().unwrap_or(0);
        if !seen.insert((line_no, code)) {
            continue;
        }
        let (title, detail) = humanize_chktex_warning(code, msg.trim());
        out.push(LintWarning {
            line: line_no.max(1),
            col: col_no,
            code,
            title,
            detail,
        });
        if out.len() >= 150 {
            break;
        }
    }
    out
}

pub async fn run_chktex(source: String) -> Result<Vec<LintWarning>, String> {
    let mut child = Command::new("chktex")
        .kill_on_drop(true)
        .args(["-q", "-f", "%l:%c:%n:%k:%m\n"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "chktex non trovato nel sistema (installa texlive-binextra).".to_string()
            } else {
                format!("Impossibile avviare chktex: {e}")
            }
        })?;

    if let Some(stdin) = child.stdin.take() {
        write_and_close_stdin(stdin, source.as_bytes()).await;
    }
    let out = child
        .wait_with_output()
        .await
        .map_err(|e| format!("Errore chktex: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    Ok(parse_chktex_output(&stdout))
}

// ============================================================================
// LaTeX-aware Spellchecker (`hunspell` IT / EN)
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellIssue {
    pub line: usize,
    pub word: String,
    pub suggestions: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum SpellCheckOutcome {
    Ok(Vec<SpellIssue>),
    MissingDictionary { lang: String, hint: String },
    Error(String),
}

const SKIP_ENVS: &[&str] = &[
    "equation",
    "equation*",
    "align",
    "align*",
    "gather",
    "gather*",
    "multline",
    "multline*",
    "flalign",
    "flalign*",
    "alignat",
    "alignat*",
    "math",
    "displaymath",
    "tikzpicture",
    "tikzcd",
    "pgfpicture",
    "lstlisting",
    "minted",
    "verbatim",
    "tabular",
    "tabular*",
    "array",
    "pmatrix",
    "bmatrix",
    "vmatrix",
    "cases",
    "dcases",
    "thebibliography",
];

const SKIP_ARG_CMDS: &[&str] = &[
    "cite",
    "citet",
    "citep",
    "parencite",
    "textcite",
    "autocite",
    "nocite",
    "Cite",
    "ref",
    "eqref",
    "cref",
    "Cref",
    "autoref",
    "pageref",
    "nameref",
    "label",
    "usepackage",
    "documentclass",
    "bibliographystyle",
    "bibliography",
    "addbibresource",
    "input",
    "include",
    "subfile",
    "includegraphics",
    "url",
    "nolinkurl",
    "texttt",
    "gls",
    "acrshort",
    "acrlong",
    "acrfull",
    "orcidlink",
    "definecolor",
    "color",
    "pgfplotsset",
    "usetikzlibrary",
    "newtheorem",
    "DeclareMathOperator",
    "newcommand",
    "renewcommand",
];

/// Extracts unique prose words along with the 1-based line number of their first appearance,
/// ignoring preamble setup, `%` comments, `$...$`/`\[...\]` math zones, math/code environments,
/// and non-prose command arguments (`\cite{...}`, `\ref{...}`, `\label{...}`, etc.).
pub fn extract_prose_words(latex: &str) -> Vec<(usize, String)> {
    let chars: Vec<char> = latex.chars().collect();
    let len = chars.len();
    let mut i = 0usize;
    let mut line = 1usize;

    // If `\begin{document}` exists, jump to it (while still counting lines accurately!).
    if let Some(byte_idx) = latex.find("\\begin{document}") {
        line += latex[..byte_idx].bytes().filter(|&b| b == b'\n').count();
        let char_offset = latex[..byte_idx].chars().count() + "\\begin{document}".chars().count();
        i = char_offset.min(len);
    }

    let mut in_inline_dollar = false;
    let mut in_display_dollar = false;
    let mut in_paren_math = false;
    let mut in_bracket_math = false;
    let mut skip_env_depth = 0usize;

    let mut first_line_for_word: HashMap<String, usize> = HashMap::new();
    let mut ordered_words: Vec<(usize, String)> = Vec::new();

    while i < len {
        let ch = chars[i];
        if ch == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if ch == '%' {
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        if ch == '\\' {
            if i + 1 >= len {
                break;
            }
            let next = chars[i + 1];
            match next {
                '(' => {
                    in_paren_math = true;
                    i += 2;
                    continue;
                }
                ')' => {
                    in_paren_math = false;
                    i += 2;
                    continue;
                }
                '[' => {
                    in_bracket_math = true;
                    i += 2;
                    continue;
                }
                ']' => {
                    in_bracket_math = false;
                    i += 2;
                    continue;
                }
                ch2 if !ch2.is_ascii_alphabetic() => {
                    if ch2 == '\n' {
                        line += 1;
                    }
                    i += 2;
                    continue;
                }
                _ => {
                    // Parse command name
                    let cmd_start = i + 1;
                    let mut cmd_end = cmd_start;
                    while cmd_end < len && chars[cmd_end].is_ascii_alphabetic() {
                        cmd_end += 1;
                    }
                    let cmd: String = chars[cmd_start..cmd_end].iter().collect();
                    i = cmd_end;
                    if i < len && chars[i] == '*' {
                        i += 1;
                    }

                    if cmd == "end" {
                        if let Some((env_name, new_i, nl)) = consume_brace_group(&chars, i) {
                            line += nl;
                            i = new_i;
                            if env_name == "document" {
                                break;
                            }
                            if SKIP_ENVS.contains(&env_name.trim()) {
                                skip_env_depth = skip_env_depth.saturating_sub(1);
                            }
                        }
                        continue;
                    }

                    if cmd == "begin" {
                        if let Some((env_name, new_i, nl)) = consume_brace_group(&chars, i) {
                            line += nl;
                            i = new_i;
                            if SKIP_ENVS.contains(&env_name.trim()) {
                                skip_env_depth += 1;
                            }
                        }
                        continue;
                    }

                    if cmd == "href" {
                        // Skip URL `{...}` first argument, keep second `{text}` argument for spellcheck!
                        if let Some((_, new_i, nl)) = consume_brace_group(&chars, i) {
                            line += nl;
                            i = new_i;
                        }
                        continue;
                    }

                    if SKIP_ARG_CMDS.contains(&cmd.as_str()) {
                        // Skip optional `[...]` and mandatory `{...}` arguments
                        loop {
                            let mut probe = i;
                            let mut nl_ws = 0usize;
                            while probe < len && chars[probe].is_whitespace() {
                                if chars[probe] == '\n' {
                                    nl_ws += 1;
                                }
                                probe += 1;
                            }
                            if probe < len && chars[probe] == '[' {
                                if let Some((_, next_p, nl)) =
                                    consume_delimited(&chars, probe, '[', ']')
                                {
                                    line += nl_ws + nl;
                                    i = next_p;
                                    continue;
                                }
                            } else if probe < len && chars[probe] == '{' {
                                if let Some((_, next_p, nl)) =
                                    consume_delimited(&chars, probe, '{', '}')
                                {
                                    line += nl_ws + nl;
                                    i = next_p;
                                }
                            }
                            break;
                        }
                    }
                    continue;
                }
            }
        }

        if ch == '$' {
            if i + 1 < len && chars[i + 1] == '$' {
                if !in_inline_dollar {
                    in_display_dollar = !in_display_dollar;
                }
                i += 2;
            } else {
                if !in_display_dollar && !in_bracket_math && skip_env_depth == 0 {
                    in_inline_dollar = !in_inline_dollar;
                }
                i += 1;
            }
            continue;
        }

        let in_math_or_skip = in_inline_dollar
            || in_display_dollar
            || in_paren_math
            || in_bracket_math
            || skip_env_depth > 0;

        if in_math_or_skip {
            i += 1;
            continue;
        }

        if ch.is_alphabetic() {
            let start = i;
            let word_line = line;
            while i < len
                && (chars[i].is_alphabetic()
                    || (matches!(chars[i], '\'' | '’')
                        && i + 1 < len
                        && chars[i + 1].is_alphabetic()))
            {
                i += 1;
            }
            let raw_word: String = chars[start..i].iter().collect();
            // Split Italian elisions like `l'insieme` / `un'ipotesi` so `insieme`/`ipotesi` check cleanly
            for part in raw_word.split(['\'', '’']) {
                let char_len = part.chars().count();
                if char_len >= 2 && !part.chars().all(|c| c.is_uppercase()) {
                    if !first_line_for_word.contains_key(part) {
                        first_line_for_word.insert(part.to_string(), word_line);
                        ordered_words.push((word_line, part.to_string()));
                    }
                }
            }
            continue;
        }

        i += 1;
    }

    ordered_words
}

fn consume_brace_group(chars: &[char], from: usize) -> Option<(String, usize, usize)> {
    let mut p = from;
    let mut nl = 0usize;
    while p < chars.len() && chars[p].is_whitespace() {
        if chars[p] == '\n' {
            nl += 1;
        }
        p += 1;
    }
    let (content, next_p, inner_nl) = consume_delimited(chars, p, '{', '}')?;
    Some((content, next_p, nl + inner_nl))
}

fn consume_delimited(
    chars: &[char],
    from: usize,
    open: char,
    close: char,
) -> Option<(String, usize, usize)> {
    if chars.get(from).copied() != Some(open) {
        return None;
    }
    let mut depth = 1usize;
    let mut idx = from + 1;
    let mut nl = 0usize;
    let mut buf = String::new();
    while idx < chars.len() {
        let c = chars[idx];
        if c == '\n' {
            nl += 1;
        }
        if c == open {
            depth += 1;
            buf.push(c);
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some((buf, idx + 1, nl));
            }
            buf.push(c);
        } else {
            buf.push(c);
        }
        idx += 1;
    }
    None
}

pub async fn run_hunspell(source: String, lang: String) -> SpellCheckOutcome {
    let words = extract_prose_words(&source);
    if words.is_empty() {
        return SpellCheckOutcome::Ok(Vec::new());
    }

    let mut line_map: HashMap<String, usize> = HashMap::with_capacity(words.len());
    let mut payload = String::new();
    for (ln, w) in &words {
        line_map.entry(w.clone()).or_insert(*ln);
        payload.push('^');
        payload.push_str(w);
        payload.push('\n');
    }

    let mut child = match Command::new("hunspell")
        .kill_on_drop(true)
        .args(["-a", "-d", &lang])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                return SpellCheckOutcome::Error(
                    "hunspell non è installato. Installalo con: sudo pacman -S hunspell hunspell-it hunspell-en_us".to_string(),
                );
            }
            return SpellCheckOutcome::Error(format!("Impossibile avviare hunspell: {e}"));
        }
    };

    if let Some(stdin) = child.stdin.take() {
        write_and_close_stdin(stdin, payload.as_bytes()).await;
    }

    let out = match child.wait_with_output().await {
        Ok(o) => o,
        Err(e) => return SpellCheckOutcome::Error(format!("Errore hunspell: {e}")),
    };

    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("Can't open affix or dictionary")
        || stderr.contains("Impossibile aprire i file di affisso o dizionario")
    {
        let pkgs = if lang.contains("it") && lang.contains("en") {
            "hunspell-it hunspell-en_us"
        } else if lang.contains("it") {
            "hunspell-it"
        } else {
            "hunspell-en_us"
        };
        return SpellCheckOutcome::MissingDictionary {
            lang,
            hint: format!("sudo pacman -S {pkgs}"),
        };
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut issues = Vec::new();
    for raw in stdout.lines() {
        let trimmed = raw.trim();
        if let Some(rest) = trimmed.strip_prefix("& ") {
            // Format: `& <word> <count> <offset>: <sug1>, <sug2>, ...`
            let mut left_right = rest.splitn(2, ':');
            let left = left_right.next().unwrap_or("");
            let right = left_right.next().unwrap_or("");
            let Some(word) = left.split_whitespace().next() else {
                continue;
            };
            let suggestions: Vec<String> = right
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .take(5)
                .collect();
            let line = line_map.get(word).copied().unwrap_or(1);
            issues.push(SpellIssue {
                line,
                word: word.to_string(),
                suggestions,
            });
        } else if let Some(rest) = trimmed.strip_prefix("# ") {
            // Format: `# <word> <offset>` (no suggestions)
            if let Some(word) = rest.split_whitespace().next() {
                let line = line_map.get(word).copied().unwrap_or(1);
                issues.push(SpellIssue {
                    line,
                    word: word.to_string(),
                    suggestions: Vec::new(),
                });
            }
        }
        if issues.len() >= 150 {
            break;
        }
    }

    issues.sort_by_key(|i| i.line);
    SpellCheckOutcome::Ok(issues)
}

// ============================================================================
// `latexdiff` Revision Generator (vs Git HEAD or previous `.tex` file)
// ============================================================================

/// Generates `.build/<stem>_diff.tex` comparing `old_tex_path` against `current_tex_path`.
pub async fn generate_latexdiff_tex(
    old_tex_path: &Path,
    current_tex_path: &Path,
) -> Result<PathBuf, String> {
    let parent_dir = current_tex_path
        .parent()
        .ok_or_else(|| "Cartella del documento non trovata".to_string())?;
    let stem = current_tex_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("documento");
    let build_dir = parent_dir.join(".build");
    tokio::fs::create_dir_all(&build_dir)
        .await
        .map_err(|e| format!("Impossibile creare .build: {e}"))?;

    let out = Command::new("latexdiff")
        .current_dir(parent_dir)
        .kill_on_drop(true)
        .arg("--flatten")
        .arg(old_tex_path)
        .arg(current_tex_path)
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "latexdiff non trovato nel sistema (installa texlive-binextra).".to_string()
            } else {
                format!("Impossibile avviare latexdiff: {e}")
            }
        })?;

    if !out.status.success() || out.stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("Errore latexdiff: {}", stderr.trim()));
    }

    let diff_tex = build_dir.join(format!("{stem}_diff.tex"));
    tokio::fs::write(&diff_tex, &out.stdout)
        .await
        .map_err(|e| format!("Impossibile salvare il file diff: {e}"))?;
    Ok(diff_tex)
}

/// Extracts `HEAD:<rel_path>` from Git into `.build/<stem>_git_head.tex` and runs `latexdiff`.
pub async fn generate_latexdiff_against_git_head(current_tex_path: PathBuf) -> Result<PathBuf, String> {
    let parent_dir = current_tex_path
        .parent()
        .ok_or_else(|| "Cartella del documento non trovata".to_string())?;
    let stem = current_tex_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("documento");

    let top_out = Command::new("git")
        .current_dir(parent_dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .await
        .map_err(|e| format!("Impossibile eseguire git: {e}"))?;

    if !top_out.status.success() {
        return Err(
            " Questa cartella non è un repository Git. Usa 'Confronta con file .tex...' per scegliere una versione precedente."
                .trim()
                .to_string(),
        );
    }

    let repo_root = PathBuf::from(String::from_utf8_lossy(&top_out.stdout).trim());
    let canon_cur =
        std::fs::canonicalize(&current_tex_path).unwrap_or_else(|_| current_tex_path.clone());
    let canon_root = std::fs::canonicalize(&repo_root).unwrap_or(repo_root);
    let rel_path = canon_cur
        .strip_prefix(&canon_root)
        .map_err(|_| "Impossibile calcolare il percorso relativo nel repository Git".to_string())?;

    let git_spec = format!("HEAD:{}", rel_path.to_string_lossy().replace('\\', "/"));
    let show_out = Command::new("git")
        .current_dir(&canon_root)
        .args(["show", &git_spec])
        .output()
        .await
        .map_err(|e| format!("Errore lettura Git HEAD: {e}"))?;

    if !show_out.status.success() {
        return Err(format!(
            "Il file '{}' non è ancora presente in Git HEAD (fai prima un commit oppure confronta con un file sul disco).",
            rel_path.display()
        ));
    }

    let build_dir = parent_dir.join(".build");
    tokio::fs::create_dir_all(&build_dir)
        .await
        .map_err(|e| format!("Impossibile creare .build: {e}"))?;
    let old_path = build_dir.join(format!("{stem}_git_head.tex"));
    tokio::fs::write(&old_path, &show_out.stdout)
        .await
        .map_err(|e| format!("Impossibile scrivere snapshot Git: {e}"))?;

    generate_latexdiff_tex(&old_path, &current_tex_path).await
}

/// Opens a file dialog to select an older `.tex` version and generates `.build/<stem>_diff.tex`.
pub async fn pick_old_tex_and_latexdiff(current_tex_path: PathBuf) -> Result<PathBuf, String> {
    let handle = rfd::AsyncFileDialog::new()
        .set_title("Seleziona la versione precedente (.tex) per latexdiff")
        .add_filter("File LaTeX", &["tex"])
        .pick_file()
        .await
        .ok_or_else(|| "Selezione annullata".to_string())?;

    generate_latexdiff_tex(handle.path(), &current_tex_path).await
}

pub const APP_VERSION: &str = env!("FEATHERTEX_APP_VERSION");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: String,
    pub url: String,
    pub deb_url: Option<String>,
    pub linux_bin_url: Option<String>,
    pub win_zip_url: Option<String>,
}

fn parse_semver_tuple(raw: &str) -> Option<(u32, u32, u32)> {
    let clean = raw.trim().trim_start_matches(['v', 'V']);
    let core = clean.split(['-', '+']).next().unwrap_or(clean);
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u32>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u32>().ok()?;
    Some((major, minor, patch))
}

pub fn is_newer_version(latest: &str, current: &str) -> bool {
    match (parse_semver_tuple(latest), parse_semver_tuple(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

fn extract_json_string_field(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let pos = json.find(&needle)?;
    let after = &json[pos + needle.len()..];
    let colon = after.find(':')?;
    let rest = after[colon + 1..].trim_start();
    let inner = rest.strip_prefix('"')?;
    let mut out = String::new();
    let mut escaped = false;
    for ch in inner.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(out);
        } else {
            out.push(ch);
        }
    }
    None
}

fn extract_release_asset_urls(json: &str) -> (Option<String>, Option<String>, Option<String>) {
    let mut deb_url = None;
    let mut linux_bin_url = None;
    let mut win_zip_url = None;

    let needle = "\"browser_download_url\"";
    let mut rest = json;
    while let Some(pos) = rest.find(needle) {
        rest = &rest[pos + needle.len()..];
        let Some(colon) = rest.find(':') else {
            break;
        };
        let after_colon = rest[colon + 1..].trim_start();
        let Some(inner) = after_colon.strip_prefix('"') else {
            continue;
        };
        let mut url = String::new();
        let mut escaped = false;
        let mut closed = false;
        for ch in inner.chars() {
            if escaped {
                url.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                closed = true;
                break;
            } else {
                url.push(ch);
            }
        }
        if !closed || !url.starts_with("https://github.com/") {
            continue;
        }
        let lower = url.to_ascii_lowercase();
        if lower.ends_with(".deb") && deb_url.is_none() {
            deb_url = Some(url);
        } else if lower.contains("linux-x86_64") && !lower.ends_with(".zip") && linux_bin_url.is_none() {
            linux_bin_url = Some(url);
        } else if lower.contains("windows") && lower.ends_with(".zip") && win_zip_url.is_none() {
            win_zip_url = Some(url);
        }
    }

    (deb_url, linux_bin_url, win_zip_url)
}

pub async fn check_for_update() -> Option<UpdateInfo> {
    let url = "https://api.github.com/repos/framrt/FeatherTex/releases/latest";
    let curl_out = Command::new("curl")
        .kill_on_drop(true)
        .args([
            "-sSL",
            "--fail",
            "--max-time",
            "6",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: FeatherTex",
            "--",
            url,
        ])
        .output()
        .await;

    let stdout_bytes = match curl_out {
        Ok(o) if o.status.success() && !o.stdout.is_empty() => o.stdout,
        _ => {
            let wget_out = Command::new("wget")
                .kill_on_drop(true)
                .args([
                    "-q",
                    "--timeout=6",
                    "--https-only",
                    "--header=Accept: application/vnd.github+json",
                    "--header=User-Agent: FeatherTex",
                    "-O",
                    "-",
                    "--",
                    url,
                ])
                .output()
                .await
                .ok()?;
            if !wget_out.status.success() {
                return None;
            }
            wget_out.stdout
        }
    };

    let body = String::from_utf8_lossy(&stdout_bytes);
    let tag = extract_json_string_field(&body, "tag_name")?;
    let version = tag.trim().trim_start_matches(['v', 'V']).to_string();
    if !is_newer_version(&version, APP_VERSION) {
        return None;
    }
    let html_url = extract_json_string_field(&body, "html_url")
        .filter(|u| u.starts_with("https://github.com/"))
        .unwrap_or_else(|| "https://github.com/framrt/FeatherTex/releases/latest".to_string());
    let (deb_url, linux_bin_url, win_zip_url) = extract_release_asset_urls(&body);

    Some(UpdateInfo {
        version,
        url: html_url,
        deb_url,
        linux_bin_url,
        win_zip_url,
    })
}

async fn download_https_to_file(url: &str, dest: &Path) -> Result<(), String> {
    let curl_res = Command::new("curl")
        .kill_on_drop(true)
        .args([
            "-sSL",
            "--fail",
            "--max-time",
            "120",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "-H",
            "User-Agent: FeatherTex",
            "-o",
        ])
        .arg(dest)
        .arg("--")
        .arg(url)
        .output()
        .await;

    match curl_res {
        Ok(out) if out.status.success() => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let wget_out = Command::new("wget")
                .kill_on_drop(true)
                .args([
                    "-q",
                    "--timeout=120",
                    "--https-only",
                    "--header=User-Agent: FeatherTex",
                    "-O",
                ])
                .arg(dest)
                .arg("--")
                .arg(url)
                .output()
                .await
                .map_err(|we| format!("Né curl né wget disponibili per scaricare: {we}"))?;
            if !wget_out.status.success() {
                let stderr = String::from_utf8_lossy(&wget_out.stderr);
                return Err(format!("Download fallito (wget): {}", stderr.trim()));
            }
            Ok(())
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            Err(format!("Download fallito: {}", stderr.trim()))
        }
        Err(e) => Err(format!("Impossibile avviare curl: {e}")),
    }
}

#[cfg(target_os = "linux")]
async fn pipe_bytes_through_tar(tar_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut child = Command::new("tar")
        .kill_on_drop(true)
        .args(["-xO", "./usr/bin/feathertex"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Impossibile avviare tar: {e}"))?;

    if let Some(stdin) = child.stdin.take() {
        write_and_close_stdin(stdin, tar_bytes).await;
    }
    let out = child
        .wait_with_output()
        .await
        .map_err(|e| format!("Errore estrazione tar: {e}"))?;
    if out.status.success() && !out.stdout.is_empty() {
        Ok(out.stdout)
    } else {
        Err(format!(
            "Impossibile estrarre binario dal pacchetto: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

#[cfg(target_os = "linux")]
async fn extract_binary_from_deb(deb_path: &Path, bin_dest: &Path) -> Result<(), String> {
    // Path 1: `dpkg-deb --fsys-tarfile` (Debian/Ubuntu/Mint)
    if let Ok(dpkg_out) = Command::new("dpkg-deb")
        .kill_on_drop(true)
        .arg("--fsys-tarfile")
        .arg(deb_path)
        .output()
        .await
    {
        if dpkg_out.status.success() && !dpkg_out.stdout.is_empty() {
            let bin_bytes = pipe_bytes_through_tar(&dpkg_out.stdout).await?;
            tokio::fs::write(bin_dest, &bin_bytes)
                .await
                .map_err(|e| format!("Errore scrittura binario estratto: {e}"))?;
            return Ok(());
        }
    }

    // Path 2: `ar` (binutils, works on Arch Linux without dpkg)
    let list_out = Command::new("ar")
        .kill_on_drop(true)
        .arg("t")
        .arg(deb_path)
        .output()
        .await
        .map_err(|_| {
            "Strumento 'dpkg-deb' o 'ar' non trovato per estrarre l'aggiornamento".to_string()
        })?;
    if !list_out.status.success() {
        return Err("Impossibile leggere il contenuto del pacchetto .deb".to_string());
    }
    let archive_name = String::from_utf8_lossy(&list_out.stdout)
        .lines()
        .find(|l| l.trim().starts_with("data.tar"))
        .map(|l| l.trim().to_string())
        .ok_or_else(|| "Archivio data.tar non trovato nel .deb".to_string())?;

    let ar_out = Command::new("ar")
        .kill_on_drop(true)
        .arg("p")
        .arg(deb_path)
        .arg(&archive_name)
        .output()
        .await
        .map_err(|e| format!("Errore estrazione ar: {e}"))?;
    if !ar_out.status.success() || ar_out.stdout.is_empty() {
        return Err("Estrazione data.tar dal .deb fallita".to_string());
    }

    let bin_bytes = pipe_bytes_through_tar(&ar_out.stdout).await?;
    tokio::fs::write(bin_dest, &bin_bytes)
        .await
        .map_err(|e| format!("Errore scrittura binario estratto: {e}"))?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn is_dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".feathertex_write_probe_{}", std::process::id()));
    if std::fs::File::create(&probe).is_ok() {
        let _ = std::fs::remove_file(&probe);
        true
    } else {
        false
    }
}

pub async fn install_update(info: UpdateInfo) -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;

        let current_exe = std::env::current_exe()
            .map_err(|e| format!("Impossibile individuare l'eseguibile corrente: {e}"))?;
        let exe_dir = current_exe
            .parent()
            .ok_or_else(|| "Cartella eseguibile non valida".to_string())?;
        let writable = is_dir_writable(exe_dir);
        let has_apt = Path::new("/usr/bin/apt-get").is_file() || Path::new("/bin/apt-get").is_file();
        let has_pkexec =
            Path::new("/usr/bin/pkexec").is_file() || Path::new("/bin/pkexec").is_file();

        // System-wide Debian/Mint/Ubuntu install: upgrade cleanly through apt-get via pkexec
        if !writable && has_apt && has_pkexec {
            if let Some(deb_url) = info.deb_url.as_deref() {
                let deb_path =
                    std::env::temp_dir().join(format!("feathertex_update_{}.deb", info.version));
                download_https_to_file(deb_url, &deb_path).await?;
                let status = Command::new("pkexec")
                    .arg("apt-get")
                    .arg("install")
                    .arg("-y")
                    .arg(&deb_path)
                    .output()
                    .await;
                let _ = tokio::fs::remove_file(&deb_path).await;
                let out = status.map_err(|e| format!("Errore avvio pkexec/apt-get: {e}"))?;
                if !out.status.success() {
                    let msg = String::from_utf8_lossy(&out.stderr);
                    return Err(format!("Installazione .deb annullata o fallita: {}", msg.trim()));
                }
                return Ok(info.version);
            }
        }

        // Portable binary or Arch Linux / other distros: download binary (or extract from .deb)
        let tmp_bin = std::env::temp_dir().join(format!("feathertex_bin_{}", info.version));
        if let Some(bin_url) = info.linux_bin_url.as_deref() {
            download_https_to_file(bin_url, &tmp_bin).await?;
        } else if let Some(deb_url) = info.deb_url.as_deref() {
            let deb_path =
                std::env::temp_dir().join(format!("feathertex_update_{}.deb", info.version));
            download_https_to_file(deb_url, &deb_path).await?;
            let ext_res = extract_binary_from_deb(&deb_path, &tmp_bin).await;
            let _ = tokio::fs::remove_file(&deb_path).await;
            ext_res?;
        } else {
            return Err("Nessun pacchetto Linux trovato nella Release GitHub.".to_string());
        }

        std::fs::set_permissions(&tmp_bin, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Impossibile rendere eseguibile il nuovo binario: {e}"))?;

        if writable {
            let sibling_tmp = current_exe.with_file_name(format!(
                ".feathertex_update_tmp_{}",
                std::process::id()
            ));
            let copy_res = tokio::fs::copy(&tmp_bin, &sibling_tmp).await;
            let _ = tokio::fs::remove_file(&tmp_bin).await;
            copy_res.map_err(|e| format!("Errore copia aggiornamento: {e}"))?;
            std::fs::set_permissions(&sibling_tmp, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("Errore permessi aggiornamento: {e}"))?;
            if let Err(e) = std::fs::rename(&sibling_tmp, &current_exe) {
                let _ = std::fs::remove_file(&sibling_tmp);
                return Err(format!("Errore sostituzione binario: {e}"));
            }
        } else if has_pkexec {
            let out = Command::new("pkexec")
                .args(["install", "-Dm755"])
                .arg(&tmp_bin)
                .arg(&current_exe)
                .output()
                .await
                .map_err(|e| format!("Errore avvio pkexec: {e}"))?;
            let _ = tokio::fs::remove_file(&tmp_bin).await;
            if !out.status.success() {
                return Err("Autorizzazione pkexec annullata o installazione fallita.".to_string());
            }
        } else {
            let _ = tokio::fs::remove_file(&tmp_bin).await;
            return Err(
                "Cartella di installazione protetta e 'pkexec' non disponibile.".to_string(),
            );
        }

        Ok(info.version)
    }

    #[cfg(target_os = "windows")]
    {
        let zip_url = info
            .win_zip_url
            .as_deref()
            .ok_or_else(|| "Nessun pacchetto Windows trovato nella Release.".to_string())?;
        let current_exe = std::env::current_exe()
            .map_err(|e| format!("Impossibile individuare feathertex.exe: {e}"))?;
        let stage_dir =
            std::env::temp_dir().join(format!("feathertex_win_update_{}", info.version));
        let _ = tokio::fs::remove_dir_all(&stage_dir).await;
        tokio::fs::create_dir_all(&stage_dir)
            .await
            .map_err(|e| format!("Errore creazione cartella temporanea: {e}"))?;
        let zip_path = stage_dir.join("update.zip");
        download_https_to_file(zip_url, &zip_path).await?;

        let extract_cmd = format!(
            "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
            zip_path.display().to_string().replace('\'', "''"),
            stage_dir.display().to_string().replace('\'', "''"),
        );
        let extract_out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &extract_cmd])
            .output()
            .await
            .map_err(|e| format!("Errore PowerShell: {e}"))?;
        if !extract_out.status.success() {
            return Err("Impossibile estrarre lo ZIP di aggiornamento.".to_string());
        }

        let new_exe = stage_dir.join("feathertex.exe");
        if !new_exe.is_file() {
            return Err("feathertex.exe non trovato nell'archivio ZIP.".to_string());
        }

        // Schedule replacement + relaunch after process exits
        let new_exe_str = new_exe.display().to_string().replace('\'', "''");
        let cur_exe_str = current_exe.display().to_string().replace('\'', "''");
        let swap_script = format!(
            "Start-Sleep -Seconds 2; Move-Item -LiteralPath '{new_exe_str}' -Destination '{cur_exe_str}' -Force; Start-Process -FilePath '{cur_exe_str}'"
        );
        std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-WindowStyle",
                "Hidden",
                "-Command",
                &swap_script,
            ])
            .spawn()
            .map_err(|e| format!("Impossibile avviare il programma di aggiornamento: {e}"))?;

        Ok(info.version)
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = info;
        Err("Aggiornamento automatico non supportato su questo sistema.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_texcount_brief_output() {
        let sample = "142+12+6 (4/1/18/5) Total\n";
        let stats = parse_texcount_brief(sample).expect("parsed texcount");
        assert_eq!(stats.body_words, 142);
        assert_eq!(stats.header_words, 12);
        assert_eq!(stats.caption_words, 6);
        assert_eq!(stats.total_words(), 160);
        assert_eq!(stats.inline_math, 18);
        assert_eq!(stats.display_math, 5);
    }

    #[test]
    fn extracts_prose_words_ignoring_math_and_citations() {
        let src = "\
\\documentclass{article}
\\usepackage{amsmath}
\\begin{document}
\\section{Introduzione generale}
Dimostriamo il teorema~\\ref{thm:uno} citando~\\cite{knuth1984} con $\\alpha + \\beta = \\gamma$.
\\begin{align*}
  f(x) &= \\sin(x) + \\cos(y)
\\end{align*}
Un'ipotesi fondamentale. % commento ignoratoxyz
\\end{document}
";
        let words: Vec<String> = extract_prose_words(src).into_iter().map(|(_, w)| w).collect();
        assert!(words.contains(&"Introduzione".to_string()));
        assert!(words.contains(&"Dimostriamo".to_string()));
        assert!(words.contains(&"ipotesi".to_string()));
        assert!(words.contains(&"fondamentale".to_string()));
        // Must NOT contain LaTeX commands, citation keys, labels, math, or comments!
        assert!(!words.iter().any(|w| w == "knuth" || w == "thm" || w == "alpha" || w == "sin" || w == "ignoratoxyz"));
    }

    #[test]
    fn runs_chktex_and_latexdiff_end_to_end() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        if std::process::Command::new("chktex").arg("--version").output().is_ok() {
            let sample = "\\documentclass{article}\n\\begin{document}\nVedi \\cite{a} e \"ciao\" !\n\\end{document}\n";
            let warnings = rt.block_on(run_chktex(sample.to_string())).expect("chktex ok");
            assert!(!warnings.is_empty());
            assert!(warnings.iter().any(|w| w.code == 2 || w.code == 18 || w.code == 26));
        }

        if std::process::Command::new("latexdiff").arg("--version").output().is_ok() {
            let dir = std::env::temp_dir().join("feathertex_latexdiff_test");
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let old_f = dir.join("old.tex");
            let new_f = dir.join("new.tex");
            std::fs::write(
                &old_f,
                "\\documentclass{article}\n\\begin{document}\nVersione vecchia.\n\\end{document}\n",
            )
            .unwrap();
            std::fs::write(
                &new_f,
                "\\documentclass{article}\n\\begin{document}\nVersione nuova migliorata.\n\\end{document}\n",
            )
            .unwrap();
            let diff_path = rt
                .block_on(generate_latexdiff_tex(&old_f, &new_f))
                .expect("latexdiff generated");
            let diff_txt = std::fs::read_to_string(&diff_path).unwrap();
            assert!(diff_txt.contains("DIFadd") || diff_txt.contains("migliorata"));
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn parses_github_release_version_comparison() {
        assert!(is_newer_version("v0.2.0", "0.1.2"));
        assert!(is_newer_version("0.1.3", "0.1.2"));
        assert!(!is_newer_version("v0.1.2", "0.1.2"));
        assert!(!is_newer_version("v0.1.1", "0.1.2"));

        let sample = r#"{"tag_name": "v0.2.0", "html_url": "https://github.com/framrt/FeatherTex/releases/tag/v0.2.0", "assets": [{"browser_download_url": "https://github.com/framrt/FeatherTex/releases/download/v0.2.0/feathertex_0.2.0_amd64.deb"}, {"browser_download_url": "https://github.com/framrt/FeatherTex/releases/download/v0.2.0/feathertex-linux-x86_64"}, {"browser_download_url": "https://github.com/framrt/FeatherTex/releases/download/v0.2.0/feathertex-0.2.0-windows-x86_64.zip"}]}"#;
        assert_eq!(extract_json_string_field(sample, "tag_name").as_deref(), Some("v0.2.0"));
        assert_eq!(
            extract_json_string_field(sample, "html_url").as_deref(),
            Some("https://github.com/framrt/FeatherTex/releases/tag/v0.2.0")
        );
        let (deb, bin, win) = extract_release_asset_urls(sample);
        assert!(deb.unwrap().ends_with(".deb"));
        assert!(bin.unwrap().ends_with("feathertex-linux-x86_64"));
        assert!(win.unwrap().ends_with(".zip"));
    }
}

