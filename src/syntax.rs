use crate::symbols::{FORMULAS, GREEK_SYMBOLS, LOGIC_SYMBOLS, MATH_SYMBOLS};
use crate::theme;
use iced::advanced::text::highlighter::{self, Highlighter};
use iced::{Color, Font, Theme};
use std::collections::HashSet;
use std::ops::Range;
use std::sync::{Arc, OnceLock};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatexHighlightSettings {
    pub dark_mode: bool,
    pub revision: u64,
    pub misspelled: Arc<HashSet<String>>,
    pub user_commands: Arc<HashSet<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        tokenize_line(line, &self.settings.misspelled, &self.settings.user_commands).into_iter()
    }

    fn current_line(&self) -> usize {
        self.current_line
    }
}

fn known_latex_commands() -> &'static HashSet<&'static str> {
    static KNOWN: OnceLock<HashSet<&'static str>> = OnceLock::new();
    KNOWN.get_or_init(|| {
        let mut set = HashSet::with_capacity(640);

        let mut add_commands_from = |snippet: &'static str| {
            let bytes = snippet.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    let start = i;
                    i += 1;
                    if i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                        while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                            i += 1;
                        }
                        set.insert(&snippet[start..i]);
                        continue;
                    }
                }
                i += 1;
            }
        };

        for sym in LOGIC_SYMBOLS
            .iter()
            .chain(GREEK_SYMBOLS.iter())
            .chain(MATH_SYMBOLS.iter())
        {
            add_commands_from(sym.insert);
        }
        for formula in FORMULAS.iter() {
            add_commands_from(formula.insert);
        }

        const EXTRA: &[&str] = &[
            // Document & sectioning
            "\\documentclass",
            "\\LoadClass",
            "\\usepackage",
            "\\RequirePackage",
            "\\PassOptionsToPackage",
            "\\ProvidesPackage",
            "\\ProvidesClass",
            "\\begin",
            "\\end",
            "\\part",
            "\\chapter",
            "\\section",
            "\\subsection",
            "\\subsubsection",
            "\\paragraph",
            "\\subparagraph",
            "\\title",
            "\\subtitle",
            "\\author",
            "\\date",
            "\\institute",
            "\\thanks",
            "\\and",
            "\\today",
            "\\maketitle",
            "\\titlepage",
            "\\tableofcontents",
            "\\listoffigures",
            "\\listoftables",
            "\\appendix",
            "\\frontmatter",
            "\\mainmatter",
            "\\backmatter",
            "\\input",
            "\\include",
            "\\includeonly",
            "\\subfile",
            // Spacing, line/page breaks & formatting
            "\\newline",
            "\\newpage",
            "\\clearpage",
            "\\cleardoublepage",
            "\\pagebreak",
            "\\nopagebreak",
            "\\linebreak",
            "\\nolinebreak",
            "\\noindent",
            "\\indent",
            "\\par",
            "\\parskip",
            "\\parindent",
            "\\baselineskip",
            "\\hspace",
            "\\vspace",
            "\\hfill",
            "\\vfill",
            "\\hrule",
            "\\vrule",
            "\\rule",
            "\\quad",
            "\\qquad",
            "\\smallskip",
            "\\medskip",
            "\\bigskip",
            "\\centering",
            "\\raggedright",
            "\\raggedleft",
            "\\item",
            "\\label",
            "\\caption",
            "\\footnote",
            "\\footnotetext",
            "\\footnotemark",
            "\\marginpar",
            "\\mbox",
            "\\fbox",
            "\\makebox",
            "\\framebox",
            "\\parbox",
            "\\hbox",
            "\\vbox",
            "\\phantom",
            "\\hphantom",
            "\\vphantom",
            "\\smash",
            // Text styles & sizes
            "\\textbf",
            "\\textit",
            "\\emph",
            "\\texttt",
            "\\textsc",
            "\\textrm",
            "\\textsf",
            "\\textsl",
            "\\textup",
            "\\textmd",
            "\\textnormal",
            "\\underline",
            "\\overline",
            "\\bfseries",
            "\\itshape",
            "\\ttfamily",
            "\\rmfamily",
            "\\sffamily",
            "\\scshape",
            "\\slshape",
            "\\upshape",
            "\\mdseries",
            "\\normalfont",
            "\\tiny",
            "\\scriptsize",
            "\\footnotesize",
            "\\small",
            "\\normalsize",
            "\\large",
            "\\Large",
            "\\LARGE",
            "\\huge",
            "\\Huge",
            "\\LaTeX",
            "\\TeX",
            "\\LaTeXe",
            "\\textbackslash",
            "\\textasciitilde",
            "\\textasciicircum",
            "\\textquotedblleft",
            "\\textquotedblright",
            "\\textquoteleft",
            "\\textquoteright",
            "\\textbraceleft",
            "\\textbraceright",
            "\\textunderscore",
            "\\textbar",
            "\\textless",
            "\\textgreater",
            "\\ldots",
            "\\dots",
            "\\cdots",
            "\\vdots",
            "\\ddots",
            // Macros, theorems, counters & internals
            "\\newcommand",
            "\\renewcommand",
            "\\providecommand",
            "\\newenvironment",
            "\\renewenvironment",
            "\\DeclareMathOperator",
            "\\DeclarePairedDelimiter",
            "\\def",
            "\\edef",
            "\\gdef",
            "\\xdef",
            "\\let",
            "\\newtheorem",
            "\\theoremstyle",
            "\\qed",
            "\\qedhere",
            "\\proofname",
            "\\setlength",
            "\\addtolength",
            "\\setcounter",
            "\\addtocounter",
            "\\stepcounter",
            "\\refstepcounter",
            "\\newcounter",
            "\\numberwithin",
            "\\thesection",
            "\\theequation",
            "\\thefigure",
            "\\thetable",
            "\\makeatletter",
            "\\makeatother",
            "\\AtBeginDocument",
            "\\AtBeginSection",
            "\\ignorespaces",
            "\\relax",
            "\\protect",
            "\\expandafter",
            "\\ifx",
            "\\else",
            "\\fi",
            // References & bibliography
            "\\ref",
            "\\eqref",
            "\\cref",
            "\\Cref",
            "\\crefrange",
            "\\Crefrange",
            "\\pageref",
            "\\autoref",
            "\\nameref",
            "\\cite",
            "\\parencite",
            "\\textcite",
            "\\footcite",
            "\\autocite",
            "\\citet",
            "\\citep",
            "\\citeauthor",
            "\\citeyear",
            "\\nocite",
            "\\bibliography",
            "\\bibliographystyle",
            "\\addbibresource",
            "\\printbibliography",
            // Math core, fonts, delimiters & operators
            "\\text",
            "\\intertext",
            "\\shortintertext",
            "\\tag",
            "\\nonumber",
            "\\notag",
            "\\allowdisplaybreaks",
            "\\displaystyle",
            "\\textstyle",
            "\\scriptstyle",
            "\\scriptscriptstyle",
            "\\frac",
            "\\dfrac",
            "\\tfrac",
            "\\cfrac",
            "\\binom",
            "\\dbinom",
            "\\tbinom",
            "\\sqrt",
            "\\substack",
            "\\stackrel",
            "\\underset",
            "\\overset",
            "\\operatorname",
            "\\limits",
            "\\nolimits",
            "\\left",
            "\\right",
            "\\middle",
            "\\big",
            "\\Big",
            "\\bigg",
            "\\Bigg",
            "\\bigl",
            "\\bigr",
            "\\Bigl",
            "\\Bigr",
            "\\biggl",
            "\\biggr",
            "\\Biggl",
            "\\Biggr",
            "\\mathbb",
            "\\mathcal",
            "\\mathfrak",
            "\\mathscr",
            "\\mathrm",
            "\\mathbf",
            "\\mathit",
            "\\mathsf",
            "\\mathtt",
            "\\mathnormal",
            "\\mathclap",
            "\\boldsymbol",
            "\\bm",
            "\\hat",
            "\\widehat",
            "\\check",
            "\\tilde",
            "\\widetilde",
            "\\acute",
            "\\grave",
            "\\dot",
            "\\ddot",
            "\\dddot",
            "\\breve",
            "\\bar",
            "\\vec",
            "\\overbrace",
            "\\underbrace",
            "\\overrightarrow",
            "\\overleftarrow",
            "\\sum",
            "\\prod",
            "\\coprod",
            "\\int",
            "\\iint",
            "\\iiint",
            "\\iiiint",
            "\\oint",
            "\\bigcup",
            "\\bigcap",
            "\\bigsqcup",
            "\\bigvee",
            "\\bigwedge",
            "\\bigoplus",
            "\\bigotimes",
            "\\lim",
            "\\limsup",
            "\\liminf",
            "\\sup",
            "\\inf",
            "\\max",
            "\\min",
            "\\argmax",
            "\\argmin",
            "\\arg",
            "\\det",
            "\\dim",
            "\\ker",
            "\\deg",
            "\\gcd",
            "\\hom",
            "\\Pr",
            "\\exp",
            "\\ln",
            "\\log",
            "\\lg",
            "\\sin",
            "\\cos",
            "\\tan",
            "\\cot",
            "\\sec",
            "\\csc",
            "\\arcsin",
            "\\arccos",
            "\\arctan",
            "\\sinh",
            "\\cosh",
            "\\tanh",
            "\\coth",
            "\\bmod",
            "\\pmod",
            "\\mod",
            "\\pod",
            "\\not",
            "\\prime",
            // Tables, figures, graphics & colors
            "\\includegraphics",
            "\\includesvg",
            "\\graphicspath",
            "\\textwidth",
            "\\linewidth",
            "\\textheight",
            "\\paperwidth",
            "\\paperheight",
            "\\columnsep",
            "\\toprule",
            "\\midrule",
            "\\bottomrule",
            "\\cmidrule",
            "\\morecmidrules",
            "\\hline",
            "\\cline",
            "\\multicolumn",
            "\\multirow",
            "\\arraystretch",
            "\\tabcolsep",
            "\\color",
            "\\textcolor",
            "\\colorbox",
            "\\fcolorbox",
            "\\definecolor",
            "\\rowcolor",
            "\\cellcolor",
            "\\columncolor",
            // Hyperref, URLs, Beamer, TikZ, SI, Glossaries, Listings, latexdiff
            "\\url",
            "\\href",
            "\\nolinkurl",
            "\\hypertarget",
            "\\hyperlink",
            "\\hypersetup",
            "\\phantomsection",
            "\\frame",
            "\\frametitle",
            "\\framesubtitle",
            "\\pause",
            "\\onslide",
            "\\only",
            "\\uncover",
            "\\visible",
            "\\invisible",
            "\\alert",
            "\\usetheme",
            "\\usecolortheme",
            "\\usefonttheme",
            "\\useinnertheme",
            "\\useoutertheme",
            "\\insertframenumber",
            "\\inserttotalframenumber",
            "\\usetikzlibrary",
            "\\tikz",
            "\\tikzset",
            "\\draw",
            "\\path",
            "\\node",
            "\\coordinate",
            "\\fill",
            "\\filldraw",
            "\\shade",
            "\\shadedraw",
            "\\clip",
            "\\foreach",
            "\\pgfplotsset",
            "\\addplot",
            "\\legend",
            "\\SI",
            "\\si",
            "\\num",
            "\\ang",
            "\\qty",
            "\\unit",
            "\\sisetup",
            "\\gls",
            "\\Gls",
            "\\glspl",
            "\\Glspl",
            "\\newglossaryentry",
            "\\makeglossaries",
            "\\printglossaries",
            "\\newacronym",
            "\\acrshort",
            "\\acrlong",
            "\\acrfull",
            "\\lstset",
            "\\lstinline",
            "\\lstinputlisting",
            "\\mintinline",
            "\\inputminted",
            "\\setminted",
            "\\orcidlink",
            "\\geometry",
            "\\newgeometry",
            "\\restoregeometry",
            "\\captionsetup",
            "\\subcaption",
            "\\DIFadd",
            "\\DIFdel",
            "\\DIFaddbegin",
            "\\DIFaddend",
            "\\DIFdelbegin",
            "\\DIFdelend",
            "\\DIFaddtex",
            "\\DIFdeltex",
        ];

        for cmd in EXTRA {
            set.insert(cmd);
        }

        set
    })
}

