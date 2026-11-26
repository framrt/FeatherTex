use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineKind {
    Part,
    Chapter,
    Section,
    Subsection,
    Subsubsection,
    Theorem,
    Label,
    Include,
}

impl OutlineKind {
    pub fn indent_level(self) -> u16 {
        match self {
            OutlineKind::Part | OutlineKind::Chapter | OutlineKind::Include => 0,
            OutlineKind::Section => 0,
            OutlineKind::Subsection => 1,
            OutlineKind::Subsubsection => 2,
            OutlineKind::Theorem => 1,
            OutlineKind::Label => 2,
        }
    }

    pub fn badge(self) -> &'static str {
        match self {
            OutlineKind::Part => "PRT",
            OutlineKind::Chapter => "CAP",
            OutlineKind::Section => "§",
            OutlineKind::Subsection => "§§",
            OutlineKind::Subsubsection => "§§§",
            OutlineKind::Theorem => "THM",
            OutlineKind::Label => "LBL",
            OutlineKind::Include => "FILE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineFilter {
    All,
    Sections,
    Theorems,
    Labels,
    Files,
}

impl OutlineFilter {
    pub fn matches(self, kind: OutlineKind) -> bool {
        match self {
            OutlineFilter::All => true,
            OutlineFilter::Sections => matches!(
                kind,
                OutlineKind::Part
                    | OutlineKind::Chapter
                    | OutlineKind::Section
                    | OutlineKind::Subsection
                    | OutlineKind::Subsubsection
            ),
            OutlineFilter::Theorems => kind == OutlineKind::Theorem,
            OutlineFilter::Labels => kind == OutlineKind::Label,
            OutlineFilter::Files => kind == OutlineKind::Include,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineItem {
    pub kind: OutlineKind,
    pub title: String,
    pub file_path: Option<PathBuf>,
    pub file_name: String,
    pub line_1based: usize,
    pub from_included_file: bool,
}

const THEOREM_ENVS: &[(&str, &str)] = &[
    ("theorem", "Teorema"),
    ("theorem*", "Teorema"),
    ("lemma", "Lemma"),
    ("lemma*", "Lemma"),
    ("proposition", "Proposizione"),
    ("proposition*", "Proposizione"),
    ("corollary", "Corollario"),
    ("corollary*", "Corollario"),
    ("definition", "Definizione"),
    ("definition*", "Definizione"),
    ("remark", "Osservazione"),
    ("remark*", "Osservazione"),
    ("example", "Esempio"),
    ("example*", "Esempio"),
    ("conjecture", "Congettura"),
    ("claim", "Claim"),
];

/// Builds a multi-file project outline starting from `root_path` (or `unsaved_active` if unsaved),
/// recursively expanding `\input{...}`, `\include{...}`, and `\subfile{...}` across the project.
pub fn build_project_outline(
    root_path: Option<&Path>,
    fallback_text: &str,
    fallback_name: &str,
    open_buffers: &[(PathBuf, String)],
) -> Vec<OutlineItem> {
    let mut items = Vec::new();
    let mut visited = HashSet::new();

    match root_path {
        Some(path) => {
            let root_dir = path.parent().unwrap_or_else(|| Path::new("."));
            collect_outline_recursive(
                path,
                root_dir,
                open_buffers,
                Some(fallback_text),
                false,
                0,
                &mut visited,
                &mut items,
            );
        }
        None => {
            parse_single_buffer_outline(fallback_text, None, fallback_name, false, &mut items);
        }
    }

    items
}

#[allow(clippy::too_many_arguments)]
fn collect_outline_recursive(
    file_path: &Path,
    root_dir: &Path,
    open_buffers: &[(PathBuf, String)],
    root_fallback_text: Option<&str>,
    from_included: bool,
    depth: usize,
    visited: &mut HashSet<PathBuf>,
    out: &mut Vec<OutlineItem>,
) {
    if depth > 8 {
        return;
    }
    let canon = std::fs::canonicalize(file_path).unwrap_or_else(|_| file_path.to_path_buf());
    if !visited.insert(canon.clone()) {
        return;
    }

    let source_cow: String = if let Some((_, buf)) = open_buffers
        .iter()
        .find(|(p, _)| paths_equivalent(p, file_path))
    {
        buf.clone()
    } else if let Ok(bytes) = std::fs::read(file_path) {
        String::from_utf8_lossy(&bytes).into_owned()
    } else if let Some(fb) = root_fallback_text {
        fb.to_string()
    } else {
        return;
    };

    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("documento.tex")
        .to_string();
    let cur_dir = file_path.parent().unwrap_or(root_dir);

    for (idx, raw_line) in source_cow.lines().enumerate() {
        let line_1based = idx + 1;
        let active = strip_comment(raw_line);
        if active.trim().is_empty() {
            continue;
        }

        // 1. Sectioning commands
        if let Some((kind, title)) = extract_section_heading(active) {
            out.push(OutlineItem {
                kind,
                title,
                file_path: Some(file_path.to_path_buf()),
                file_name: file_name.clone(),
                line_1based,
                from_included_file: from_included,
            });
        }

        // 2. Theorem-like environments
        if let Some(thm_title) = extract_theorem_begin(active) {
            out.push(OutlineItem {
                kind: OutlineKind::Theorem,
                title: thm_title,
                file_path: Some(file_path.to_path_buf()),
                file_name: file_name.clone(),
                line_1based,
                from_included_file: from_included,
            });
        }

        // 3. Labels `\label{...}`
        for lbl in extract_command_args(active, "\\label") {
            if !lbl.is_empty() {
                out.push(OutlineItem {
                    kind: OutlineKind::Label,
                    title: lbl,
                    file_path: Some(file_path.to_path_buf()),
                    file_name: file_name.clone(),
                    line_1based,
                    from_included_file: from_included,
                });
            }
        }

        // 4. Child file inclusions: `\input{...}`, `\include{...}`, `\subfile{...}`
        for cmd in ["\\input", "\\include", "\\subfile"] {
            for target in extract_command_args(active, cmd) {
                if let Some(child_path) = resolve_included_tex(target.trim(), cur_dir, root_dir) {
                    let display_rel = child_path
                        .strip_prefix(root_dir)
                        .unwrap_or(&child_path)
                        .to_string_lossy()
                        .to_string();
                    out.push(OutlineItem {
                        kind: OutlineKind::Include,
                        title: display_rel,
                        file_path: Some(child_path.clone()),
                        file_name: file_name.clone(),
                        line_1based,
                        from_included_file: from_included,
                    });
                    collect_outline_recursive(
                        &child_path,
                        root_dir,
                        open_buffers,
                        None,
                        true,
                        depth + 1,
                        visited,
                        out,
                    );
                } else if !target.trim().is_empty() {
                    out.push(OutlineItem {
                        kind: OutlineKind::Include,
                        title: format!("{} (non trovato)", target.trim()),
                        file_path: Some(file_path.to_path_buf()),
                        file_name: file_name.clone(),
                        line_1based,
                        from_included_file: from_included,
                    });
                }
            }
        }
    }
}

fn parse_single_buffer_outline(
    source: &str,
    file_path: Option<PathBuf>,
    file_name: &str,
    from_included: bool,
    out: &mut Vec<OutlineItem>,
) {
    for (idx, raw_line) in source.lines().enumerate() {
        let line_1based = idx + 1;
        let active = strip_comment(raw_line);
        if active.trim().is_empty() {
            continue;
        }
        if let Some((kind, title)) = extract_section_heading(active) {
            out.push(OutlineItem {
                kind,
                title,
                file_path: file_path.clone(),
                file_name: file_name.to_string(),
                line_1based,
                from_included_file: from_included,
            });
        }
        if let Some(thm_title) = extract_theorem_begin(active) {
            out.push(OutlineItem {
                kind: OutlineKind::Theorem,
                title: thm_title,
                file_path: file_path.clone(),
                file_name: file_name.to_string(),
                line_1based,
                from_included_file: from_included,
            });
        }
        for lbl in extract_command_args(active, "\\label") {
            if !lbl.is_empty() {
                out.push(OutlineItem {
                    kind: OutlineKind::Label,
                    title: lbl,
                    file_path: file_path.clone(),
                    file_name: file_name.to_string(),
                    line_1based,
                    from_included_file: from_included,
                });
            }
        }
    }
}

fn paths_equivalent(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

fn resolve_included_tex(raw: &str, cur_dir: &Path, root_dir: &Path) -> Option<PathBuf> {
    if raw.is_empty() {
        return None;
    }
    let p = Path::new(raw);
    let with_ext = if p.extension().is_none() {
        PathBuf::from(format!("{raw}.tex"))
    } else {
        p.to_path_buf()
    };

    for base in [root_dir, cur_dir] {
        let cand = if with_ext.is_absolute() {
            with_ext.clone()
        } else {
            base.join(&with_ext)
        };
        if cand.is_file() {
            return Some(cand);
        }
        let exact = if p.is_absolute() {
            p.to_path_buf()
        } else {
            base.join(p)
        };
        if exact.is_file() {
            return Some(exact);
        }
    }
    None
}

fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == b'%' {
            return &line[..i];
        }
        i += 1;
    }
    line
}

fn extract_balanced_group(s: &str, open: char, close: char) -> Option<(&str, &str)> {
    let rest = s.strip_prefix(open)?;
    let mut depth = 1usize;
    for (idx, ch) in rest.char_indices() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return Some((&rest[..idx], &rest[idx + close.len_utf8()..]));
            }
        }
    }
    None
}

