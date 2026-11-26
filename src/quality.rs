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
}