fn is_known_latex_command(cmd: &str, user_commands: &HashSet<String>) -> bool {
    // Escaped punctuation/single non-alphabetic command like `\\`, `\,`, `\%`, `\{`, `\}`, `\"`
    if !cmd[1..].chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return true;
    }
    let normalized = cmd.strip_suffix('*').unwrap_or(cmd);
    known_latex_commands().contains(normalized) || user_commands.contains(normalized)
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

fn tokenize_line(
    line: &str,
    misspelled: &HashSet<String>,
    user_commands: &HashSet<String>,
) -> Vec<(Range<usize>, Token)> {
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
                _ if !is_known_latex_command(cmd, user_commands) => Token::Misspelled,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_unknown_latex_command_and_allows_valid_and_user_macros() {
        let misspelled = HashSet::from(["sbagliato".to_string()]);
        let user_cmds = HashSet::from(["\\mycustommacro".to_string()]);

        let toks = tokenize_line(
            "Testo \\newlin \\newline \\mycustommacro \\sigma sbagliato.",
            &misspelled,
            &user_cmds,
        );

        let token_for = |needle: &str, line: &str| {
            let start = line.find(needle).unwrap();
            let end = start + needle.len();
            toks.iter()
                .find(|(r, _)| *r == (start..end))
                .map(|(_, k)| *k)
        };

        let line = "Testo \\newlin \\newline \\mycustommacro \\sigma sbagliato.";
        assert_eq!(token_for("\\newlin", line), Some(Token::Misspelled));
        assert_eq!(token_for("\\newline", line), Some(Token::Command));
        assert_eq!(token_for("\\mycustommacro", line), Some(Token::Command));
        assert_eq!(token_for("\\sigma", line), Some(Token::Command));
        assert_eq!(token_for("sbagliato", line), Some(Token::Misspelled));
    }

    #[test]
    fn demo_tex_files_have_no_false_positive_unknown_commands() {
        let empty = HashSet::new();
        for src in [
            include_str!("../demo/main.tex"),
            include_str!("../demo/chapters/intro.tex"),
            include_str!("../demo/chapters/results.tex"),
        ] {
            for line in src.lines() {
                for (range, tok) in tokenize_line(line, &empty, &empty) {
                    assert_ne!(
                        tok,
                        Token::Misspelled,
                        "unexpected Token::Misspelled for {:?} in line {:?}",
                        &line[range],
                        line
                    );
                }
            }
        }
    }
}
