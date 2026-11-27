use crate::symbols::{FormulaContext, FormulaItem};
use iced::widget::text_editor;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX_UNDO_HISTORY: usize = 120;
const UNDO_GROUP_WINDOW: Duration = Duration::from_millis(700);

#[derive(Clone)]
struct UndoSnapshot {
    text: String,
    line: usize,
    col: usize,
}

const DEFAULT_TEMPLATE: &str = r#"\documentclass[a4paper,12pt]{article}
\usepackage[utf8]{inputenc}
% \usepackage[italian]{babel} % Decommenta se hai texlive-langitalian
\usepackage{amsmath}
\usepackage{amssymb}
\usepackage{amsthm}
\usepackage{mathtools}
\usepackage{bm}

\newtheorem{theorem}{Teorema}
\newtheorem{lemma}{Lemma}
\newtheorem{proposition}{Proposizione}
\newtheorem{corollary}{Corollario}
\newtheorem{definition}{Definizione}

\title{Nuovo Documento}
\author{}
\date{\today}

\begin{document}
\maketitle

\section{Introduzione}
Inizia a scrivere qui il tuo documento \LaTeX.
Esempio: $\forall x \in \mathbb{N},\; x \ge 0$.

\end{document}
"#;

pub struct Document {
    pub path: Option<PathBuf>,
    pub content: text_editor::Content,
    pub is_dirty: bool,
    pub highlight_revision: u64,
    cached_max_line_cols: usize,
    cached_packages: HashSet<String>,
    pub cached_user_commands: Arc<HashSet<String>>,
    undo_stack: Vec<UndoSnapshot>,
    redo_stack: Vec<UndoSnapshot>,
    last_typing_undo_at: Option<Instant>,
    last_typing_line: Option<usize>,
}

impl Default for Document {
    fn default() -> Self {
        let content = text_editor::Content::with_text(DEFAULT_TEMPLATE);
        let mut doc = Self {
            path: None,
            content,
            is_dirty: false,
            highlight_revision: 0,
            cached_max_line_cols: 0,
            cached_packages: HashSet::new(),
            cached_user_commands: Arc::new(HashSet::new()),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_typing_undo_at: None,
            last_typing_line: None,
        };
        doc.rebuild_caches();
        doc
    }
}