fn extract_section_heading(line: &str) -> Option<(OutlineKind, String)> {
    const CMDS: &[(&str, OutlineKind)] = &[
        ("\\subsubsection", OutlineKind::Subsubsection),
        ("\\subsection", OutlineKind::Subsection),
        ("\\section", OutlineKind::Section),
        ("\\chapter", OutlineKind::Chapter),
        ("\\part", OutlineKind::Part),
    ];

    for &(cmd, kind) in CMDS {
        if let Some(pos) = line.find(cmd) {
            let after = &line[pos + cmd.len()..];
            // Ensure we didn't match a prefix of another command like `\sectionmark`
            if after
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
            {
                continue;
            }
            let after = after.trim_start();
            let after = after.strip_prefix('*').unwrap_or(after).trim_start();
            // Optional short title `[short]`
            let after = if after.starts_with('[') {
                extract_balanced_group(after, '[', ']')
                    .map(|(_, r)| r.trim_start())
                    .unwrap_or(after)
            } else {
                after
            };
            if let Some((title, _)) = extract_balanced_group(after, '{', '}') {
                let clean = clean_latex_title(title);
                if !clean.is_empty() {
                    return Some((kind, clean));
                }
            }
        }
    }
    None
}

fn extract_theorem_begin(line: &str) -> Option<String> {
    let pos = line.find("\\begin{")?;
    let after = &line[pos + "\\begin{".len()..];
    let close = after.find('}')?;
    let env = &after[..close];
    let (_, label_it) = THEOREM_ENVS.iter().find(|&&(e, _)| e == env)?;
    let rest = after[close + 1..].trim_start();
    if let Some((opt_name, _)) = extract_balanced_group(rest, '[', ']') {
        let clean = clean_latex_title(opt_name);
        if !clean.is_empty() {
            return Some(format!("{label_it}: {clean}"));
        }
    }
    Some((*label_it).to_string())
}

fn extract_command_args(line: &str, cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(pos) = rest.find(cmd) {
        let after = &rest[pos + cmd.len()..];
        if after
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        {
            rest = after;
            continue;
        }
        let trimmed = after.trim_start();
        if let Some((arg, rem)) = extract_balanced_group(trimmed, '{', '}') {
            out.push(arg.trim().to_string());
            rest = rem;
        } else {
            break;
        }
    }
    out
}

