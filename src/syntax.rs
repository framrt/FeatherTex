use crate::theme;
use iced::advanced::text::highlighter::{self, Highlighter};
use iced::{Color, Font, Theme};
use std::collections::HashSet;
use std::ops::Range;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatexHighlightSettings {
    pub dark_mode: bool,
    pub revision: u64,
    pub misspelled: Arc<HashSet<String>>,
}

#[derive(Debug, Clone, Copy)]
pub enum Token {
    Comment,
    Keyword,
    Environment,
    Command,
    Math,
    MathCommand,
    Bracket,
    Number,
    Misspelled,
}

impl Token {
    pub fn to_format(&self, theme: &Theme) -> highlighter::Format<Font> {
        let is_dark = theme.extended_palette().is_dark;
        let bold_mono = theme::mono_font_bold();

        let (color, font_override) = if is_dark {
            match self {
                Token::Comment => (Color::from_rgb(0.518, 0.494, 0.443), None),
                Token::Keyword => (Color::from_rgb(0.808, 0.569, 0.910), Some(bold_mono)),
                Token::Environment => (Color::from_rgb(0.922, 0.722, 0.341), Some(bold_mono)),
                Token::Command => (Color::from_rgb(0.478, 0.722, 0.957), None),
                Token::Math => (Color::from_rgb(0.451, 0.824, 0.671), None),
                Token::MathCommand => (Color::from_rgb(0.329, 0.855, 0.816), Some(bold_mono)),
                Token::Bracket => (Color::from_rgb(0.922, 0.655, 0.333), Some(bold_mono)),
                Token::Number => (Color::from_rgb(0.929, 0.545, 0.431), None),
                Token::Misspelled => (Color::from_rgb(0.980, 0.360, 0.360), Some(bold_mono)),
            }
        } else {
            match self {
                Token::Comment => (Color::from_rgb(0.467, 0.427, 0.365), None),
                Token::Keyword => (Color::from_rgb(0.443, 0.145, 0.518), Some(bold_mono)),
                Token::Environment => (Color::from_rgb(0.651, 0.369, 0.043), Some(bold_mono)),
                Token::Command => (Color::from_rgb(0.075, 0.294, 0.553), None),
                Token::Math => (Color::from_rgb(0.055, 0.365, 0.278), None),
                Token::MathCommand => (Color::from_rgb(0.027, 0.388, 0.365), Some(bold_mono)),
                Token::Bracket => (Color::from_rgb(0.588, 0.302, 0.039), Some(bold_mono)),
                Token::Number => (Color::from_rgb(0.663, 0.212, 0.090), None),
                Token::Misspelled => (Color::from_rgb(0.840, 0.120, 0.120), Some(bold_mono)),
            }
        };

        highlighter::Format {
            color: Some(color),
            font: font_override,
        }
    }
}

pub struct LatexHighlighter {
    current_line: usize,
    settings: LatexHighlightSettings,
}

impl Highlighter for LatexHighlighter {
    type Settings = LatexHighlightSettings;
    type Highlight = Token;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, Token)>;

    fn new(settings: &Self::Settings) -> Self {
        Self {
            current_line: 0,
            settings: settings.clone(),
        }
    }

    fn update(&mut self, new_settings: &Self::Settings) {
        if self.settings != *new_settings {
            self.settings = new_settings.clone();
            self.current_line = 0;
        }
    }

    fn change_line(&mut self, line: usize) {
        self.current_line = line;
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        self.current_line += 1;
        tokenize_line(line, &self.settings.misspelled).into_iter()
    }

    fn current_line(&self) -> usize {
        self.current_line
    }
}

fn is_non_prose_brace_command(cmd: &str) -> bool {
    matches!(
        cmd,
        "\\begin"
            | "\\end"
            | "\\usepackage"
            | "\\documentclass"
            | "\\label"
            | "\\ref"
            | "\\eqref"
            | "\\cref"
            | "\\Cref"
            | "\\autoref"
            | "\\pageref"
            | "\\cite"
            | "\\parencite"
            | "\\textcite"
            | "\\citet"
            | "\\citep"
            | "\\input"
            | "\\include"
            | "\\subfile"
            | "\\bibliography"
            | "\\bibliographystyle"
            | "\\addbibresource"
            | "\\includegraphics"
            | "\\includesvg"
            | "\\graphicspath"
            | "\\url"
            | "\\href"
            | "\\usetikzlibrary"
            | "\\usetheme"
            | "\\usecolortheme"
    )
}