impl Document {
    fn push_undo_snapshot(&mut self, force_new: bool) {
        let (line, col) = self.content.cursor_position();
        let now = Instant::now();
        if !force_new {
            if let (Some(prev_t), Some(prev_line)) = (self.last_typing_undo_at, self.last_typing_line)
            {
                if prev_line == line && now.duration_since(prev_t) < UNDO_GROUP_WINDOW {
                    self.last_typing_undo_at = Some(now);
                    return;
                }
            }
        }

        self.undo_stack.push(UndoSnapshot {
            text: self.content.text(),
            line,
            col,
        });
        if self.undo_stack.len() > MAX_UNDO_HISTORY {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
        self.last_typing_undo_at = if force_new { None } else { Some(now) };
        self.last_typing_line = if force_new { None } else { Some(line) };
    }

    pub fn undo(&mut self) -> bool {
        let Some(snap) = self.undo_stack.pop() else {
            return false;
        };
        let (cur_line, cur_col) = self.content.cursor_position();
        self.redo_stack.push(UndoSnapshot {
            text: self.content.text(),
            line: cur_line,
            col: cur_col,
        });
        self.content = text_editor::Content::with_text(&snap.text);
        self.move_cursor_to(snap.line, snap.col);
        self.is_dirty = true;
        self.last_typing_undo_at = None;
        self.last_typing_line = None;
        self.rebuild_caches();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(snap) = self.redo_stack.pop() else {
            return false;
        };
        let (cur_line, cur_col) = self.content.cursor_position();
        self.undo_stack.push(UndoSnapshot {
            text: self.content.text(),
            line: cur_line,
            col: cur_col,
        });
        self.content = text_editor::Content::with_text(&snap.text);
        self.move_cursor_to(snap.line, snap.col);
        self.is_dirty = true;
        self.last_typing_undo_at = None;
        self.last_typing_line = None;
        self.rebuild_caches();
        true
    }

    pub fn cut_selection(&mut self) -> Option<String> {
        let sel = self.content.selection()?;
        if sel.is_empty() {
            return None;
        }
        self.push_undo_snapshot(true);
        self.content
            .perform(text_editor::Action::Edit(text_editor::Edit::Delete));
        self.is_dirty = true;
        self.rebuild_caches();
        Some(sel)
    }

    fn move_cursor_to(&mut self, line_0based: usize, byte_col: usize) {
        let target_line = line_0based.min(self.line_count().saturating_sub(1));
        self.content
            .perform(text_editor::Action::Move(text_editor::Motion::DocumentStart));
        for _ in 0..target_line {
            self.content
                .perform(text_editor::Action::Move(text_editor::Motion::Down));
        }
        let char_col = self
            .content
            .line(target_line)
            .map(|line| {
                let mut safe = byte_col.min(line.len());
                while safe > 0 && !line.is_char_boundary(safe) {
                    safe -= 1;
                }
                line[..safe].chars().count()
            })
            .unwrap_or(0);
        for _ in 0..char_col {
            self.content
                .perform(text_editor::Action::Move(text_editor::Motion::Right));
        }
    }
    fn rebuild_caches(&mut self) {
        self.highlight_revision = self.highlight_revision.wrapping_add(1);
        let mut max_cols = 0usize;
        let mut packages = HashSet::new();
        let mut user_cmds = HashSet::new();

        for line in self.content.lines() {
            let cols = visual_line_width(&line);
            if cols > max_cols {
                max_cols = cols;
            }
            extract_packages_from_line(&line, &mut packages);
            extract_user_commands_from_line(&line, &mut user_cmds);
        }

        self.cached_max_line_cols = max_cols;
        self.cached_packages = packages;
        self.cached_user_commands = Arc::new(user_cmds);
    }

    /// Fast O(1) cache update for single-line edits; falls back to full rebuild only
    /// when structure changes (Enter, Paste, Backspace/Delete, or `\usepackage` / macro definition lines).
    fn refresh_cache_after_edit(&mut self, prev_line_count: usize, edited_line: usize) {
        let new_line_count = self.content.line_count();
        if new_line_count != prev_line_count {
            self.rebuild_caches();
            return;
        }

        let (cols, touches_meta) = match self.content.line(edited_line) {
            Some(line) => (
                visual_line_width(&line),
                line.contains("\\usepackage")
                    || line.contains("command")
                    || line.contains("\\Declare")
                    || line.contains("\\def"),
            ),
            None => (0, false),
        };
        if cols > self.cached_max_line_cols {
            self.cached_max_line_cols = cols;
        }
        if touches_meta {
            self.rebuild_caches();
        }
    }

    pub fn apply_action(&mut self, action: text_editor::Action, auto_close: bool) {
        let is_edit = matches!(action, text_editor::Action::Edit(_));
        if !is_edit {
            self.content.perform(action);
            return;
        }

        let force_undo_boundary = !matches!(
            action,
            text_editor::Action::Edit(text_editor::Edit::Insert(ch)) if ch != ' ' && ch != '\t'
        );
        self.push_undo_snapshot(force_undo_boundary);

        self.is_dirty = true;
        let prev_lines = self.content.line_count();
        let (edited_line, _) = self.content.cursor_position();
        let is_paste = matches!(action, text_editor::Action::Edit(text_editor::Edit::Paste(_)));

        // Convert literal Tab key presses into 4 spaces consistently across the editor.
        if matches!(action, text_editor::Action::Edit(text_editor::Edit::Insert('\t'))) {
            self.content
                .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                    Arc::new("    ".to_string()),
                )));
            self.highlight_revision = self.highlight_revision.wrapping_add(1);
            self.refresh_cache_after_edit(prev_lines, edited_line);
            return;
        }

        if !auto_close {
            self.content.perform(action);
        } else {
            match action {
                text_editor::Action::Edit(text_editor::Edit::Insert(ch)) => {
                    self.handle_smart_insert(ch);
                }
                text_editor::Action::Edit(text_editor::Edit::Enter) => {
                    self.handle_smart_enter();
                }
                text_editor::Action::Edit(text_editor::Edit::Backspace) => {
                    self.handle_smart_backspace();
                }
                other => {
                    self.content.perform(other);
                }
            }
        }

        if is_paste {
            self.rebuild_caches();
        } else {
            self.refresh_cache_after_edit(prev_lines, edited_line);
        }
    }

    fn handle_smart_insert(&mut self, ch: char) {
        let selection = self.content.selection();

        // If text is selected and user types an opening delimiter, wrap the selection.
        if let Some(sel) = selection {
            if !sel.is_empty() {
                if let Some(close) = matching_closer(ch) {
                    let wrapped = format!("{ch}{sel}{close}");
                    self.content
                        .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                            Arc::new(wrapped),
                        )));
                    return;
                }
            }
        }

        let (line_idx, col) = self.content.cursor_position();
        let (before, after) = self.split_line_at_cursor(line_idx, col);
        let next_char = after.chars().next();
        let prev_char = before.chars().last();
        let escaped = is_preceded_by_unescaped_backslash(&before);

        if ch == '$' {
            if escaped {
                self.content
                    .perform(text_editor::Action::Edit(text_editor::Edit::Insert('$')));
                return;
            }
            if prev_char == Some('$') && next_char == Some('$') {
                self.content
                    .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                        Arc::new("$$".to_string()),
                    )));
                self.content
                    .perform(text_editor::Action::Move(text_editor::Motion::Left));
                return;
            }
            if next_char == Some('$') {
                self.content
                    .perform(text_editor::Action::Move(text_editor::Motion::Right));
                return;
            }
            if should_auto_close_before(next_char) {
                self.content
                    .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                        Arc::new("$$".to_string()),
                    )));
                self.content
                    .perform(text_editor::Action::Move(text_editor::Motion::Left));
                return;
            }
            self.content
                .perform(text_editor::Action::Edit(text_editor::Edit::Insert('$')));
            return;
        }

        // Overtype / skip-over for closing delimiters `}`, `]`, `)`.
        if matches!(ch, '}' | ']' | ')') {
            if next_char == Some(ch) {
                self.content
                    .perform(text_editor::Action::Move(text_editor::Motion::Right));
            } else {
                self.content
                    .perform(text_editor::Action::Edit(text_editor::Edit::Insert(ch)));
            }

            if ch == '}' {
                self.maybe_auto_close_begin_after_brace();
            }
            return;
        }

        // Opening delimiters `{`, `[`, `(`
        if let Some(close) = matching_closer(ch) {
            if escaped {
                if should_auto_close_before(next_char) {
                    let pair = format!("{ch}\\{close}");
                    self.content
                        .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                            Arc::new(pair),
                        )));
                    self.content
                        .perform(text_editor::Action::Move(text_editor::Motion::Left));
                    self.content
                        .perform(text_editor::Action::Move(text_editor::Motion::Left));
                    return;
                }
            } else if should_auto_close_before(next_char) {
                let pair = format!("{ch}{close}");
                self.content
                    .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                        Arc::new(pair),
                    )));
                self.content
                    .perform(text_editor::Action::Move(text_editor::Motion::Left));
                return;
            }
        }

        self.content
            .perform(text_editor::Action::Edit(text_editor::Edit::Insert(ch)));
    }

    fn maybe_auto_close_begin_after_brace(&mut self) {
        let (line_idx, col) = self.content.cursor_position();
        let (before, after) = self.split_line_at_cursor(line_idx, col);

        if !after.trim().is_empty() {
            return;
        }

        let Some(env_name) = extract_begin_env_at_end(&before) else {
            return;
        };

        let env_owned = env_name.to_string();
        let full_text = self.content.text();
        if !has_unclosed_begin(&full_text, &env_owned) {
            return;
        }

        let indent = leading_whitespace(&before);
        let closing = format!("\n{indent}\\end{{{env_owned}}}");
        self.content
            .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                Arc::new(closing),
            )));
        self.content
            .perform(text_editor::Action::Move(text_editor::Motion::Up));
        self.content
            .perform(text_editor::Action::Move(text_editor::Motion::End));
    }

    fn handle_smart_enter(&mut self) {
        if self.content.selection().is_some() {
            self.content
                .perform(text_editor::Action::Edit(text_editor::Edit::Enter));
            return;
        }

        let (line_idx, col) = self.content.cursor_position();
        let (before, after) = self.split_line_at_cursor(line_idx, col);
        let indent = leading_whitespace(&before);

        // Case 1: Pressing Enter inside an empty bracket pair `{|}`, `[|]`, `(|)`.
        if let (Some(prev), Some(next)) = (before.chars().last(), after.chars().next()) {
            if matching_closer(prev) == Some(next) && prev != '$' {
                let snippet = format!("\n{indent}  \n{indent}");
                self.paste_preserving_tabs(&snippet);
                self.content
                    .perform(text_editor::Action::Move(text_editor::Motion::Up));
                self.content
                    .perform(text_editor::Action::Move(text_editor::Motion::End));
                return;
            }
        }

        // Case 2: Pressing Enter after `\begin{env}` (with optional `[...]` / `{...}` arguments).
        if after.trim().is_empty() {
            if let Some(env_name) = extract_begin_env_on_line(&before) {
                let env_owned = env_name.to_string();
                let next_line_has_end = self
                    .content
                    .line(line_idx + 1)
                    .map(|l| l.trim().starts_with(&format!("\\end{{{env_owned}}}")))
                    .unwrap_or(false);

                if next_line_has_end {
                    let snippet = format!("\n{indent}  ");
                    self.content
                        .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                            Arc::new(snippet),
                        )));
                    return;
                }

                let full_text = self.content.text();
                if has_unclosed_begin(&full_text, &env_owned) {
                    let snippet = format!("\n{indent}  \n{indent}\\end{{{env_owned}}}");
                    self.content
                        .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                            Arc::new(snippet),
                        )));
                    self.content
                        .perform(text_editor::Action::Move(text_editor::Motion::Up));
                    self.content
                        .perform(text_editor::Action::Move(text_editor::Motion::End));
                    return;
                }

                let snippet = format!("\n{indent}  ");
                self.content
                    .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                        Arc::new(snippet),
                    )));
                return;
            }
        }

        // Case 3: Preserve current indentation.
        if !indent.is_empty() {
            let snippet = format!("\n{indent}");
            self.paste_preserving_tabs(&snippet);
        } else {
            self.content
                .perform(text_editor::Action::Edit(text_editor::Edit::Enter));
        }
    }

    /// Works around a cosmic-text 0.12 bug where `insert_at` calls `.strip_suffix(char::is_control)`
    /// on the last inserted line, accidentally stripping a trailing `'\t'`.
    fn paste_preserving_tabs(&mut self, text: &str) {
        if text.ends_with('\t') {
            let mut padded = String::with_capacity(text.len() + 1);
            padded.push_str(text);
            padded.push(' ');
            self.content
                .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                    Arc::new(padded),
                )));
            self.content
                .perform(text_editor::Action::Edit(text_editor::Edit::Backspace));
        } else {
            self.content
                .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                    Arc::new(text.to_string()),
                )));
        }
    }

    fn handle_smart_backspace(&mut self) {
        if self.content.selection().is_some() {
            self.content
                .perform(text_editor::Action::Edit(text_editor::Edit::Backspace));
            return;
        }

        let (line_idx, col) = self.content.cursor_position();
        let (before, after) = self.split_line_at_cursor(line_idx, col);

        // Delete empty escaped LaTeX pairs: `\{|\}`, `\[|\]`, `\(|\)`
        if let (Some(open), Some(close)) = (
            before.chars().last(),
            after.strip_prefix('\\').and_then(|s| s.chars().next()),
        ) {
            if matches!(open, '{' | '[' | '(')
                && matching_closer(open) == Some(close)
                && is_preceded_by_unescaped_backslash(&before[..before.len() - open.len_utf8()])
            {
                self.content
                    .perform(text_editor::Action::Edit(text_editor::Edit::Delete));
                self.content
                    .perform(text_editor::Action::Edit(text_editor::Edit::Delete));
                self.content
                    .perform(text_editor::Action::Edit(text_editor::Edit::Backspace));
                self.content
                    .perform(text_editor::Action::Edit(text_editor::Edit::Backspace));
                return;
            }
        }

        // Delete empty regular pairs: `{|}`, `[|]`, `(|)`, `$|$`
        if let (Some(open), Some(close)) = (before.chars().last(), after.chars().next()) {
            if matching_closer(open) == Some(close) {
                self.content
                    .perform(text_editor::Action::Edit(text_editor::Edit::Delete));
                self.content
                    .perform(text_editor::Action::Edit(text_editor::Edit::Backspace));
                return;
            }
        }

        self.content
            .perform(text_editor::Action::Edit(text_editor::Edit::Backspace));
    }

    pub fn insert_snippet(&mut self, snippet: &str) {
        self.push_undo_snapshot(true);
        let prev_lines = self.content.line_count();
        let (edited_line, _) = self.content.cursor_position();
        self.paste_preserving_tabs(snippet);
        if !snippet.contains('\n') {
            if let Some(first_empty) = snippet.find("{}") {
                let chars_after_open = snippet[first_empty + 1..].chars().count();
                for _ in 0..chars_after_open {
                    self.content
                        .perform(text_editor::Action::Move(text_editor::Motion::Left));
                }
            }
        }
        self.is_dirty = true;
        self.highlight_revision = self.highlight_revision.wrapping_add(1);
        if snippet.contains('\n') || snippet.contains("\\usepackage") {
            self.rebuild_caches();
        } else {
            self.refresh_cache_after_edit(prev_lines, edited_line);
        }
    }

    /// Inserts a formula from the palette, auto-importing its required package/preamble
    /// setup if missing and automatically wrapping math snippets in `$...$` or `\[...\]`
    /// when the cursor is currently outside a LaTeX math zone.
    /// Returns the package name if a new `\usepackage{...}` was added to the preamble.
    pub fn insert_formula(&mut self, formula: &FormulaItem) -> Option<&'static str> {
        self.push_undo_snapshot(true);

        let (added_pkg, _) = self.ensure_preamble_entries(
            formula.required_package,
            formula.preamble_setup,
            false,
        );

        let in_math = self.is_cursor_in_math_mode();
        let (line_idx, col) = self.content.cursor_position();
        let (before_cursor, _) = self.split_line_at_cursor(line_idx, col);
        let has_text_before = !before_cursor.trim().is_empty();

        let to_insert = match formula.context {
            FormulaContext::InlineMath => {
                if in_math {
                    formula.insert.to_string()
                } else {
                    format!("${}$", formula.insert)
                }
            }
            FormulaContext::DisplayMath => {
                if in_math {
                    formula.insert.trim().to_string()
                } else {
                    let prefix = if has_text_before { "\n" } else { "" };
                    format!("{prefix}\\[\n{}\n\\]\n", formula.insert)
                }
            }
            FormulaContext::Block => {
                let is_algorithm = formula.insert.starts_with("\\begin{algorithm}");
                let prefix = if is_algorithm {
                    if has_text_before {
                        "\n\n"
                    } else if line_idx > 0
                        && self
                            .content
                            .line(line_idx - 1)
                            .is_some_and(|l| !l.trim().is_empty())
                    {
                        "\n"
                    } else {
                        ""
                    }
                } else if has_text_before {
                    "\n"
                } else {
                    ""
                };
                format!("{prefix}{}", formula.insert)
            }
        };

        let prev_lines = self.content.line_count();
        self.paste_preserving_tabs(&to_insert);
        self.is_dirty = true;
        if to_insert.contains('\n') || added_pkg {
            self.rebuild_caches();
        } else {
            self.refresh_cache_after_edit(prev_lines, line_idx);
        }

        if added_pkg {
            formula.required_package
        } else {
            None
        }
    }

    /// Returns `true` when the cursor is currently inside an unclosed LaTeX math zone
    /// (`$...$`, `$$...$$`, `\(...\)`, `\[...\]`, or standard math environments).
    pub fn is_cursor_in_math_mode(&self) -> bool {
        let (cursor_line, cursor_col) = self.content.cursor_position();
        let mut prefix = String::new();
        for idx in 0..=cursor_line {
            let Some(line) = self.content.line(idx) else {
                break;
            };
            if idx == cursor_line {
                let mut safe = cursor_col.min(line.len());
                while safe > 0 && !line.is_char_boundary(safe) {
                    safe -= 1;
                }
                prefix.push_str(&line[..safe]);
            } else {
                prefix.push_str(&line);
                prefix.push('\n');
            }
        }

        let body_start = prefix
            .find("\\begin{document}")
            .map(|i| i + "\\begin{document}".len())
            .unwrap_or(0);
        is_in_math_mode(&prefix[body_start..])
    }

    pub fn complete_prefix(
        &mut self,
        prefix_char_len: usize,
        completion: &str,
        auto_close: bool,
    ) {
        for _ in 0..prefix_char_len {
            self.content
                .perform(text_editor::Action::Edit(text_editor::Edit::Backspace));
        }

        if auto_close {
            if let Some(env_name) = extract_begin_env_at_end(completion) {
                let (line_idx, col) = self.content.cursor_position();
                let (before, after) = self.split_line_at_cursor(line_idx, col);
                if after.trim().is_empty() {
                    let indent = leading_whitespace(&before);
                    let snippet = format!("{completion}\n{indent}  \n{indent}\\end{{{env_name}}}");
                    self.insert_snippet(&snippet);
                    self.content
                        .perform(text_editor::Action::Move(text_editor::Motion::Up));
                    self.content
                        .perform(text_editor::Action::Move(text_editor::Motion::End));
                    return;
                }
            }
        }

        self.insert_snippet(completion);

        if let Some(first_empty) = completion.find("{}") {
            let chars_after_open = completion[first_empty + 1..].chars().count();
            for _ in 0..chars_after_open {
                self.content
                    .perform(text_editor::Action::Move(text_editor::Motion::Left));
            }
        }
    }

    /// O(1) lookup against the cached preamble packages.
    pub fn has_package(&self, package: &str) -> bool {
        self.cached_packages.contains(package)
    }

    pub fn add_package(&mut self, package: &str) -> bool {
        let default_setup = if package == "pgfplots" {
            Some("\\pgfplotsset{compat=1.18}")
        } else {
            None
        };
        let (added_pkg, _) = self.ensure_preamble_entries(Some(package), default_setup, true);
        added_pkg
    }

    fn ensure_preamble_entries(
        &mut self,
        package: Option<&str>,
        setup_line: Option<&str>,
        record_undo: bool,
    ) -> (bool, bool) {
        let need_pkg = package.is_some_and(|p| !self.has_package(p));
        let current = self.content.text();
        let preamble_end = current.find("\\begin{document}").unwrap_or(current.len());
        let preamble = &current[..preamble_end];

        let need_setup = setup_line.is_some_and(|line| !preamble_has_setup(preamble, line));

        if !need_pkg && !need_setup {
            return (false, false);
        }

        if record_undo {
            self.push_undo_snapshot(true);
        }

        let (orig_line, orig_col) = self.content.cursor_position();
        let mut insert_block = String::new();
        let mut added_lines = 0usize;

        if need_pkg {
            if let Some(pkg) = package {
                insert_block.push_str(&format!("\\usepackage{{{pkg}}}\n"));
                added_lines += 1;
            }
        }
        if need_setup {
            if let Some(line) = setup_line {
                insert_block.push_str(line);
                insert_block.push('\n');
                added_lines += 1;
            }
        }

        let (updated, insert_line_0based) = if let Some(idx) = current.find("\\begin{document}") {
            let line_before = current[..idx].bytes().filter(|&b| b == b'\n').count();
            let mut s = String::with_capacity(current.len() + insert_block.len());
            s.push_str(&current[..idx]);
            s.push_str(&insert_block);
            s.push_str(&current[idx..]);
            (s, line_before)
        } else {
            (format!("{insert_block}{current}"), 0)
        };

        self.content = text_editor::Content::with_text(&updated);
        let new_line = if orig_line >= insert_line_0based {
            orig_line + added_lines
        } else {
            orig_line
        };
        self.move_cursor_to(new_line, orig_col);
        self.is_dirty = true;
        self.rebuild_caches();
        (need_pkg, need_setup)
    }

    pub fn add_packages(&mut self, packages: &[&str]) -> usize {
        let mut added = 0;
        for pkg in packages {
            if self.add_package(pkg) {
                added += 1;
            }
        }
        added
    }

    pub fn completion_context(&self) -> Option<crate::project::CompletionContext> {
        let (line_idx, col) = self.content.cursor_position();
        let (before_cursor, _) = self.split_line_at_cursor(line_idx, col);
        crate::project::detect_completion_context(&before_cursor)
    }

    /// Completes a `\cite{...}` or `\ref{...}` key argument without breaking an existing closing `}`.
    pub fn complete_argument(&mut self, prefix_char_len: usize, key: &str) {
        self.push_undo_snapshot(true);
        for _ in 0..prefix_char_len {
            self.content
                .perform(text_editor::Action::Edit(text_editor::Edit::Backspace));
        }
        self.paste_preserving_tabs(key);
        let (line_idx, col) = self.content.cursor_position();
        let (_, after) = self.split_line_at_cursor(line_idx, col);
        if after.starts_with('}') {
            self.content
                .perform(text_editor::Action::Move(text_editor::Motion::Right));
        }
        self.is_dirty = true;
        self.rebuild_caches();
    }

    pub fn load_text(&mut self, path: PathBuf, text: String) {
        self.path = Some(path);
        self.content = text_editor::Content::with_text(&text);
        self.is_dirty = false;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.last_typing_undo_at = None;
        self.last_typing_line = None;
        self.rebuild_caches();
    }

    /// Moves the editor cursor to the start of the given 1-based line number.
    /// Exact because the editor always uses `Wrapping::None`.
    pub fn jump_to_line(&mut self, line_1based: usize) {
        let target_0based = line_1based
            .saturating_sub(1)
            .min(self.line_count().saturating_sub(1));
        self.content
            .perform(text_editor::Action::Move(text_editor::Motion::DocumentStart));
        for _ in 0..target_0based {
            self.content
                .perform(text_editor::Action::Move(text_editor::Motion::Down));
        }
    }

    /// Computes the exact byte offset of the cursor inside `self.text()`.
    pub fn cursor_byte_offset(&self) -> usize {
        let (line_idx, col_byte) = self.content.cursor_position();
        let text = self.content.text();
        let mut cur_line = 0usize;
        let mut line_start = 0usize;
        for (idx, b) in text.bytes().enumerate() {
            if cur_line == line_idx {
                break;
            }
            if b == b'\n' {
                cur_line += 1;
                line_start = idx + 1;
            }
        }
        let line_end = text[line_start..]
            .find('\n')
            .map(|rel| line_start + rel)
            .unwrap_or(text.len());
        let mut safe_col = col_byte.min(line_end.saturating_sub(line_start));
        while safe_col > 0 && !text.is_char_boundary(line_start + safe_col) {
            safe_col -= 1;
        }
        line_start + safe_col
    }

    /// Returns `(start_line_0based, end_line_0based, had_selection)` for the current selection
    /// (or `(cursor_line, cursor_line, false)` when nothing is selected).
    fn selected_line_span(&self, text: &str) -> (usize, usize, bool) {
        let (cursor_line, cursor_col) = self.content.cursor_position();
        let Some(sel) = self.content.selection().filter(|s| !s.is_empty()) else {
            return (cursor_line, cursor_line, false);
        };

        let cursor_byte = self.cursor_byte_offset();
        let sel_len = sel.len();
        let (start_byte, end_byte) = if cursor_byte >= sel_len
            && text.get(cursor_byte - sel_len..cursor_byte) == Some(sel.as_str())
        {
            (cursor_byte - sel_len, cursor_byte)
        } else if text.get(cursor_byte..cursor_byte + sel_len) == Some(sel.as_str()) {
            (cursor_byte, cursor_byte + sel_len)
        } else if let Some(found) = text.find(sel.as_str()) {
            (found, found + sel_len)
        } else {
            return (cursor_line, cursor_line, false);
        };

        let start_line = text[..start_byte].bytes().filter(|&b| b == b'\n').count();
        let mut end_line = text[..end_byte].bytes().filter(|&b| b == b'\n').count();
        // If selection ends at column 0 of the next line, exclude that trailing line.
        if end_line > start_line && text.as_bytes().get(end_byte.wrapping_sub(1)) == Some(&b'\n') {
            end_line -= 1;
        }
        let _ = cursor_col;
        (start_line, end_line, true)
    }

    fn select_lines_inclusive(&mut self, start_line: usize, end_line: usize) {
        self.move_cursor_to(start_line, 0);
        for _ in start_line..end_line {
            self.content.perform(text_editor::Action::Select(
                text_editor::Motion::Down,
            ));
        }
        self.content
            .perform(text_editor::Action::Select(text_editor::Motion::End));
    }

    /// Toggles `% ` comments on the current line or all selected lines (`Ctrl+/`).
    /// Returns the number of affected lines.
    pub fn toggle_comment(&mut self) -> usize {
        let full = self.content.text();
        let had_trailing_nl = full.ends_with('\n');
        let mut lines: Vec<String> = full.split('\n').map(str::to_string).collect();
        if had_trailing_nl && lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        if lines.is_empty() {
            lines.push(String::new());
        }

        let (orig_line, orig_col) = self.content.cursor_position();
        let (start_l, end_l, had_sel) = self.selected_line_span(&full);
        let end_l = end_l.min(lines.len().saturating_sub(1));
        let start_l = start_l.min(end_l);

        let all_commented = (start_l..=end_l)
            .filter(|&i| !lines[i].trim().is_empty())
            .all(|i| lines[i].trim_start().starts_with('%'));
        let any_non_empty = (start_l..=end_l).any(|i| !lines[i].trim().is_empty());

        self.push_undo_snapshot(true);

        let mut single_col_delta: isize = 0;
        if any_non_empty && all_commented {
            for (idx, line) in lines.iter_mut().enumerate().take(end_l + 1).skip(start_l) {
                let ws_bytes = line.bytes().take_while(|&b| b == b' ' || b == b'\t').count();
                if line[ws_bytes..].starts_with("% ") {
                    line.replace_range(ws_bytes..ws_bytes + 2, "");
                    if idx == orig_line && orig_col > ws_bytes {
                        single_col_delta = -2;
                    }
                } else if line[ws_bytes..].starts_with('%') {
                    line.replace_range(ws_bytes..ws_bytes + 1, "");
                    if idx == orig_line && orig_col > ws_bytes {
                        single_col_delta = -1;
                    }
                }
            }
        } else {
            for (idx, line) in lines.iter_mut().enumerate().take(end_l + 1).skip(start_l) {
                if line.trim().is_empty() && start_l != end_l {
                    continue;
                }
                let ws_bytes = line.bytes().take_while(|&b| b == b' ' || b == b'\t').count();
                line.insert_str(ws_bytes, "% ");
                if idx == orig_line {
                    single_col_delta = 2;
                }
            }
        }

        let mut updated = lines.join("\n");
        if had_trailing_nl {
            updated.push('\n');
        }
        self.content = text_editor::Content::with_text(&updated);
        if had_sel {
            self.select_lines_inclusive(start_l, end_l);
        } else {
            let new_col = (orig_col as isize + single_col_delta).max(0) as usize;
            self.move_cursor_to(orig_line, new_col);
        }
        self.is_dirty = true;
        self.rebuild_caches();
        end_l - start_l + 1
    }

    /// Indents (`unindent == false`) or unindents (`unindent == true`) the selected block or current line.
    pub fn indent_lines(&mut self, unindent: bool) -> usize {
        let full = self.content.text();
        let had_trailing_nl = full.ends_with('\n');
        let mut lines: Vec<String> = full.split('\n').map(str::to_string).collect();
        if had_trailing_nl && lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        if lines.is_empty() {
            lines.push(String::new());
        }

        let (orig_line, orig_col) = self.content.cursor_position();
        let (start_l, end_l, had_sel) = self.selected_line_span(&full);
        let end_l = end_l.min(lines.len().saturating_sub(1));
        let start_l = start_l.min(end_l);

        self.push_undo_snapshot(true);

        let mut single_col_delta: isize = 0;
        let mut changed = 0usize;

        for (idx, line) in lines.iter_mut().enumerate().take(end_l + 1).skip(start_l) {
            if unindent {
                if line.starts_with('\t') {
                    line.remove(0);
                    changed += 1;
                    if idx == orig_line {
                        single_col_delta = -1;
                    }
                } else {
                    let spaces = line.bytes().take(4).take_while(|&b| b == b' ').count();
                    if spaces > 0 {
                        line.replace_range(0..spaces, "");
                        changed += 1;
                        if idx == orig_line {
                            single_col_delta = -(spaces as isize);
                        }
                    }
                }
            } else if !line.is_empty() || start_l == end_l {
                line.insert_str(0, "    ");
                changed += 1;
                if idx == orig_line {
                    single_col_delta = 4;
                }
            }
        }

        if changed == 0 {
            return 0;
        }

        let mut updated = lines.join("\n");
        if had_trailing_nl {
            updated.push('\n');
        }
        self.content = text_editor::Content::with_text(&updated);
        if had_sel {
            self.select_lines_inclusive(start_l, end_l);
        } else {
            let new_col = (orig_col as isize + single_col_delta).max(0) as usize;
            self.move_cursor_to(orig_line, new_col);
        }
        self.is_dirty = true;
        self.rebuild_caches();
        changed
    }

    /// Selects the byte span `[start_byte..end_byte]` in the editor so a search match is highlighted.
    pub fn select_byte_range(&mut self, start_byte: usize, end_byte: usize) -> usize {
        let text = self.content.text();
        let safe_start = start_byte.min(text.len());
        let safe_end = end_byte.min(text.len()).max(safe_start);

        let line_0based = text[..safe_start].bytes().filter(|&b| b == b'\n').count();
        let line_start = text[..safe_start]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        let col_byte = safe_start - line_start;

        self.move_cursor_to(line_0based, col_byte);
        let char_len = text[safe_start..safe_end].chars().count().min(256);
        for _ in 0..char_len {
            self.content
                .perform(text_editor::Action::Select(text_editor::Motion::Right));
        }
        line_0based
    }

    /// Replaces `[start_byte..end_byte]` with `replacement` as an undoable edit.
    pub fn replace_byte_range(
        &mut self,
        start_byte: usize,
        end_byte: usize,
        replacement: &str,
    ) -> usize {
        let mut text = self.content.text();
        let s = start_byte.min(text.len());
        let e = end_byte.min(text.len()).max(s);
        self.push_undo_snapshot(true);
        text.replace_range(s..e, replacement);
        let after_byte = s + replacement.len();
        let line_0based = text[..after_byte].bytes().filter(|&b| b == b'\n').count();
        let line_start = text[..after_byte].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let col_byte = after_byte - line_start;

        self.content = text_editor::Content::with_text(&text);
        self.move_cursor_to(line_0based, col_byte);
        self.is_dirty = true;
        self.rebuild_caches();
        line_0based
    }

    /// Replaces the entire buffer text as an undoable edit, keeping the cursor near its previous line.
    pub fn replace_entire_text(&mut self, new_text: String) {
        let (orig_line, orig_col) = self.content.cursor_position();
        self.push_undo_snapshot(true);
        self.content = text_editor::Content::with_text(&new_text);
        self.move_cursor_to(orig_line, orig_col);
        self.is_dirty = true;
        self.rebuild_caches();
    }

    pub fn text(&self) -> String {
        self.content.text()
    }

    pub fn line_count(&self) -> usize {
        self.content.line_count().max(1)
    }

    /// O(1) cached maximum visual line width (counting tabs as 8-col tab stops).
    pub fn max_line_chars(&self) -> usize {
        self.cached_max_line_cols
    }

    /// Visual column offset of the cursor on its current line (accounting for `\t`).
    pub fn cursor_visual_col(&self) -> usize {
        let (line_idx, col) = self.content.cursor_position();
        let (before, _) = self.split_line_at_cursor(line_idx, col);
        visual_line_width(&before)
    }

    pub fn file_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("Senza titolo.tex")
            .to_string()
    }

    pub fn display_name(&self) -> String {
        let name = self.file_name();
        if self.is_dirty {
            format!("{name}*")
        } else {
            name
        }
    }

    fn split_line_at_cursor(&self, line_idx: usize, col: usize) -> (String, String) {
        let Some(line) = self.content.line(line_idx) else {
            return (String::new(), String::new());
        };
        let mut safe_col = col.min(line.len());
        while safe_col > 0 && !line.is_char_boundary(safe_col) {
            safe_col -= 1;
        }
        (line[..safe_col].to_string(), line[safe_col..].to_string())
    }
}