fn clean_latex_title(raw: &str) -> String {
    let mut s = raw.replace('~', " ");
    for wrapper in ["\\textbf", "\\textit", "\\emph", "\\texttt"] {
        while let Some(pos) = s.find(wrapper) {
            let after = &s[pos + wrapper.len()..];
            if let Some((inner, rem)) = extract_balanced_group(after, '{', '}') {
                s = format!("{}{}{}", &s[..pos], inner, rem);
            } else {
                break;
            }
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ============================================================================
// Search, Replace & Lightweight Regex Engine (Unicode + Capture Groups $1..$9)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub use_regex: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextMatch {
    pub start_byte: usize,
    pub end_byte: usize,
    pub line_0based: usize,
    pub col_byte: usize,
    pub captures: Vec<Option<(usize, usize)>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderSearchHit {
    pub file_path: PathBuf,
    pub file_name: String,
    pub line_1based: usize,
    pub preview: String,
}

#[derive(Debug, Clone)]
enum RegexNode {
    Literal(char),
    Any, // `.` (any non-newline char)
    LineStart,
    LineEnd,
    Class { ranges: Vec<(char, char)>, negated: bool },
    Shorthand(char), // d, D, w, W, s, S
    Seq(Vec<RegexNode>),
    Alt(Box<RegexNode>, Box<RegexNode>),
    Repeat {
        inner: Box<RegexNode>,
        min: usize,
        max: Option<usize>,
        greedy: bool,
    },
    Group {
        index: usize,
        inner: Box<RegexNode>,
    },
}

struct RegexParser<'a> {
    chars: Vec<char>,
    pos: usize,
    next_group: usize,
    _phantom: &'a (),
}

impl<'a> RegexParser<'a> {
    fn new(pattern: &'a str) -> Self {
        Self {
            chars: pattern.chars().collect(),
            pos: 0,
            next_group: 1,
            _phantom: &(),
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += 1;
        Some(ch)
    }

    fn parse_expr(&mut self) -> Result<RegexNode, String> {
        let mut left = self.parse_seq()?;
        while self.peek() == Some('|') {
            self.next();
            let right = self.parse_seq()?;
            left = RegexNode::Alt(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_seq(&mut self) -> Result<RegexNode, String> {
        let mut items = Vec::new();
        while let Some(ch) = self.peek() {
            if ch == ')' || ch == '|' {
                break;
            }
            items.push(self.parse_quantified()?);
        }
        if items.len() == 1 {
            Ok(items.pop().unwrap())
        } else {
            Ok(RegexNode::Seq(items))
        }
    }

    fn parse_quantified(&mut self) -> Result<RegexNode, String> {
        let atom = self.parse_atom()?;
        let (min, max) = match self.peek() {
            Some('*') => {
                self.next();
                (0, None)
            }
            Some('+') => {
                self.next();
                (1, None)
            }
            Some('?') => {
                self.next();
                (0, Some(1))
            }
            _ => return Ok(atom),
        };
        let greedy = if self.peek() == Some('?') {
            self.next();
            false
        } else {
            true
        };
        Ok(RegexNode::Repeat {
            inner: Box::new(atom),
            min,
            max,
            greedy,
        })
    }

    fn parse_atom(&mut self) -> Result<RegexNode, String> {
        let ch = self
            .next()
            .ok_or_else(|| "Espressione regolare incompleta".to_string())?;
        match ch {
            '.' => Ok(RegexNode::Any),
            '^' => Ok(RegexNode::LineStart),
            '$' => Ok(RegexNode::LineEnd),
            '(' => {
                let idx = self.next_group;
                self.next_group += 1;
                let inner = self.parse_expr()?;
                if self.next() != Some(')') {
                    return Err("Parentesi '(' non chiusa nella regex".to_string());
                }
                Ok(RegexNode::Group {
                    index: idx,
                    inner: Box::new(inner),
                })
            }
            '[' => self.parse_class(),
            '\\' => {
                let esc = self
                    .next()
                    .ok_or_else(|| "Backslash finale '\\' non valido nella regex".to_string())?;
                match esc {
                    'd' | 'D' | 'w' | 'W' | 's' | 'S' => Ok(RegexNode::Shorthand(esc)),
                    'n' => Ok(RegexNode::Literal('\n')),
                    't' => Ok(RegexNode::Literal('\t')),
                    'r' => Ok(RegexNode::Literal('\r')),
                    other => Ok(RegexNode::Literal(other)),
                }
            }
            '*' | '+' | '?' => Err(format!("Quantificatore '{ch}' senza espressione precedente")),
            ')' => Err("Parentesi ')' senza apertura".to_string()),
            other => Ok(RegexNode::Literal(other)),
        }
    }

    fn parse_class(&mut self) -> Result<RegexNode, String> {
        let negated = if self.peek() == Some('^') {
            self.next();
            true
        } else {
            false
        };
        let mut ranges = Vec::new();
        let mut closed = false;

        while let Some(ch) = self.next() {
            if ch == ']' && !ranges.is_empty() {
                closed = true;
                break;
            }
            let start_ch = if ch == '\\' {
                match self
                    .next()
                    .ok_or_else(|| "Escape incompleto in [...]".to_string())?
                {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    c => c,
                }
            } else {
                ch
            };

            if self.peek() == Some('-') && self.chars.get(self.pos + 1).copied() != Some(']') {
                self.next(); // consume '-'
                let end_raw = self
                    .next()
                    .ok_or_else(|| "Intervallo incompleto in [...]".to_string())?;
                let end_ch = if end_raw == '\\' {
                    self.next()
                        .ok_or_else(|| "Escape incompleto in [...]".to_string())?
                } else {
                    end_raw
                };
                ranges.push((start_ch.min(end_ch), start_ch.max(end_ch)));
            } else {
                ranges.push((start_ch, start_ch));
            }

            if self.peek() == Some(']') {
                self.next();
                closed = true;
                break;
            }
        }

        if !closed {
            return Err("Parentesi '[' non chiusa nella regex".to_string());
        }
        Ok(RegexNode::Class { ranges, negated })
    }
}

fn compile_regex(pattern: &str) -> Result<(RegexNode, usize), String> {
    let mut parser = RegexParser::new(pattern);
    let node = parser.parse_expr()?;
    if parser.pos < parser.chars.len() {
        return Err("Parentesi ')' non bilanciata nella regex".to_string());
    }
    Ok((node, parser.next_group.saturating_sub(1)))
}

struct MatchCtx<'a> {
    text: &'a str,
    case_sensitive: bool,
    steps: usize,
}

const MAX_REGEX_STEPS_PER_START: usize = 20_000;

fn char_eq(a: char, b: char, case_sensitive: bool) -> bool {
    if case_sensitive || a == b {
        a == b
    } else if a.is_ascii() && b.is_ascii() {
        a.eq_ignore_ascii_case(&b)
    } else {
        a.to_lowercase().eq(b.to_lowercase())
    }
}

fn match_shorthand(kind: char, ch: char) -> bool {
    match kind {
        'd' => ch.is_ascii_digit(),
        'D' => !ch.is_ascii_digit(),
        'w' => ch.is_alphanumeric() || ch == '_',
        'W' => !(ch.is_alphanumeric() || ch == '_'),
        's' => ch.is_whitespace(),
        'S' => !ch.is_whitespace(),
        _ => false,
    }
}

fn eval_node(
    node: &RegexNode,
    ctx: &mut MatchCtx<'_>,
    pos: usize,
    caps: &mut [Option<(usize, usize)>],
    cont: &dyn Fn(&mut MatchCtx<'_>, usize, &mut [Option<(usize, usize)>]) -> Option<usize>,
) -> Option<usize> {
    ctx.steps += 1;
    if ctx.steps > MAX_REGEX_STEPS_PER_START {
        return None;
    }

    match node {
        RegexNode::Literal(expected) => {
            let ch = ctx.text[pos..].chars().next()?;
            if char_eq(ch, *expected, ctx.case_sensitive) {
                cont(ctx, pos + ch.len_utf8(), caps)
            } else {
                None
            }
        }
        RegexNode::Any => {
            let ch = ctx.text[pos..].chars().next()?;
            if ch != '\n' {
                cont(ctx, pos + ch.len_utf8(), caps)
            } else {
                None
            }
        }
        RegexNode::LineStart => {
            if pos == 0 || ctx.text.as_bytes().get(pos.wrapping_sub(1)) == Some(&b'\n') {
                cont(ctx, pos, caps)
            } else {
                None
            }
        }
        RegexNode::LineEnd => {
            if pos == ctx.text.len() || ctx.text.as_bytes().get(pos) == Some(&b'\n') {
                cont(ctx, pos, caps)
            } else {
                None
            }
        }
        RegexNode::Shorthand(k) => {
            let ch = ctx.text[pos..].chars().next()?;
            if match_shorthand(*k, ch) {
                cont(ctx, pos + ch.len_utf8(), caps)
            } else {
                None
            }
        }
        RegexNode::Class { ranges, negated } => {
            let ch = ctx.text[pos..].chars().next()?;
            let in_any = ranges.iter().any(|&(lo, hi)| {
                if ctx.case_sensitive {
                    ch >= lo && ch <= hi
                } else {
                    let cl = ch.to_ascii_lowercase();
                    cl >= lo.to_ascii_lowercase() && cl <= hi.to_ascii_lowercase()
                }
            });
            if in_any != *negated {
                cont(ctx, pos + ch.len_utf8(), caps)
            } else {
                None
            }
        }
        RegexNode::Seq(items) => eval_seq(items, ctx, pos, caps, cont),
        RegexNode::Alt(a, b) => {
            let saved = caps.to_vec();
            if let Some(end) = eval_node(a, ctx, pos, caps, cont) {
                return Some(end);
            }
            caps.clone_from_slice(&saved);
            eval_node(b, ctx, pos, caps, cont)
        }
        RegexNode::Group { index, inner } => {
            let start = pos;
            let idx = *index;
            let prev = caps.get(idx).copied().flatten();
            let res = eval_node(inner, ctx, pos, caps, &|ctx2, after_pos, caps2| {
                if idx < caps2.len() {
                    caps2[idx] = Some((start, after_pos));
                }
                cont(ctx2, after_pos, caps2)
            });
            if res.is_none() && idx < caps.len() {
                caps[idx] = prev;
            }
            res
        }
        RegexNode::Repeat {
            inner,
            min,
            max,
            greedy,
        } => eval_repeat(inner, *min, *max, *greedy, 0, ctx, pos, caps, cont),
    }
}

fn eval_seq(
    items: &[RegexNode],
    ctx: &mut MatchCtx<'_>,
    pos: usize,
    caps: &mut [Option<(usize, usize)>],
    cont: &dyn Fn(&mut MatchCtx<'_>, usize, &mut [Option<(usize, usize)>]) -> Option<usize>,
) -> Option<usize> {
    let Some((first, rest)) = items.split_first() else {
        return cont(ctx, pos, caps);
    };
    eval_node(first, ctx, pos, caps, &|ctx2, next_pos, caps2| {
        eval_seq(rest, ctx2, next_pos, caps2, cont)
    })
}

#[allow(clippy::too_many_arguments)]
fn eval_repeat(
    inner: &RegexNode,
    min: usize,
    max: Option<usize>,
    greedy: bool,
    count: usize,
    ctx: &mut MatchCtx<'_>,
    pos: usize,
    caps: &mut [Option<(usize, usize)>],
    cont: &dyn Fn(&mut MatchCtx<'_>, usize, &mut [Option<(usize, usize)>]) -> Option<usize>,
) -> Option<usize> {
    if let Some(m) = max {
        if count == m {
            return cont(ctx, pos, caps);
        }
    }
    if count < min {
        return eval_node(inner, ctx, pos, caps, &|ctx2, next_pos, caps2| {
            if next_pos == pos && count + 1 >= min {
                cont(ctx2, next_pos, caps2)
            } else {
                eval_repeat(inner, min, max, greedy, count + 1, ctx2, next_pos, caps2, cont)
            }
        });
    }

    let saved = caps.to_vec();
    if greedy {
        let step_more = eval_node(inner, ctx, pos, caps, &|ctx2, next_pos, caps2| {
            if next_pos == pos {
                None
            } else {
                eval_repeat(inner, min, max, greedy, count + 1, ctx2, next_pos, caps2, cont)
            }
        });
        if step_more.is_some() {
            return step_more;
        }
        caps.clone_from_slice(&saved);
        cont(ctx, pos, caps)
    } else {
        if let Some(done) = cont(ctx, pos, caps) {
            return Some(done);
        }
        caps.clone_from_slice(&saved);
        eval_node(inner, ctx, pos, caps, &|ctx2, next_pos, caps2| {
            if next_pos == pos {
                None
            } else {
                eval_repeat(inner, min, max, greedy, count + 1, ctx2, next_pos, caps2, cont)
            }
        })
    }
}

fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (idx, b) in text.bytes().enumerate() {
        if b == b'\n' {
            starts.push(idx + 1);
        }
    }
    starts
}

fn byte_to_line_col(starts: &[usize], byte_pos: usize) -> (usize, usize) {
    let line = match starts.binary_search(&byte_pos) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    };
    (line, byte_pos.saturating_sub(starts[line]))
}

/// Finds all non-overlapping matches of `query` in `text`.
pub fn find_matches(
    text: &str,
    query: &str,
    opts: SearchOptions,
) -> Result<Vec<TextMatch>, String> {
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let starts = line_starts(text);
    let mut matches = Vec::new();

    if opts.use_regex {
        let (ast, group_count) = compile_regex(query)?;
        let mut pos = 0usize;
        while pos <= text.len() {
            let mut caps = vec![None; group_count + 1];
            let mut ctx = MatchCtx {
                text,
                case_sensitive: opts.case_sensitive,
                steps: 0,
            };
            if let Some(end_byte) = eval_node(&ast, &mut ctx, pos, &mut caps, &|_, p, _| Some(p)) {
                if end_byte > pos {
                    caps[0] = Some((pos, end_byte));
                    let (line_0based, col_byte) = byte_to_line_col(&starts, pos);
                    matches.push(TextMatch {
                        start_byte: pos,
                        end_byte,
                        line_0based,
                        col_byte,
                        captures: caps,
                    });
                    pos = end_byte;
                    continue;
                }
            }
            if pos == text.len() {
                break;
            }
            let step = text[pos..].chars().next().map_or(1, char::len_utf8);
            pos += step;
        }
    } else if opts.case_sensitive {
        let mut search_from = 0usize;
        while let Some(rel) = text[search_from..].find(query) {
            let start_byte = search_from + rel;
            let end_byte = start_byte + query.len();
            let (line_0based, col_byte) = byte_to_line_col(&starts, start_byte);
            matches.push(TextMatch {
                start_byte,
                end_byte,
                line_0based,
                col_byte,
                captures: vec![Some((start_byte, end_byte))],
            });
            search_from = end_byte;
        }
    } else {
        // Unicode-safe case-insensitive literal scan
        let q_chars: Vec<char> = query.chars().collect();
        let mut pos = 0usize;
        while pos < text.len() {
            let mut cur = pos;
            let mut matched = true;
            for &qc in &q_chars {
                if let Some(tc) = text[cur..].chars().next() {
                    if char_eq(tc, qc, false) {
                        cur += tc.len_utf8();
                    } else {
                        matched = false;
                        break;
                    }
                } else {
                    matched = false;
                    break;
                }
            }
            if matched && cur > pos {
                let (line_0based, col_byte) = byte_to_line_col(&starts, pos);
                matches.push(TextMatch {
                    start_byte: pos,
                    end_byte: cur,
                    line_0based,
                    col_byte,
                    captures: vec![Some((pos, cur))],
                });
                pos = cur;
            } else {
                pos += text[pos..].chars().next().map_or(1, char::len_utf8);
            }
        }
    }

    Ok(matches)
}

pub fn expand_replacement(
    source: &str,
    m: &TextMatch,
    replacement: &str,
    use_regex: bool,
) -> String {
    if !use_regex || !replacement.contains('$') {
        return replacement.to_string();
    }
    let mut out = String::with_capacity(replacement.len() + 16);
    let mut chars = replacement.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '$' {
            match chars.peek().copied() {
                Some('$') => {
                    chars.next();
                    out.push('$');
                }
                Some(d) if d.is_ascii_digit() => {
                    chars.next();
                    let idx = (d as u8 - b'0') as usize;
                    if let Some(Some((s, e))) = m.captures.get(idx) {
                        out.push_str(&source[*s..*e]);
                    }
                }
                _ => out.push('$'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Replaces all non-overlapping matches of `query` in `text`, returning `(new_text, count)`.
pub fn replace_all_matches(
    text: &str,
    query: &str,
    replacement: &str,
    opts: SearchOptions,
) -> Result<(String, usize), String> {
    let matches = find_matches(text, query, opts)?;
    if matches.is_empty() {
        return Ok((text.to_string(), 0));
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0usize;
    for m in &matches {
        out.push_str(&text[last..m.start_byte]);
        out.push_str(&expand_replacement(text, m, replacement, opts.use_regex));
        last = m.end_byte;
    }
    out.push_str(&text[last..]);
    Ok((out, matches.len()))
}

/// Searches `.tex`, `.bib`, `.sty`, `.cls` files inside `root_dir` (up to depth 4, max 300 line hits).
pub fn search_in_folder(
    root_dir: &Path,
    query: &str,
    opts: SearchOptions,
    open_buffers: &[(PathBuf, String)],
) -> Result<Vec<FolderSearchHit>, String> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    // Validate regex upfront so syntax errors surface immediately.
    if opts.use_regex {
        let _ = compile_regex(query)?;
    }

    let mut files = Vec::new();
    collect_tex_files_recursive(root_dir, 0, &mut files);
    files.sort();

    let mut hits = Vec::new();
    for file_path in files {
        let content: String = if let Some((_, buf)) = open_buffers
            .iter()
            .find(|(p, _)| paths_equivalent(p, &file_path))
        {
            buf.clone()
        } else if let Ok(bytes) = std::fs::read(&file_path) {
            String::from_utf8_lossy(&bytes).into_owned()
        } else {
            continue;
        };

        let matches = find_matches(&content, query, opts)?;
        if matches.is_empty() {
            continue;
        }

        let rel_name = file_path
            .strip_prefix(root_dir)
            .unwrap_or(&file_path)
            .to_string_lossy()
            .to_string();
        let lines: Vec<&str> = content.lines().collect();
        let mut last_line_seen: Option<usize> = None;

        for m in matches {
            if last_line_seen == Some(m.line_0based) {
                continue;
            }
            last_line_seen = Some(m.line_0based);
            let raw_preview = lines.get(m.line_0based).copied().unwrap_or("").trim();
            let preview: String = raw_preview.chars().take(90).collect();
            hits.push(FolderSearchHit {
                file_path: file_path.clone(),
                file_name: rel_name.clone(),
                line_1based: m.line_0based + 1,
                preview,
            });
            if hits.len() >= 300 {
                return Ok(hits);
            }
        }
    }

    Ok(hits)
}

fn collect_tex_files_recursive(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') || name == "target" {
            continue;
        }
        if path.is_dir() {
            collect_tex_files_recursive(&path, depth + 1, out);
        } else if path.is_file() {
            let is_tex_ext = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| {
                    matches!(ext.to_ascii_lowercase().as_str(), "tex" | "bib" | "sty" | "cls")
                });
            if is_tex_ext {
                out.push(path);
            }
        }
    }
}

// ============================================================================
// Scientific Autocomplete (\cite & \ref), BibTeX Parser, Figures, Tables & DOI/arXiv
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionContext {
    Citation { prefix: String },
    Reference { prefix: String },
    Command { prefix: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuggestionItem {
    pub insert_text: String,
    pub display_label: String,
    pub detail: Option<String>,
    pub is_arg_completion: bool,
}

const CITE_COMMANDS: &[&str] = &[
    "cite",
    "citet",
    "citep",
    "citealt",
    "citealp",
    "citeauthor",
    "citeyear",
    "parencite",
    "Parencite",
    "textcite",
    "Textcite",
    "autocite",
    "Autocite",
    "nocite",
    "footcite",
    "Cite",
];

const REF_COMMANDS: &[&str] = &[
    "ref",
    "eqref",
    "cref",
    "Cref",
    "autoref",
    "pageref",
    "nameref",
    "vref",
];

/// Detects whether `before_cursor` (on the current line) is inside `\cite{...}`, `\ref{...}`,
/// or typing a `\command` prefix.
pub fn detect_completion_context(before_cursor: &str) -> Option<CompletionContext> {
    // Check if cursor is inside an unclosed `{...` argument on this line.
    if let Some((cmd_name, arg_tail)) = find_enclosing_brace_command(before_cursor) {
        let key_prefix = arg_tail
            .rsplit(',')
            .next()
            .unwrap_or(arg_tail)
            .trim_start();
        if !key_prefix.contains(char::is_whitespace) {
            if CITE_COMMANDS.contains(&cmd_name) {
                return Some(CompletionContext::Citation {
                    prefix: key_prefix.to_string(),
                });
            }
            if REF_COMMANDS.contains(&cmd_name) {
                return Some(CompletionContext::Reference {
                    prefix: key_prefix.to_string(),
                });
            }
        }
    }

    // Fallback: standard `\command` prefix
    let start = before_cursor.rfind(|c: char| {
        c.is_whitespace() || matches!(c, '{' | '}' | '(' | ')' | '[' | ']' | '$' | ',' | ';')
    });
    let token = match start {
        Some(pos) => {
            let ch_len = before_cursor[pos..].chars().next().map_or(1, char::len_utf8);
            &before_cursor[pos + ch_len..]
        }
        None => before_cursor,
    };
    if token.starts_with('\\') && token.len() >= 2 {
        Some(CompletionContext::Command {
            prefix: token.to_string(),
        })
    } else {
        None
    }
}

fn find_enclosing_brace_command(before_cursor: &str) -> Option<(&str, &str)> {
    // Walk backwards to find the innermost unclosed `{`
    let mut depth = 0usize;
    let mut open_brace_idx = None;
    for (idx, ch) in before_cursor.char_indices().rev() {
        match ch {
            '}' => depth += 1,
            '{' => {
                if depth == 0 {
                    open_brace_idx = Some(idx);
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    let brace_pos = open_brace_idx?;
    let arg_tail = &before_cursor[brace_pos + 1..];
    let mut head = before_cursor[..brace_pos].trim_end();

    // Strip up to 2 optional `[...]` arguments before `{` (e.g. `\parencite[see][p.~4]{key`)
    for _ in 0..2 {
        if head.ends_with(']') {
            let mut b_depth = 0usize;
            let mut open_b = None;
            for (i, c) in head.char_indices().rev() {
                if c == ']' {
                    b_depth += 1;
                } else if c == '[' {
                    b_depth -= 1;
                    if b_depth == 0 {
                        open_b = Some(i);
                        break;
                    }
                }
            }
            if let Some(ob) = open_b {
                head = head[..ob].trim_end();
            } else {
                break;
            }
        }
    }

    let head = head.strip_suffix('*').unwrap_or(head);
    let slash_pos = head.rfind('\\')?;
    let cmd = &head[slash_pos + 1..];
    if !cmd.is_empty() && cmd.chars().all(|c| c.is_ascii_alphabetic()) {
        Some((cmd, arg_tail))
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BibEntry {
    pub key: String,
    pub entry_type: String,
    pub author: String,
    pub year: String,
    pub title: String,
}

impl BibEntry {
    pub fn summary(&self) -> String {
        let auth = if self.author.is_empty() {
            "Autore sconosciuto"
        } else {
            self.author.as_str()
        };
        let yr = if self.year.is_empty() {
            "n.d."
        } else {
            self.year.as_str()
        };
        if self.title.is_empty() {
            format!("{auth} ({yr})")
        } else {
            let short_t: String = self.title.chars().take(52).collect();
            let ellipsis = if self.title.chars().count() > 52 {
                "…"
            } else {
                ""
            };
            format!("{auth} ({yr}) — {short_t}{ellipsis}")
        }
    }
}

pub fn parse_bib_content(raw: &str) -> Vec<BibEntry> {
    let mut entries = Vec::new();
    let mut seen_keys = HashSet::new();
    let bytes = raw.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;

    while i < len {
        if bytes[i] == b'%' {
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i] != b'@' {
            i += 1;
            continue;
        }
        i += 1;
        let type_start = i;
        while i < len && bytes[i].is_ascii_alphabetic() {
            i += 1;
        }
        let entry_type = raw[type_start..i].to_ascii_lowercase();
        while i < len && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= len || (bytes[i] != b'{' && bytes[i] != b'(') {
            continue;
        }
        let open_delim = bytes[i];
        let close_delim = if open_delim == b'{' { b'}' } else { b')' };
        let body_start = i + 1;
        let mut depth = 1usize;
        i += 1;
        let mut in_quotes = false;
        while i < len && depth > 0 {
            if bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            if bytes[i] == b'"' && depth == 1 {
                in_quotes = !in_quotes;
            } else if !in_quotes {
                if bytes[i] == open_delim || bytes[i] == b'{' {
                    depth += 1;
                } else if bytes[i] == close_delim || bytes[i] == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            i += 1;
        }
        let body_end = i.min(len);
        if i < len {
            i += 1;
        }

        if entry_type.is_empty()
            || matches!(entry_type.as_str(), "comment" | "preamble" | "string")
        {
            continue;
        }

        let body = &raw[body_start..body_end];
        let Some(comma_pos) = body.find(',') else {
            continue;
        };
        let key = body[..comma_pos].trim().to_string();
        if key.is_empty() || !seen_keys.insert(key.clone()) {
            continue;
        }

        let fields_str = &body[comma_pos + 1..];
        let fields = parse_bib_fields(fields_str);

        let raw_author = fields
            .iter()
            .find(|(k, _)| k == "author" || k == "editor")
            .map(|(_, v)| v.as_str())
            .unwrap_or("");
        let raw_year = fields
            .iter()
            .find(|(k, _)| k == "year" || k == "date")
            .map(|(_, v)| extract_4digit_year(v))
            .unwrap_or_default();
        let raw_title = fields
            .iter()
            .find(|(k, _)| k == "title")
            .map(|(_, v)| clean_bib_value(v))
            .unwrap_or_default();

        entries.push(BibEntry {
            key,
            entry_type,
            author: shorten_bib_authors(&clean_bib_value(raw_author)),
            year: raw_year,
            title: raw_title,
        });
    }

    entries
}

fn parse_bib_fields(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;

    while i < len {
        while i < len && (bytes[i].is_ascii_whitespace() || bytes[i] == b',') {
            i += 1;
        }
        let name_start = i;
        while i < len
            && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'-' | b':'))
        {
            i += 1;
        }
        let name = s[name_start..i].trim().to_ascii_lowercase();
        while i < len && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if name.is_empty() || i >= len || bytes[i] != b'=' {
            while i < len && bytes[i] != b',' {
                i += 1;
            }
            continue;
        }
        i += 1; // skip '='
        while i < len && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= len {
            break;
        }

        let val = if bytes[i] == b'{' {
            let start = i + 1;
            let mut depth = 1usize;
            i += 1;
            while i < len && depth > 0 {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'{' {
                    depth += 1;
                } else if bytes[i] == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                i += 1;
            }
            let end = i.min(len);
            if i < len {
                i += 1;
            }
            s[start..end].to_string()
        } else if bytes[i] == b'"' {
            let start = i + 1;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'"' {
                    break;
                }
                i += 1;
            }
            let end = i.min(len);
            if i < len {
                i += 1;
            }
            s[start..end].to_string()
        } else {
            let start = i;
            while i < len && bytes[i] != b',' && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            s[start..i].to_string()
        };

        out.push((name, val));
    }

    out
}

fn clean_bib_value(raw: &str) -> String {
    let cleaned: String = raw
        .replace(['{', '}', '~'], " ")
        .replace("\\&", "&")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    cleaned
}

fn extract_4digit_year(raw: &str) -> String {
    let bytes = raw.as_bytes();
    if bytes.len() >= 4 {
        for w in bytes.windows(4) {
            if w.iter().all(|b| b.is_ascii_digit()) {
                return String::from_utf8_lossy(w).into_owned();
            }
        }
    }
    raw.trim().to_string()
}

fn shorten_bib_authors(cleaned: &str) -> String {
    if cleaned.is_empty() {
        return String::new();
    }
    let parts: Vec<&str> = cleaned
        .split(" and ")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return String::new();
    }
    let surname_of = |full: &str| -> String {
        if let Some((last, _)) = full.split_once(',') {
            last.trim().to_string()
        } else {
            full.split_whitespace().last().unwrap_or(full).to_string()
        }
    };
    match parts.len() {
        1 => surname_of(parts[0]),
        2 => format!("{} & {}", surname_of(parts[0]), surname_of(parts[1])),
        _ => format!("{} et al.", surname_of(parts[0])),
    }
}

/// Collects `.bib` entries across all `.bib` files in the project folder and open `.bib` buffers.
pub fn collect_project_bib_entries(
    root_dir: Option<&Path>,
    open_buffers: &[(PathBuf, String)],
    active_tex: &str,
) -> Vec<BibEntry> {
    let mut bib_files: Vec<PathBuf> = Vec::new();

    if let Some(dir) = root_dir {
        // 1. Explicit `\bibliography{...}` and `\addbibresource{...}` in active text
        for line in active_tex.lines() {
            let active = strip_comment(line);
            for arg in extract_command_args(active, "\\bibliography")
                .into_iter()
                .chain(extract_command_args(active, "\\addbibresource"))
            {
                for item in arg.split(',') {
                    let trimmed = item.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let with_ext = if trimmed.ends_with(".bib") {
                        PathBuf::from(trimmed)
                    } else {
                        PathBuf::from(format!("{trimmed}.bib"))
                    };
                    let cand = if with_ext.is_absolute() {
                        with_ext
                    } else {
                        dir.join(with_ext)
                    };
                    if cand.is_file() && !bib_files.iter().any(|p| paths_equivalent(p, &cand)) {
                        bib_files.push(cand);
                    }
                }
            }
        }

        // 2. Any `.bib` file inside `root_dir` (up to depth 3)
        let mut all_files = Vec::new();
        collect_tex_files_recursive(dir, 0, &mut all_files);
        for f in all_files {
            if f.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("bib"))
                && !bib_files.iter().any(|p| paths_equivalent(p, &f))
            {
                bib_files.push(f);
            }
        }
    }

    // 3. Also include any open `.bib` tabs
    for (p, _) in open_buffers {
        if p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("bib"))
            && !bib_files.iter().any(|existing| paths_equivalent(existing, p))
        {
            bib_files.push(p.clone());
        }
    }

    let mut entries = Vec::new();
    let mut seen_keys = HashSet::new();

    for bib_path in bib_files {
        let content: String = if let Some((_, buf)) = open_buffers
            .iter()
            .find(|(p, _)| paths_equivalent(p, &bib_path))
        {
            buf.clone()
        } else if let Ok(bytes) = std::fs::read(&bib_path) {
            String::from_utf8_lossy(&bytes).into_owned()
        } else {
            continue;
        };

        for entry in parse_bib_content(&content) {
            if seen_keys.insert(entry.key.clone()) {
                entries.push(entry);
            }
        }
    }

    entries
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelEntry {
    pub key: String,
    pub context: String,
}

/// Scans the multi-file project outline to collect all `\label{...}` keys with section/theorem context.
pub fn collect_project_labels(
    root_path: Option<&Path>,
    fallback_text: &str,
    fallback_name: &str,
    open_buffers: &[(PathBuf, String)],
) -> Vec<LabelEntry> {
    let outline = build_project_outline(root_path, fallback_text, fallback_name, open_buffers);
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut current_heading = String::new();

    for item in outline {
        match item.kind {
            OutlineKind::Part
            | OutlineKind::Chapter
            | OutlineKind::Section
            | OutlineKind::Subsection
            | OutlineKind::Subsubsection
            | OutlineKind::Theorem => {
                current_heading = item.title;
            }
            OutlineKind::Label => {
                if seen.insert(item.key_or_title().to_string()) {
                    let loc = if item.from_included_file {
                        format!("{}:{}", item.file_name, item.line_1based)
                    } else {
                        format!("riga {}", item.line_1based)
                    };
                    let ctx = if current_heading.is_empty() {
                        loc
                    } else {
                        format!("{current_heading} · {loc}")
                    };
                    out.push(LabelEntry {
                        key: item.title,
                        context: ctx,
                    });
                }
            }
            OutlineKind::Include => {}
        }
    }

    out
}

impl OutlineItem {
    fn key_or_title(&self) -> &str {
        &self.title
    }
}

pub fn suggest_citations(entries: &[BibEntry], prefix: &str) -> Vec<SuggestionItem> {
    let lower = prefix.to_lowercase();
    let mut out = Vec::new();
    for e in entries {
        let matches = lower.is_empty()
            || e.key.to_lowercase().contains(&lower)
            || e.author.to_lowercase().contains(&lower)
            || e.title.to_lowercase().contains(&lower)
            || e.year.contains(&lower);
        if matches {
            out.push(SuggestionItem {
                insert_text: e.key.clone(),
                display_label: e.key.clone(),
                detail: Some(e.summary()),
                is_arg_completion: true,
            });
            if out.len() >= 10 {
                break;
            }
        }
    }
    out
}

pub fn suggest_labels(labels: &[LabelEntry], prefix: &str) -> Vec<SuggestionItem> {
    let lower = prefix.to_lowercase();
    let mut out = Vec::new();
    for l in labels {
        if lower.is_empty()
            || l.key.to_lowercase().starts_with(&lower)
            || l.key.to_lowercase().contains(&lower)
        {
            out.push(SuggestionItem {
                insert_text: l.key.clone(),
                display_label: l.key.clone(),
                detail: Some(l.context.clone()),
                is_arg_completion: true,
            });
            if out.len() >= 10 {
                break;
            }
        }
    }
    out
}

// ============================================================================
// Figure Asset Helper, Table Generator & DOI / arXiv BibTeX Importer
// ============================================================================

pub fn is_supported_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "pdf" | "eps" | "svg"
            )
        })
}

/// Prepares a `\begin{figure}...\end{figure}` block for `image_path`.
/// If `active_tex_path` is saved and `image_path` lies outside the project folder, copies the
/// image into `<project_dir>/figures/<filename>` so the project stays self-contained!
/// Returns `(snippet, required_package)`.
pub fn prepare_figure_snippet(
    image_path: &Path,
    active_tex_path: Option<&Path>,
    width_ratio: f32,
) -> Result<(String, &'static str), String> {
    if !image_path.is_file() {
        return Err("File immagine non trovato".to_string());
    }
    let ext = image_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let stem = image_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("figura");

    let rel_tex_path = if let Some(tex_p) = active_tex_path {
        if let Some(proj_dir) = tex_p.parent() {
            let canon_proj =
                std::fs::canonicalize(proj_dir).unwrap_or_else(|_| proj_dir.to_path_buf());
            let canon_img =
                std::fs::canonicalize(image_path).unwrap_or_else(|_| image_path.to_path_buf());
            if let Ok(rel) = canon_img.strip_prefix(&canon_proj) {
                rel.to_string_lossy().replace('\\', "/")
            } else {
                let figs_dir = proj_dir.join("figures");
                std::fs::create_dir_all(&figs_dir)
                    .map_err(|e| format!("Impossibile creare cartella figures/: {e}"))?;
                let file_name = image_path
                    .file_name()
                    .ok_or_else(|| "Nome immagine non valido".to_string())?;
                let initial_dest = figs_dir.join(file_name);
                let final_dest =
                    if !initial_dest.exists() || paths_equivalent(image_path, &initial_dest) {
                        initial_dest
                    } else {
                        let mut chosen = None;
                        for idx in 1..=9999u32 {
                            let candidate_name = if ext.is_empty() {
                                format!("{stem}_{idx}")
                            } else {
                                format!("{stem}_{idx}.{ext}")
                            };
                            let candidate = figs_dir.join(&candidate_name);
                            if !candidate.exists() {
                                chosen = Some(candidate);
                                break;
                            }
                        }
                        chosen.ok_or_else(|| {
                            "Impossibile trovare un nome univoco nella cartella figures/".to_string()
                        })?
                    };
                if !final_dest.exists() || !paths_equivalent(image_path, &final_dest) {
                    std::fs::copy(image_path, &final_dest)
                        .map_err(|e| format!("Errore copia immagine in figures/: {e}"))?;
                }
                let final_name = final_dest
                    .file_name()
                    .unwrap_or(file_name)
                    .to_string_lossy();
                format!("figures/{final_name}")
            }
        } else {
            image_path.to_string_lossy().replace('\\', "/")
        }
    } else {
        image_path.to_string_lossy().replace('\\', "/")
    };

    let slug: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    let safe_slug = if slug.is_empty() {
        "immagine".to_string()
    } else {
        slug
    };
    let caption_human = stem.replace(['_', '-'], " ");
    let w = width_ratio.clamp(0.2, 1.0);

    let (include_cmd, pkg) = if ext == "svg" {
        (
            format!("\\includesvg[width={w:.1}\\textwidth]{{{rel_tex_path}}}"),
            "svg",
        )
    } else {
        (
            format!("\\includegraphics[width={w:.1}\\textwidth]{{{rel_tex_path}}}"),
            "graphicx",
        )
    };

    let snippet = format!(
        "\\begin{{figure}}[htbp]\n  \\centering\n  {include_cmd}\n  \\caption{{{caption_human}}}\n  \\label{{fig:{safe_slug}}}\n\\end{{figure}}\n"
    );
    Ok((snippet, pkg))
}

pub async fn pick_image_file() -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title("Inserisci figura (.pdf, .png, .jpg, .svg)")
        .add_filter("Immagini e Grafici", &["pdf", "png", "jpg", "jpeg", "eps", "svg"])
        .pick_file()
        .await
        .map(|h| h.path().to_path_buf())
}

/// Generates a clean LaTeX `tabular` (either `booktabs` publication-grade or classic `\hline`).
pub fn generate_latex_table(
    rows: usize,
    cols: usize,
    align: char,
    use_booktabs: bool,
    with_header: bool,
    wrap_table_env: bool,
) -> String {
    let r = rows.clamp(1, 20);
    let c = cols.clamp(1, 12);
    let al = if matches!(align, 'l' | 'c' | 'r') {
        align
    } else {
        'c'
    };

    let col_spec = if use_booktabs {
        std::iter::repeat_n(al, c).collect::<String>()
    } else {
        let inner: Vec<String> = (0..c).map(|_| al.to_string()).collect();
        format!("|{}|", inner.join("|"))
    };

    let indent = if wrap_table_env { "  " } else { "" };
    let mut out = String::new();
    if wrap_table_env {
        out.push_str("\\begin{table}[htbp]\n  \\centering\n");
    }
    out.push_str(&format!("{indent}\\begin{{tabular}}{{{col_spec}}}\n"));
    if use_booktabs {
        out.push_str(&format!("{indent}  \\toprule\n"));
    } else {
        out.push_str(&format!("{indent}  \\hline\n"));
    }

    if with_header {
        let headers: Vec<String> = (1..=c)
            .map(|j| format!("\\textbf{{Col {j}}}"))
            .collect();
        out.push_str(&format!("{indent}  {} \\\\\n", headers.join(" & ")));
        if use_booktabs {
            out.push_str(&format!("{indent}  \\midrule\n"));
        } else {
            out.push_str(&format!("{indent}  \\hline\n"));
        }
    }

    for i in 1..=r {
        let cells: Vec<String> = (1..=c).map(|j| format!("$a_{{{i}{j}}}$")).collect();
        out.push_str(&format!("{indent}  {} \\\\\n", cells.join(" & ")));
        if !use_booktabs {
            out.push_str(&format!("{indent}  \\hline\n"));
        }
    }

    if use_booktabs {
        out.push_str(&format!("{indent}  \\bottomrule\n"));
    }
    out.push_str(&format!("{indent}\\end{{tabular}}\n"));

    if wrap_table_env {
        out.push_str("  \\caption{Didascalia della tabella}\n  \\label{tab:dati}\n\\end{table}\n");
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BibIdentifier {
    Doi(String),
    Arxiv(String),
}

pub fn classify_bib_identifier(raw: &str) -> Option<BibIdentifier> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        return None;
    }

    // Strip URL prefixes if pasted directly from browser
    let lower = trimmed.to_ascii_lowercase();
    if let Some(rest) = lower
        .strip_prefix("https://doi.org/")
        .or_else(|| lower.strip_prefix("http://doi.org/"))
        .or_else(|| lower.strip_prefix("https://dx.doi.org/"))
        .or_else(|| lower.strip_prefix("doi:"))
    {
        let orig_offset = trimmed.len() - rest.len();
        let doi = trimmed[orig_offset..].trim();
        if doi.starts_with("10.") && doi.contains('/') {
            return Some(BibIdentifier::Doi(doi.to_string()));
        }
    }
    if trimmed.starts_with("10.") && trimmed.contains('/') {
        return Some(BibIdentifier::Doi(trimmed.to_string()));
    }

    if let Some(rest) = lower
        .strip_prefix("https://arxiv.org/abs/")
        .or_else(|| lower.strip_prefix("http://arxiv.org/abs/"))
        .or_else(|| lower.strip_prefix("https://arxiv.org/pdf/"))
        .or_else(|| lower.strip_prefix("arxiv:"))
    {
        let orig_offset = trimmed.len() - rest.len();
        let id = trimmed[orig_offset..]
            .trim()
            .trim_end_matches(".pdf")
            .trim();
        if !id.is_empty() {
            return Some(BibIdentifier::Arxiv(id.to_string()));
        }
    }

    // Bare arXiv new-style ID `YYMM.NNNNN` (with optional `vN`) or old-style ` archive/YYMMNNN`
    let base_id = trimmed.split('v').next().unwrap_or(trimmed);
    if let Some((yymm, num)) = base_id.split_once('.') {
        if yymm.len() == 4
            && yymm.bytes().all(|b| b.is_ascii_digit())
            && (4..=6).contains(&num.len())
            && num.bytes().all(|b| b.is_ascii_digit())
        {
            return Some(BibIdentifier::Arxiv(trimmed.to_string()));
        }
    }
    if let Some((cat, num)) = base_id.split_once('/') {
        if !cat.is_empty()
            && cat.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-')
            && num.len() == 7
            && num.bytes().all(|b| b.is_ascii_digit())
        {
            return Some(BibIdentifier::Arxiv(trimmed.to_string()));
        }
    }

    None
}

/// Fetches a BibTeX entry from `doi.org` or `arxiv.org` using `curl`.
pub async fn fetch_bibtex_entry(raw_input: String) -> Result<(String, BibEntry), String> {
    let ident = classify_bib_identifier(&raw_input).ok_or_else(|| {
        "Inserisci un DOI valido (es. 10.1145/3581784) o un arXiv ID (es. 1706.03762)".to_string()
    })?;

    let mut cmd = tokio::process::Command::new("curl");
    cmd.kill_on_drop(true).args([
        "-sSL",
        "--fail",
        "--max-time",
        "12",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
    ]);

    match &ident {
        BibIdentifier::Doi(doi) => {
            cmd.args([
                "-H",
                "Accept: application/x-bibtex; charset=utf-8",
                "--",
                &format!("https://doi.org/{doi}"),
            ]);
        }
        BibIdentifier::Arxiv(arxiv_id) => {
            cmd.args(["--", &format!("https://arxiv.org/bibtex/{arxiv_id}")]);
        }
    }

    let out = cmd.output().await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "curl non è installato nel sistema.".to_string()
        } else {
            format!("Impossibile avviare curl: {e}")
        }
    })?;

    if !out.status.success() {
        return Err(
            "Impossibile scaricare la voce BibTeX (verifica la connessione internet e che il DOI / arXiv ID esista)."
                .to_string(),
        );
    }

    let bib_raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let parsed = parse_bib_content(&bib_raw);
    let Some(entry) = parsed.into_iter().next() else {
        return Err("La risposta ricevuta non contiene una voce BibTeX valida.".to_string());
    };

    Ok((bib_raw, entry))
}

/// Finds the project's primary `.bib` file (or creates `references.bib` in `project_dir`) and
/// appends `bibtex_text` if the key is not already present. Returns `(bib_path, was_added)`.
pub fn append_bibtex_to_project(
    project_dir: &Path,
    active_tex: &str,
    bibtex_text: &str,
    entry_key: &str,
) -> Result<(PathBuf, bool), String> {
    let mut target_bib: Option<PathBuf> = None;

    for line in active_tex.lines() {
        let active = strip_comment(line);
        for arg in extract_command_args(active, "\\addbibresource")
            .into_iter()
            .chain(extract_command_args(active, "\\bibliography"))
        {
            if let Some(first) = arg.split(',').next() {
                let t = first.trim();
                if !t.is_empty() {
                    let p = if t.ends_with(".bib") {
                        PathBuf::from(t)
                    } else {
                        PathBuf::from(format!("{t}.bib"))
                    };
                    let escapes_project = p.is_absolute()
                        || p.components().any(|c| {
                            matches!(
                                c,
                                std::path::Component::ParentDir | std::path::Component::Prefix(_)
                            )
                        });
                    if !escapes_project {
                        target_bib = Some(project_dir.join(p));
                        break;
                    }
                }
            }
        }
        if target_bib.is_some() {
            break;
        }
    }

    if target_bib.is_none() {
        if let Ok(entries) = std::fs::read_dir(project_dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_file()
                    && p.extension()
                        .and_then(|x| x.to_str())
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("bib"))
                {
                    target_bib = Some(p);
                    break;
                }
            }
        }
    }

    let bib_path = target_bib.unwrap_or_else(|| project_dir.join("references.bib"));
    if let Some(parent) = bib_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Impossibile creare cartella bibliografia: {e}"))?;
    }

    let existing = std::fs::read_to_string(&bib_path).unwrap_or_default();
    if parse_bib_content(&existing)
        .iter()
        .any(|e| e.key == entry_key)
    {
        return Ok((bib_path, false));
    }

    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    if !updated.is_empty() {
        updated.push('\n');
    }
    updated.push_str(bibtex_text.trim());
    updated.push('\n');

    crate::document::write_file_atomic_sync(&bib_path, updated.as_bytes())
        .map_err(|e| format!("Errore salvataggio file .bib: {e}"))?;
    Ok((bib_path, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_multifile_outline_following_input_and_include() {
        let dir = std::env::temp_dir().join("feathertex_outline_test");
        let _ = std::fs::remove_dir_all(&dir);
        let ch_dir = dir.join("chapters");
        std::fs::create_dir_all(&ch_dir).unwrap();

        let main_tex = dir.join("main.tex");
        let cap1_tex = ch_dir.join("cap1.tex");

        std::fs::write(
            &main_tex,
            "\\documentclass{article}\n\\begin{document}\n\\section{Introduzione}\\label{sec:intro}\n\\input{chapters/cap1}\n\\end{document}\n",
        )
        .unwrap();
        std::fs::write(
            &cap1_tex,
            "\\subsection{Modello Formale}\n\\begin{theorem}[Completezza]\\label{thm:comp}\n$x=y$\n\\end{theorem}\n",
        )
        .unwrap();

        let items = build_project_outline(Some(&main_tex), "", "main.tex", &[]);
        let titles: Vec<(&str, OutlineKind, bool)> = items
            .iter()
            .map(|i| (i.title.as_str(), i.kind, i.from_included_file))
            .collect();

        assert!(titles.contains(&("Introduzione", OutlineKind::Section, false)));
        assert!(titles.contains(&("sec:intro", OutlineKind::Label, false)));
        assert!(titles.contains(&("chapters/cap1.tex", OutlineKind::Include, false)));
        assert!(titles.contains(&("Modello Formale", OutlineKind::Subsection, true)));
        assert!(titles.contains(&("Teorema: Completezza", OutlineKind::Theorem, true)));
        assert!(titles.contains(&("thm:comp", OutlineKind::Label, true)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn regex_search_and_capture_group_replace_work_on_latex() {
        let src = "Vedi \\cite{knuth1984} e anche \\cite{lamport1994}.";
        let opts = SearchOptions {
            case_sensitive: true,
            use_regex: true,
        };
        let matches = find_matches(src, r"\\cite\{([^}]+)\}", opts).unwrap();
        assert_eq!(matches.len(), 2);

        let (replaced, count) =
            replace_all_matches(src, r"\\cite\{([^}]+)\}", r"\parencite{$1}", opts).unwrap();
        assert_eq!(count, 2);
        assert_eq!(
            replaced,
            "Vedi \\parencite{knuth1984} e anche \\parencite{lamport1994}."
        );
    }

    #[test]
    fn folder_search_scans_subdirectories_and_skips_build_dir() {
        let dir = std::env::temp_dir().join("feathertex_folder_search_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::create_dir_all(dir.join(".build")).unwrap();

        std::fs::write(dir.join("a.tex"), "Primo \\alpha teorema\n").unwrap();
        std::fs::write(dir.join("sub").join("b.tex"), "Secondo \\ALPHA lemma\n").unwrap();
        std::fs::write(dir.join(".build").join("ignored.tex"), "\\alpha\n").unwrap();

        let hits = search_in_folder(
            &dir,
            "\\alpha",
            SearchOptions {
                case_sensitive: false,
                use_regex: false,
            },
            &[],
        )
        .unwrap();
        assert_eq!(hits.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_bibtex_and_detects_cite_and_ref_contexts() {
        let bib = r#"
@article{knuth1984,
  author = {Knuth, Donald E.},
  title = {{Literate Programming}},
  journal = {The Computer Journal},
  year = {1984}
}
@inproceedings{vaswani2017,
  author = {Ashish Vaswani and Noam Shazeer and Niki Parmar},
  title = "Attention Is All You Need",
  date = {2017-06}
}
"#;
        let entries = parse_bib_content(bib);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "knuth1984");
        assert_eq!(entries[0].author, "Knuth");
        assert_eq!(entries[0].year, "1984");
        assert_eq!(entries[0].title, "Literate Programming");
        assert_eq!(entries[1].author, "Vaswani et al.");
        assert_eq!(entries[1].year, "2017");

        assert_eq!(
            detect_completion_context("Vedi \\cite{knuth1984, vas"),
            Some(CompletionContext::Citation {
                prefix: "vas".to_string()
            })
        );
        assert_eq!(
            detect_completion_context("Come in \\parencite[p.~4]{"),
            Some(CompletionContext::Citation {
                prefix: String::new()
            })
        );
        assert_eq!(
            detect_completion_context("Per il Teorema~\\cref{thm:"),
            Some(CompletionContext::Reference {
                prefix: "thm:".to_string()
            })
        );
    }

    #[test]
    fn generates_booktabs_table_and_classifies_doi_arxiv() {
        let tbl = generate_latex_table(2, 3, 'c', true, true, true);
        assert!(tbl.contains("\\begin{table}[htbp]"));
        assert!(tbl.contains("\\begin{tabular}{ccc}"));
        assert!(tbl.contains("\\toprule"));
        assert!(tbl.contains("\\midrule"));
        assert!(tbl.contains("\\bottomrule"));

        assert_eq!(
            classify_bib_identifier("https://doi.org/10.1145/3581784"),
            Some(BibIdentifier::Doi("10.1145/3581784".to_string()))
        );
        assert_eq!(
            classify_bib_identifier("arXiv:1706.03762v5"),
            Some(BibIdentifier::Arxiv("1706.03762v5".to_string()))
        );
        assert_eq!(
            classify_bib_identifier("10.1145/3581784 --upload-file /etc/passwd"),
            None
        );
    }

    #[test]
    fn figure_copy_avoids_overwrite_and_bib_stays_within_project() {
        let dir = std::env::temp_dir().join("feathertex_safety_guards_test");
        let _ = std::fs::remove_dir_all(&dir);
        let proj = dir.join("proj");
        let figs = proj.join("figures");
        let ext_dir = dir.join("external");
        std::fs::create_dir_all(&figs).unwrap();
        std::fs::create_dir_all(&ext_dir).unwrap();

        let tex_path = proj.join("main.tex");
        std::fs::write(&tex_path, "\\documentclass{article}\n").unwrap();

        // Existing figure in figures/plot.png must NOT be overwritten by external plot.png
        let existing_fig = figs.join("plot.png");
        let new_fig = ext_dir.join("plot.png");
        std::fs::write(&existing_fig, b"EXISTING_PNG").unwrap();
        std::fs::write(&new_fig, b"NEW_PNG").unwrap();

        let (snippet, pkg) =
            prepare_figure_snippet(&new_fig, Some(&tex_path), 0.8).expect("figure snippet");
        assert_eq!(pkg, "graphicx");
        assert!(snippet.contains("figures/plot_1.png"), "got: {snippet}");
        assert_eq!(std::fs::read(&existing_fig).unwrap(), b"EXISTING_PNG");
        assert_eq!(std::fs::read(figs.join("plot_1.png")).unwrap(), b"NEW_PNG");

        // Parent-traversal \addbibresource must be ignored and fall back inside project dir
        let (bib_path, added) = append_bibtex_to_project(
            &proj,
            "\\addbibresource{../outside.bib}\n",
            "@article{k2026,\n  title={Safe}\n}\n",
            "k2026",
        )
        .expect("append bib");
        assert!(added);
        assert_eq!(bib_path, proj.join("references.bib"));
        assert!(!dir.join("outside.bib").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}