fn tokenize_line(line: &str, misspelled: &HashSet<String>) -> Vec<(Range<usize>, Token)> {
    let mut tokens = Vec::new();
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut in_math = false;
    let mut expect_env_name = false;
    let mut skip_non_prose_brace = false;

    while i < len {
        let b = bytes[i];

        if b == b'%' {
            tokens.push((i..len, Token::Comment));
            break;
        }

        if b == b'$' {
            let start = i;
            i += 1;
            if i < len && bytes[i] == b'$' {
                i += 1;
            }
            in_math = !in_math;
            expect_env_name = false;
            skip_non_prose_brace = false;
            tokens.push((start..i, Token::Math));
            continue;
        }

        if b == b'\\' {
            let start = i;
            i += 1;
            if i < len {
                let next_ch = line[i..].chars().next().unwrap();
                if next_ch.is_ascii_alphabetic() {
                    while i < len && bytes[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    if i < len && bytes[i] == b'*' {
                        i += 1;
                    }
                } else {
                    i += next_ch.len_utf8();
                }
            }

            let cmd = &line[start..i];
            expect_env_name = matches!(cmd, "\\begin" | "\\end");
            skip_non_prose_brace = is_non_prose_brace_command(cmd);

            let kind = match cmd {
                "\\documentclass"
                | "\\usepackage"
                | "\\begin"
                | "\\end"
                | "\\part"
                | "\\part*"
                | "\\chapter"
                | "\\chapter*"
                | "\\section"
                | "\\section*"
                | "\\subsection"
                | "\\subsection*"
                | "\\subsubsection"
                | "\\subsubsection*"
                | "\\paragraph"
                | "\\title"
                | "\\author"
                | "\\date"
                | "\\maketitle"
                | "\\tableofcontents"
                | "\\newcommand"
                | "\\renewcommand"
                | "\\DeclareMathOperator"
                | "\\DeclareMathOperator*"
                | "\\DeclarePairedDelimiter"
                | "\\newtheorem"
                | "\\newtheorem*"
                | "\\theoremstyle"
                | "\\usetikzlibrary"
                | "\\pgfplotsset"
                | "\\label"
                | "\\ref"
                | "\\eqref"
                | "\\cref"
                | "\\Cref"
                | "\\cite"
                | "\\parencite"
                | "\\citet"
                | "\\citep"
                | "\\input"
                | "\\include"
                | "\\bibliography"
                | "\\bibliographystyle"
                | "\\addbibresource"
                | "\\frametitle" => Token::Keyword,
                "\\[" | "\\]" | "\\(" | "\\)" => {
                    in_math = matches!(cmd, "\\[" | "\\(");
                    Token::Math
                }
                "\\{" | "\\}" => Token::Bracket,
                _ if in_math => Token::MathCommand,
                _ => Token::Command,
            };
            tokens.push((start..i, kind));
            continue;
        }

        if b == b'{' && expect_env_name {
            tokens.push((i..i + 1, Token::Bracket));
            i += 1;
            let env_start = i;
            while i < len && bytes[i] != b'}' && bytes[i] != b'%' {
                let ch = line[i..].chars().next().unwrap();
                i += ch.len_utf8();
            }
            if i > env_start {
                tokens.push((env_start..i, Token::Environment));
            }
            if i < len && bytes[i] == b'}' {
                tokens.push((i..i + 1, Token::Bracket));
                i += 1;
            }
            expect_env_name = false;
            skip_non_prose_brace = false;
            continue;
        }

        expect_env_name = false;

        if b == b'[' && skip_non_prose_brace {
            tokens.push((i..i + 1, Token::Bracket));
            i += 1;
            let mut depth = 1usize;
            while i < len && depth > 0 {
                if bytes[i] == b'[' {
                    depth += 1;
                    tokens.push((i..i + 1, Token::Bracket));
                    i += 1;
                } else if bytes[i] == b']' {
                    depth -= 1;
                    tokens.push((i..i + 1, Token::Bracket));
                    i += 1;
                } else if bytes[i] == b'%' {
                    break;
                } else {
                    let ch = line[i..].chars().next().unwrap();
                    i += ch.len_utf8();
                }
            }
            continue;
        }

        if b == b'{' && skip_non_prose_brace {
            tokens.push((i..i + 1, Token::Bracket));
            i += 1;
            let mut depth = 1usize;
            while i < len && depth > 0 {
                if bytes[i] == b'{' {
                    depth += 1;
                    tokens.push((i..i + 1, Token::Bracket));
                    i += 1;
                } else if bytes[i] == b'}' {
                    depth -= 1;
                    tokens.push((i..i + 1, Token::Bracket));
                    i += 1;
                } else if bytes[i] == b'%' {
                    break;
                } else {
                    let ch = line[i..].chars().next().unwrap();
                    i += ch.len_utf8();
                }
            }
            skip_non_prose_brace = false;
            continue;
        }

        if !b.is_ascii_whitespace() {
            skip_non_prose_brace = false;
        }

        if matches!(b, b'{' | b'}' | b'[' | b']') {
            tokens.push((i..i + 1, Token::Bracket));
            i += 1;
            continue;
        }

        if b.is_ascii_digit() {
            let start = i;
            while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            tokens.push((start..i, Token::Number));
            continue;
        }

        if in_math {
            let start = i;
            while i < len
                && bytes[i] != b'$'
                && bytes[i] != b'\\'
                && bytes[i] != b'%'
                && !matches!(bytes[i], b'{' | b'}' | b'[' | b']')
            {
                let ch = line[i..].chars().next().unwrap();
                i += ch.len_utf8();
            }
            if i > start {
                tokens.push((start..i, Token::Math));
            }
            continue;
        }

        let ch = line[i..].chars().next().unwrap();
        if !misspelled.is_empty() && ch.is_alphabetic() {
            let start = i;
            i += ch.len_utf8();
            while i < len {
                let next_ch = line[i..].chars().next().unwrap();
                if next_ch.is_alphabetic() {
                    i += next_ch.len_utf8();
                } else {
                    break;
                }
            }
            let word = &line[start..i];
            if misspelled.contains(word) || misspelled.contains(&word.to_lowercase()) {
                tokens.push((start..i, Token::Misspelled));
            }
            continue;
        }
        i += ch.len_utf8();
    }

    tokens
}