/// Visual column count using 4-column tab stops.
fn visual_line_width(s: &str) -> usize {
    let mut cols = 0usize;
    for ch in s.chars() {
        if ch == '\t' {
            cols += 4 - (cols % 4);
        } else {
            cols += 1;
        }
    }
    cols
}

fn extract_user_commands_from_line(line: &str, out: &mut HashSet<String>) {
    let active = strip_latex_comment(line);
    for kw in [
        "\\newcommand*",
        "\\newcommand",
        "\\renewcommand*",
        "\\renewcommand",
        "\\providecommand*",
        "\\providecommand",
        "\\DeclareMathOperator*",
        "\\DeclareMathOperator",
        "\\DeclarePairedDelimiter",
    ] {
        let mut rest = active;
        while let Some(idx) = rest.find(kw) {
            let after_kw = rest[idx + kw.len()..].trim_start();
            let candidate = if let Some(braced) = after_kw.strip_prefix('{') {
                braced.split('}').next().unwrap_or("").trim()
            } else {
                after_kw
            };
            if let Some(after_slash) = candidate.strip_prefix('\\') {
                let name_len = after_slash
                    .bytes()
                    .take_while(|b| b.is_ascii_alphabetic())
                    .count();
                if name_len > 0 {
                    out.insert(format!("\\{}", &after_slash[..name_len]));
                }
            }
            rest = &rest[idx + kw.len()..];
        }
    }

    // Also match `\def\mycmd`
    let mut rest = active;
    while let Some(idx) = rest.find("\\def") {
        let after = &rest[idx + 4..];
        let trimmed = after.trim_start();
        if let Some(after_slash) = trimmed.strip_prefix('\\') {
            let name_len = after_slash
                .bytes()
                .take_while(|b| b.is_ascii_alphabetic())
                .count();
            if name_len > 0 {
                out.insert(format!("\\{}", &after_slash[..name_len]));
            }
        }
        rest = after;
    }
}

fn extract_packages_from_line(line: &str, out: &mut HashSet<String>) {
    let active = strip_latex_comment(line);
    let mut rest = active;
    while let Some(idx) = rest.find("\\usepackage") {
        rest = &rest[idx + "\\usepackage".len()..];
        let trimmed = rest.trim_start();
        let after_opts = if let Some(opt_rest) = trimmed.strip_prefix('[') {
            match opt_rest.find(']') {
                Some(close) => opt_rest[close + 1..].trim_start(),
                None => break,
            }
        } else {
            trimmed
        };
        if let Some(brace_rest) = after_opts.strip_prefix('{') {
            if let Some(close) = brace_rest.find('}') {
                for pkg in brace_rest[..close].split(',') {
                    let p = pkg.trim();
                    if !p.is_empty() {
                        out.insert(p.to_string());
                    }
                }
                rest = &brace_rest[close + 1..];
                continue;
            }
        }
        break;
    }
}

fn preamble_has_setup(preamble: &str, setup_line: &str) -> bool {
    let needle = if setup_line.starts_with("\\pgfplotsset{") {
        "\\pgfplotsset{"
    } else if let Some(rest) = setup_line.strip_prefix("\\newtheorem{") {
        if let Some(close) = rest.find('}') {
            &setup_line[.."\\newtheorem{".len() + close + 1]
        } else {
            setup_line
        }
    } else {
        setup_line
    };

    preamble
        .lines()
        .any(|line| strip_latex_comment(line).contains(needle))
}

fn is_math_environment(name: &str) -> bool {
    matches!(
        name,
        "equation"
            | "equation*"
            | "align"
            | "align*"
            | "gather"
            | "gather*"
            | "multline"
            | "multline*"
            | "flalign"
            | "flalign*"
            | "alignat"
            | "alignat*"
            | "eqnarray"
            | "eqnarray*"
            | "math"
            | "displaymath"
    )
}

fn is_in_math_mode(text: &str) -> bool {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;

    let mut in_single_dollar = false;
    let mut in_double_dollar = false;
    let mut in_paren_math = false;
    let mut in_bracket_math = false;
    let mut math_env_depth = 0usize;

    while i < len {
        match bytes[i] {
            b'%' => {
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'\\' => {
                if i + 1 >= len {
                    break;
                }
                match bytes[i + 1] {
                    b'\\' | b'$' | b'%' => {
                        i += 2;
                    }
                    b'(' => {
                        in_paren_math = true;
                        i += 2;
                    }
                    b')' => {
                        in_paren_math = false;
                        i += 2;
                    }
                    b'[' => {
                        in_bracket_math = true;
                        i += 2;
                    }
                    b']' => {
                        in_bracket_math = false;
                        i += 2;
                    }
                    _ => {
                        let rest = &text[i..];
                        if let Some(after_begin) = rest.strip_prefix("\\begin{") {
                            if let Some(close) = after_begin.find('}') {
                                let env = &after_begin[..close];
                                if is_math_environment(env) {
                                    math_env_depth += 1;
                                }
                                i += "\\begin{".len() + close + 1;
                                continue;
                            }
                        } else if let Some(after_end) = rest.strip_prefix("\\end{") {
                            if let Some(close) = after_end.find('}') {
                                let env = &after_end[..close];
                                if is_math_environment(env) {
                                    math_env_depth = math_env_depth.saturating_sub(1);
                                }
                                i += "\\end{".len() + close + 1;
                                continue;
                            }
                        }
                        i += 2;
                    }
                }
            }
            b'$' => {
                if i + 1 < len && bytes[i + 1] == b'$' {
                    if !in_single_dollar {
                        in_double_dollar = !in_double_dollar;
                    }
                    i += 2;
                } else {
                    if !in_double_dollar && !in_bracket_math && math_env_depth == 0 {
                        in_single_dollar = !in_single_dollar;
                    }
                    i += 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }

    in_single_dollar || in_double_dollar || in_paren_math || in_bracket_math || math_env_depth > 0
}

fn matching_closer(open: char) -> Option<char> {
    match open {
        '{' => Some('}'),
        '[' => Some(']'),
        '(' => Some(')'),
        '$' => Some('$'),
        _ => None,
    }
}

fn should_auto_close_before(next_char: Option<char>) -> bool {
    match next_char {
        None => true,
        Some(c) => {
            c.is_whitespace() || matches!(c, '}' | ']' | ')' | '$' | ',' | '.' | ';' | ':' | '\\')
        }
    }
}

fn is_preceded_by_unescaped_backslash(before: &str) -> bool {
    let count = before.chars().rev().take_while(|&c| c == '\\').count();
    count % 2 == 1
}

fn leading_whitespace(line: &str) -> String {
    line.chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

fn is_valid_env_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '*' | '-' | '_'))
}

fn extract_begin_env_at_end(before_cursor: &str) -> Option<&str> {
    let without_close = before_cursor.strip_suffix('}')?;
    let begin_pos = without_close.rfind("\\begin{")?;
    let env_name = &without_close[begin_pos + "\\begin{".len()..];
    if is_valid_env_name(env_name) {
        Some(env_name)
    } else {
        None
    }
}

fn extract_begin_env_on_line(before_cursor: &str) -> Option<&str> {
    let trimmed = before_cursor.trim_end();
    let begin_pos = trimmed.rfind("\\begin{")?;
    let rest = &trimmed[begin_pos + "\\begin{".len()..];
    let close_brace = rest.find('}')?;
    let env_name = &rest[..close_brace];
    if !is_valid_env_name(env_name) {
        return None;
    }

    let trailing = rest[close_brace + 1..].trim();
    if trailing.is_empty() || is_only_bracketed_args(trailing) {
        Some(env_name)
    } else {
        None
    }
}

fn is_only_bracketed_args(s: &str) -> bool {
    let mut depth_brace = 0usize;
    let mut depth_bracket = 0usize;
    let mut saw_arg = false;

    for ch in s.chars() {
        if depth_brace == 0 && depth_bracket == 0 {
            match ch {
                '[' => {
                    depth_bracket += 1;
                    saw_arg = true;
                }
                '{' => {
                    depth_brace += 1;
                    saw_arg = true;
                }
                c if c.is_whitespace() => {}
                _ => return false,
            }
        } else {
            match ch {
                '[' if depth_brace == 0 => depth_bracket += 1,
                ']' if depth_brace == 0 => depth_bracket -= 1,
                '{' => depth_brace += 1,
                '}' => depth_brace -= 1,
                _ => {}
            }
        }
    }

    saw_arg && depth_brace == 0 && depth_bracket == 0
}

fn has_unclosed_begin(full_text: &str, env_name: &str) -> bool {
    let begin_pat = format!("\\begin{{{env_name}}}");
    let end_pat = format!("\\end{{{env_name}}}");

    let mut begins = 0usize;
    let mut ends = 0usize;

    for line in full_text.lines() {
        let active = strip_latex_comment(line);
        begins += active.matches(&begin_pat).count();
        ends += active.matches(&end_pat).count();
    }

    begins > ends
}

fn strip_latex_comment(line: &str) -> &str {
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

pub async fn read_file(path: PathBuf) -> Result<(PathBuf, String), String> {
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Errore lettura file: {e}"))?;
    Ok((path, content))
}

pub async fn pick_and_read_file() -> Result<(PathBuf, String), String> {
    let handle = rfd::AsyncFileDialog::new()
        .add_filter("File LaTeX", &["tex", "bib", "sty", "cls"])
        .pick_file()
        .await
        .ok_or_else(|| "Apertura annullata".to_string())?;

    read_file(handle.path().to_path_buf()).await
}

pub fn write_file_atomic_sync(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("feathertex");
    let tmp_path = path.with_file_name(format!(".{file_name}.feathertex_tmp_{pid}_{seq}"));

    let write_res = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp_path)?;
        f.write_all(contents)?;
        f.flush()?;
        let _ = f.sync_all();
        Ok(())
    })();

    if let Err(e) = write_res {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }

    if let Err(e) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }
    Ok(())
}

pub async fn save_file(path: Option<PathBuf>, contents: String) -> Result<PathBuf, String> {
    let target_path = match path {
        Some(p) => p,
        None => rfd::AsyncFileDialog::new()
            .add_filter("File LaTeX", &["tex"])
            .set_file_name("documento.tex")
            .save_file()
            .await
            .map(|h| h.path().to_path_buf())
            .ok_or_else(|| "Salvataggio annullato".to_string())?,
    };

    let write_path = target_path.clone();
    tokio::task::spawn_blocking(move || write_file_atomic_sync(&write_path, contents.as_bytes()))
        .await
        .map_err(|e| format!("Errore worker salvataggio: {e}"))?
        .map_err(|e| format!("Errore scrittura file: {e}"))?;

    Ok(target_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with(text: &str) -> Document {
        let mut doc = Document {
            path: None,
            content: text_editor::Content::with_text(text),
            is_dirty: false,
            highlight_revision: 0,
            cached_max_line_cols: 0,
            cached_packages: HashSet::new(),
            cached_user_commands: Arc::new(HashSet::new()),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_typing_undo_at: None,
            last_typing_line: None,
        };
        doc.rebuild_caches();
        doc
    }

    fn type_str(doc: &mut Document, s: &str) {
        for ch in s.chars() {
            if ch == '\n' {
                doc.apply_action(
                    text_editor::Action::Edit(text_editor::Edit::Enter),
                    true,
                );
            } else {
                doc.apply_action(
                    text_editor::Action::Edit(text_editor::Edit::Insert(ch)),
                    true,
                );
            }
        }
    }

    #[test]
    fn auto_closes_brackets_and_skips_over() {
        let mut doc = doc_with("");
        type_str(&mut doc, "(x+1)");
        assert_eq!(doc.text().trim_end_matches('\n'), "(x+1)");

        let mut doc2 = doc_with("");
        type_str(&mut doc2, "\\frac{a}{b}");
        assert_eq!(doc2.text().trim_end_matches('\n'), "\\frac{a}{b}");
    }

    #[test]
    fn inserts_tab_character_cleanly() {
        let mut doc = doc_with("");
        doc.apply_action(
            text_editor::Action::Edit(text_editor::Edit::Insert('\t')),
            true,
        );
        type_str(&mut doc, "x\ny");
        assert_eq!(doc.text().trim_end_matches('\n'), "    x\n    y");
        assert_eq!(doc.cursor_visual_col(), 5);
    }

    #[test]
    fn auto_closes_inline_and_display_dollar() {
        let mut doc = doc_with("");
        type_str(&mut doc, "$x$");
        assert_eq!(doc.text().trim_end_matches('\n'), "$x$");

        let mut doc2 = doc_with("");
        type_str(&mut doc2, "$$x");
        assert_eq!(doc2.text().trim_end_matches('\n'), "$$x$$");
    }

    #[test]
    fn auto_closes_escaped_latex_delimiters() {
        let mut doc = doc_with("");
        type_str(&mut doc, "\\[x");
        assert_eq!(doc.text().trim_end_matches('\n'), "\\[x\\]");
    }

    #[test]
    fn smart_backspace_removes_empty_pair() {
        let mut doc = doc_with("");
        type_str(&mut doc, "{");
        assert_eq!(doc.text().trim_end_matches('\n'), "{}");
        doc.apply_action(
            text_editor::Action::Edit(text_editor::Edit::Backspace),
            true,
        );
        assert_eq!(doc.text().trim_end_matches('\n'), "");
    }

    #[test]
    fn auto_closes_begin_environment_on_brace_and_enter() {
        let mut doc = doc_with("");
        type_str(&mut doc, "\\begin{align*}\na &= b");
        assert_eq!(
            doc.text().trim_end_matches('\n'),
            "\\begin{align*}\n  a &= b\n\\end{align*}"
        );
    }

    #[test]
    fn auto_closes_begin_with_optional_argument() {
        let mut doc = doc_with("");
        type_str(&mut doc, "\\begin{theorem}[Pitagora]\na^2 + b^2 = c^2");
        assert_eq!(
            doc.text().trim_end_matches('\n'),
            "\\begin{theorem}[Pitagora]\n  a^2 + b^2 = c^2\n\\end{theorem}"
        );
    }

    #[test]
    fn does_not_duplicate_existing_end_environment() {
        let mut doc = doc_with("\\end{proof}");
        type_str(&mut doc, "\\begin{proof}\n");
        let text = doc.text();
        assert_eq!(text.matches("\\end{proof}").count(), 1);
    }

    #[test]
    fn adds_packages_without_duplicates_and_parses_comma_list() {
        let mut doc = doc_with("\\usepackage{amsmath, amssymb, amsthm}\n\\begin{document}\n\\end{document}\n");
        assert!(doc.has_package("amsmath"));
        assert!(doc.has_package("amssymb"));
        assert!(doc.has_package("amsthm"));
        assert!(!doc.add_package("amsmath"));
        assert!(doc.add_package("physics"));
        assert!(doc.has_package("physics"));
        let added = doc.add_packages(&["physics", "siunitx", "tikz-cd"]);
        assert_eq!(added, 2);
        assert!(doc.has_package("siunitx"));
        assert!(doc.has_package("tikz-cd"));
    }

    #[test]
    fn jumps_to_requested_1based_line() {
        let mut doc = doc_with("line 1\nline 2\nline 3\nline 4\nline 5");
        doc.jump_to_line(4);
        assert_eq!(doc.content.cursor_position(), (3, 0));
        doc.jump_to_line(1);
        assert_eq!(doc.content.cursor_position(), (0, 0));
        doc.jump_to_line(999);
        assert_eq!(doc.content.cursor_position(), (4, 0));
    }

    #[test]
    fn undo_and_redo_restore_text_and_cursor() {
        let mut doc = doc_with("hello");
        doc.insert_snippet(" world");
        assert!(doc.text().starts_with(" worldhello"));
        assert!(doc.undo());
        assert_eq!(doc.text().trim_end_matches('\n'), "hello");
        assert!(doc.redo());
        assert!(doc.text().starts_with(" worldhello"));
    }

    #[test]
    fn toggle_comment_and_block_indent_work_and_undo() {
        let mut doc = doc_with("prima riga\n  seconda riga\nterza riga");
        doc.jump_to_line(2);
        doc.toggle_comment();
        assert_eq!(
            doc.text().trim_end_matches('\n'),
            "prima riga\n  % seconda riga\nterza riga"
        );
        doc.toggle_comment();
        assert_eq!(
            doc.text().trim_end_matches('\n'),
            "prima riga\n  seconda riga\nterza riga"
        );

        // Select lines 1..=2 and block-indent (4 spaces), then block-unindent
        doc.select_lines_inclusive(0, 1);
        doc.indent_lines(false);
        assert_eq!(
            doc.text().trim_end_matches('\n'),
            "    prima riga\n      seconda riga\nterza riga"
        );
        doc.indent_lines(true);
        assert_eq!(
            doc.text().trim_end_matches('\n'),
            "prima riga\n  seconda riga\nterza riga"
        );
    }

    #[test]
    fn select_and_replace_byte_range_update_buffer_cleanly() {
        let mut doc = doc_with("Teorema di \\alpha e \\beta.");
        let pos = doc.text().find("\\alpha").unwrap();
        let line = doc.select_byte_range(pos, pos + "\\alpha".len());
        assert_eq!(line, 0);
        assert_eq!(doc.content.selection().as_deref(), Some("\\alpha"));

        doc.replace_byte_range(pos, pos + "\\alpha".len(), "\\gamma");
        assert_eq!(doc.text().trim_end_matches('\n'), "Teorema di \\gamma e \\beta.");
        assert!(doc.undo());
        assert_eq!(doc.text().trim_end_matches('\n'), "Teorema di \\alpha e \\beta.");
    }
}
