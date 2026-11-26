#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteTab {
    Logic,
    Greek,
    Math,
    Formulas,
    Libraries,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryCategory {
    CoreMath,
    AnalysisPhysics,
    LogicCs,
    GraphicsDiagrams,
    Bibliography,
    Typography,
}

impl LibraryCategory {
    pub const ALL: &[LibraryCategory] = &[
        LibraryCategory::CoreMath,
        LibraryCategory::AnalysisPhysics,
        LibraryCategory::LogicCs,
        LibraryCategory::GraphicsDiagrams,
        LibraryCategory::Bibliography,
        LibraryCategory::Typography,
    ];

    pub fn title(self) -> &'static str {
        match self {
            LibraryCategory::CoreMath => "Core, Algebra e Teoremi",
            LibraryCategory::AnalysisPhysics => "Analisi, Calcolo e Fisica",
            LibraryCategory::LogicCs => "Logica e Informatica",
            LibraryCategory::GraphicsDiagrams => "Grafici, Diagrammi e Box",
            LibraryCategory::Bibliography => "Bibliografia e Pubblicazione",
            LibraryCategory::Typography => "Tipografia e Riferimenti",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SymbolItem {
    pub glyph: &'static str,
    pub label: &'static str,
    pub insert: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormulaContext {
    /// Inline math expression (wrapped in `$...$` when inserted outside math mode).
    InlineMath,
    /// Display math body (wrapped in `\[ ... \]` when inserted outside math mode).
    DisplayMath,
    /// Standalone environment/block (e.g. `align*`, `proof`, `theorem`, `tikzpicture`, `algorithm`).
    Block,
}

#[derive(Debug, Clone, Copy)]
pub struct FormulaItem {
    pub glyph: &'static str,
    pub label: &'static str,
    pub insert: &'static str,
    pub context: FormulaContext,
    pub required_package: Option<&'static str>,
    pub preamble_setup: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
pub struct MathLibrary {
    pub package: &'static str,
    pub description: &'static str,
    pub category: LibraryCategory,
    pub texlive_group: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct LibraryPreset {
    pub name: &'static str,
    pub packages: &'static [&'static str],
}

pub const LIBRARY_PRESETS: &[LibraryPreset] = &[
    LibraryPreset {
        name: "Base Math",
        packages: &["amsmath", "amssymb", "amsthm", "mathtools", "bm"],
    },
    LibraryPreset {
        name: "Analisi & Fisica",
        packages: &[
            "amsmath",
            "amssymb",
            "mathtools",
            "physics",
            "siunitx",
            "esint",
            "bm",
        ],
    },
    LibraryPreset {
        name: "Logica & CS",
        packages: &[
            "amsmath",
            "amssymb",
            "amsthm",
            "stmaryrd",
            "ebproof",
            "algorithm2e",
        ],
    },
    LibraryPreset {
        name: "Grafici & Diag.",
        packages: &["tikz", "tikz-cd", "pgfplots", "xcolor", "tcolorbox"],
    },
    LibraryPreset {
        name: "Paper / Ricerca",
        packages: &[
            "amsmath",
            "amssymb",
            "amsthm",
            "booktabs",
            "subcaption",
            "hyperref",
            "cleveref",
            "natbib",
        ],
    },
];

pub const LOGIC_SYMBOLS: &[SymbolItem] = &[
    SymbolItem { glyph: "∀", label: "\\forall", insert: "\\forall " },
    SymbolItem { glyph: "∃", label: "\\exists", insert: "\\exists " },
    SymbolItem { glyph: "∃!", label: "\\exists!", insert: "\\exists! " },
    SymbolItem { glyph: "∄", label: "\\nexists", insert: "\\nexists " },
    SymbolItem { glyph: "¬", label: "\\neg", insert: "\\neg " },
    SymbolItem { glyph: "∧", label: "\\land", insert: "\\land " },
    SymbolItem { glyph: "∨", label: "\\lor", insert: "\\lor " },
    SymbolItem { glyph: "→", label: "\\to", insert: "\\to " },
    SymbolItem { glyph: "↔", label: "\\leftrightarrow", insert: "\\leftrightarrow " },
    SymbolItem { glyph: "⇒", label: "\\Rightarrow", insert: "\\Rightarrow " },
    SymbolItem { glyph: "⇔", label: "\\Leftrightarrow", insert: "\\Leftrightarrow " },
    SymbolItem { glyph: "⟹", label: "\\implies", insert: "\\implies " },
    SymbolItem { glyph: "⟺", label: "\\iff", insert: "\\iff " },
    SymbolItem { glyph: "⇏", label: "\\nRightarrow", insert: "\\nRightarrow " },
    SymbolItem { glyph: "⊢", label: "\\vdash", insert: "\\vdash " },
    SymbolItem { glyph: "⊬", label: "\\nvdash", insert: "\\nvdash " },
    SymbolItem { glyph: "⊨", label: "\\models", insert: "\\models " },
    SymbolItem { glyph: "⊭", label: "\\nvDash", insert: "\\nvDash " },
    SymbolItem { glyph: "⊤", label: "\\top", insert: "\\top " },
    SymbolItem { glyph: "⊥", label: "\\bot", insert: "\\bot " },
    SymbolItem { glyph: "≡", label: "\\equiv", insert: "\\equiv " },
    SymbolItem { glyph: "≜", label: "\\triangleq", insert: "\\triangleq " },
    SymbolItem { glyph: "≔", label: "\\coloneqq", insert: "\\coloneqq " },
    SymbolItem { glyph: "□", label: "\\Box", insert: "\\Box " },
    SymbolItem { glyph: "◇", label: "\\Diamond", insert: "\\Diamond " },
    SymbolItem { glyph: "∴", label: "\\therefore", insert: "\\therefore " },
    SymbolItem { glyph: "∵", label: "\\because", insert: "\\because " },
    SymbolItem { glyph: "⟦⟧", label: "\\llbracket", insert: "\\llbracket  \\rrbracket" },
    SymbolItem { glyph: "λ", label: "\\lambda", insert: "\\lambda " },
];

pub const GREEK_SYMBOLS: &[SymbolItem] = &[
    SymbolItem { glyph: "α", label: "\\alpha", insert: "\\alpha" },
    SymbolItem { glyph: "β", label: "\\beta", insert: "\\beta" },
    SymbolItem { glyph: "γ", label: "\\gamma", insert: "\\gamma" },
    SymbolItem { glyph: "δ", label: "\\delta", insert: "\\delta" },
    SymbolItem { glyph: "ε", label: "\\varepsilon", insert: "\\varepsilon" },
    SymbolItem { glyph: "ζ", label: "\\zeta", insert: "\\zeta" },
    SymbolItem { glyph: "η", label: "\\eta", insert: "\\eta" },
    SymbolItem { glyph: "θ", label: "\\theta", insert: "\\theta" },
    SymbolItem { glyph: "κ", label: "\\kappa", insert: "\\kappa" },
    SymbolItem { glyph: "λ", label: "\\lambda", insert: "\\lambda" },
    SymbolItem { glyph: "μ", label: "\\mu", insert: "\\mu" },
    SymbolItem { glyph: "ν", label: "\\nu", insert: "\\nu" },
    SymbolItem { glyph: "ξ", label: "\\xi", insert: "\\xi" },
    SymbolItem { glyph: "π", label: "\\pi", insert: "\\pi" },
    SymbolItem { glyph: "ρ", label: "\\rho", insert: "\\rho" },
    SymbolItem { glyph: "σ", label: "\\sigma", insert: "\\sigma" },
    SymbolItem { glyph: "τ", label: "\\tau", insert: "\\tau" },
    SymbolItem { glyph: "φ", label: "\\varphi", insert: "\\varphi" },
    SymbolItem { glyph: "χ", label: "\\chi", insert: "\\chi" },
    SymbolItem { glyph: "ψ", label: "\\psi", insert: "\\psi" },
    SymbolItem { glyph: "ω", label: "\\omega", insert: "\\omega" },
    SymbolItem { glyph: "Γ", label: "\\Gamma", insert: "\\Gamma" },
    SymbolItem { glyph: "Δ", label: "\\Delta", insert: "\\Delta" },
    SymbolItem { glyph: "Θ", label: "\\Theta", insert: "\\Theta" },
    SymbolItem { glyph: "Λ", label: "\\Lambda", insert: "\\Lambda" },
    SymbolItem { glyph: "Ξ", label: "\\Xi", insert: "\\Xi" },
    SymbolItem { glyph: "Π", label: "\\Pi", insert: "\\Pi" },
    SymbolItem { glyph: "Σ", label: "\\Sigma", insert: "\\Sigma" },
    SymbolItem { glyph: "Φ", label: "\\Phi", insert: "\\Phi" },
    SymbolItem { glyph: "Ψ", label: "\\Psi", insert: "\\Psi" },
    SymbolItem { glyph: "Ω", label: "\\Omega", insert: "\\Omega" },
];

pub const MATH_SYMBOLS: &[SymbolItem] = &[
    // Insiemi e appartenenza
    SymbolItem { glyph: "∈", label: "\\in", insert: "\\in " },
    SymbolItem { glyph: "∉", label: "\\notin", insert: "\\notin " },
    SymbolItem { glyph: "⊆", label: "\\subseteq", insert: "\\subseteq " },
    SymbolItem { glyph: "⊂", label: "\\subset", insert: "\\subset " },
    SymbolItem { glyph: "⊇", label: "\\supseteq", insert: "\\supseteq " },
    SymbolItem { glyph: "⊃", label: "\\supset", insert: "\\supset " },
    SymbolItem { glyph: "⊈", label: "\\nsubseteq", insert: "\\nsubseteq " },
    SymbolItem { glyph: "∪", label: "\\cup", insert: "\\cup " },
    SymbolItem { glyph: "∩", label: "\\cap", insert: "\\cap " },
    SymbolItem { glyph: "⋃", label: "\\bigcup", insert: "\\bigcup_{i=1}^{n} " },
    SymbolItem { glyph: "⋂", label: "\\bigcap", insert: "\\bigcap_{i=1}^{n} " },
    SymbolItem { glyph: "∅", label: "\\emptyset", insert: "\\emptyset" },
    SymbolItem { glyph: "∖", label: "\\setminus", insert: "\\setminus " },
    // Insiemi numerici e spazi
    SymbolItem { glyph: "ℕ", label: "\\mathbb{N}", insert: "\\mathbb{N}" },
    SymbolItem { glyph: "ℤ", label: "\\mathbb{Z}", insert: "\\mathbb{Z}" },
    SymbolItem { glyph: "ℚ", label: "\\mathbb{Q}", insert: "\\mathbb{Q}" },
    SymbolItem { glyph: "ℝ", label: "\\mathbb{R}", insert: "\\mathbb{R}" },
    SymbolItem { glyph: "ℂ", label: "\\mathbb{C}", insert: "\\mathbb{C}" },
    SymbolItem { glyph: "ℙ", label: "\\mathbb{P}", insert: "\\mathbb{P}" },
    SymbolItem { glyph: "𝔼", label: "\\mathbb{E}", insert: "\\mathbb{E}" },
    SymbolItem { glyph: "𝟙", label: "\\mathds{1}", insert: "\\mathds{1}" },
    SymbolItem { glyph: "𝒫", label: "\\mathcal{P}", insert: "\\mathcal{P}()" },
    SymbolItem { glyph: "ℒ", label: "\\mathcal{L}", insert: "\\mathcal{L}" },
    SymbolItem { glyph: "𝒪", label: "\\mathcal{O}", insert: "\\mathcal{O}()" },
    SymbolItem { glyph: "ℱ", label: "\\mathcal{F}", insert: "\\mathcal{F}" },
    // Reticoli e ordini
    SymbolItem { glyph: "⊑", label: "\\sqsubseteq", insert: "\\sqsubseteq " },
    SymbolItem { glyph: "⊒", label: "\\sqsupseteq", insert: "\\sqsupseteq " },
    SymbolItem { glyph: "⊏", label: "\\sqsubset", insert: "\\sqsubset " },
    SymbolItem { glyph: "⊔", label: "\\sqcup", insert: "\\sqcup " },
    SymbolItem { glyph: "⊓", label: "\\sqcap", insert: "\\sqcap " },
    SymbolItem { glyph: "⨆", label: "\\bigsqcup", insert: "\\bigsqcup " },
    SymbolItem { glyph: "≺", label: "\\prec", insert: "\\prec " },
    SymbolItem { glyph: "⪯", label: "\\preceq", insert: "\\preceq " },
    SymbolItem { glyph: "≻", label: "\\succ", insert: "\\succ " },
    SymbolItem { glyph: "⪰", label: "\\succeq", insert: "\\succeq " },
    // Confronti e relazioni
    SymbolItem { glyph: "≤", label: "\\le", insert: "\\le " },
    SymbolItem { glyph: "≥", label: "\\ge", insert: "\\ge " },
    SymbolItem { glyph: "≠", label: "\\neq", insert: "\\neq " },
    SymbolItem { glyph: "≈", label: "\\approx", insert: "\\approx " },
    SymbolItem { glyph: "∼", label: "\\sim", insert: "\\sim " },
    SymbolItem { glyph: "≃", label: "\\simeq", insert: "\\simeq " },
    SymbolItem { glyph: "≅", label: "\\cong", insert: "\\cong " },
    SymbolItem { glyph: "∝", label: "\\propto", insert: "\\propto " },
    SymbolItem { glyph: "≪", label: "\\ll", insert: "\\ll " },
    SymbolItem { glyph: "≫", label: "\\gg", insert: "\\gg " },
    SymbolItem { glyph: "∣", label: "\\mid", insert: "\\mid " },
    SymbolItem { glyph: "∤", label: "\\nmid", insert: "\\nmid " },
    SymbolItem { glyph: "⊥", label: "\\perp", insert: "\\perp " },
    SymbolItem { glyph: "∥", label: "\\parallel", insert: "\\parallel " },
    // Operazioni algebriche
    SymbolItem { glyph: "±", label: "\\pm", insert: "\\pm " },
    SymbolItem { glyph: "∓", label: "\\mp", insert: "\\mp " },
    SymbolItem { glyph: "×", label: "\\times", insert: "\\times " },
    SymbolItem { glyph: "·", label: "\\cdot", insert: "\\cdot " },
    SymbolItem { glyph: "÷", label: "\\div", insert: "\\div " },
    SymbolItem { glyph: "∘", label: "\\circ", insert: "\\circ " },
    SymbolItem { glyph: "∙", label: "\\bullet", insert: "\\bullet " },
    SymbolItem { glyph: "⋆", label: "\\star", insert: "\\star " },
    SymbolItem { glyph: "⊗", label: "\\otimes", insert: "\\otimes " },
    SymbolItem { glyph: "⊕", label: "\\oplus", insert: "\\oplus " },
    SymbolItem { glyph: "⨁", label: "\\bigoplus", insert: "\\bigoplus " },
    SymbolItem { glyph: "⨂", label: "\\bigotimes", insert: "\\bigotimes " },
    // Frecce e funzioni
    SymbolItem { glyph: "↦", label: "\\mapsto", insert: "\\mapsto " },
    SymbolItem { glyph: "⇀", label: "\\rightharpoonup", insert: "\\rightharpoonup " },
    SymbolItem { glyph: "⇝", label: "\\rightsquigarrow", insert: "\\rightsquigarrow " },
    SymbolItem { glyph: "⇉", label: "\\rightrightarrows", insert: "\\rightrightarrows " },
    SymbolItem { glyph: "↪", label: "\\hookrightarrow", insert: "\\hookrightarrow " },
    SymbolItem { glyph: "↠", label: "\\twoheadrightarrow", insert: "\\twoheadrightarrow " },
    SymbolItem { glyph: "↛", label: "\\not\\to", insert: "\\not\\to " },
    SymbolItem { glyph: "a→", label: "\\xrightarrow{}", insert: "\\xrightarrow{}" },
    SymbolItem { glyph: "a⇒", label: "\\xRightarrow{}", insert: "\\xRightarrow{}" },
    SymbolItem { glyph: "↑", label: "\\uparrow", insert: "\\uparrow " },
    SymbolItem { glyph: "↓", label: "\\downarrow", insert: "\\downarrow " },
    // Analisi, somme, integrali
    SymbolItem { glyph: "∞", label: "\\infty", insert: "\\infty" },
    SymbolItem { glyph: "∂", label: "\\partial", insert: "\\partial " },
    SymbolItem { glyph: "∇", label: "\\nabla", insert: "\\nabla " },
    SymbolItem { glyph: "∑", label: "\\sum", insert: "\\sum_{i=1}^{n} " },
    SymbolItem { glyph: "∏", label: "\\prod", insert: "\\prod_{i=1}^{n} " },
    SymbolItem { glyph: "∫", label: "\\int", insert: "\\int_{a}^{b} " },
    SymbolItem { glyph: "∬", label: "\\iint", insert: "\\iint_{D} " },
    SymbolItem { glyph: "∭", label: "\\iiint", insert: "\\iiint_{V} " },
    SymbolItem { glyph: "∮", label: "\\oint", insert: "\\oint_{C} " },
    SymbolItem { glyph: "lim", label: "\\lim", insert: "\\lim_{x \\to \\infty} " },
    SymbolItem { glyph: "√", label: "\\sqrt{}", insert: "\\sqrt{}" },
    SymbolItem { glyph: "nCk", label: "\\binom{}{}", insert: "\\binom{n}{k}" },
    // Delimitatori, vettori, accenti, puntini
    SymbolItem { glyph: "⟨⟩", label: "\\langle \\rangle", insert: "\\langle , \\rangle" },
    SymbolItem { glyph: "‖‖", label: "\\lVert \\rVert", insert: "\\lVert  \\rVert" },
    SymbolItem { glyph: "⌊⌋", label: "\\lfloor \\rfloor", insert: "\\lfloor  \\rfloor" },
    SymbolItem { glyph: "⌈⌉", label: "\\lceil \\rceil", insert: "\\lceil  \\rceil" },
    SymbolItem { glyph: "x̄", label: "\\overline{}", insert: "\\overline{}" },
    SymbolItem { glyph: "x̂", label: "\\widehat{}", insert: "\\widehat{}" },
    SymbolItem { glyph: "x̃", label: "\\widetilde{}", insert: "\\widetilde{}" },
    SymbolItem { glyph: "v⃗", label: "\\vec{}", insert: "\\vec{}" },
    SymbolItem { glyph: "v", label: "\\bm{}", insert: "\\bm{}" },
    SymbolItem { glyph: "▷", label: "\\triangleright", insert: "\\triangleright" },
    SymbolItem { glyph: "◂", label: "\\blacktriangleleft", insert: "\\blacktriangleleft" },
    SymbolItem { glyph: "†", label: "^{\\dagger}", insert: "^{\\dagger}" },
    SymbolItem { glyph: "⋯", label: "\\cdots", insert: "\\cdots " },
    SymbolItem { glyph: "…", label: "\\dots", insert: "\\dots " },
    SymbolItem { glyph: "⋮", label: "\\vdots", insert: "\\vdots " },
    SymbolItem { glyph: "⋱", label: "\\ddots", insert: "\\ddots " },
];

pub const FORMULAS: &[FormulaItem] = &[
    FormulaItem {
        glyph: "a/b",
        label: "Frazione  \\frac{a}{b}",
        insert: "\\frac{a}{b}",
        context: FormulaContext::InlineMath,
        required_package: None,
        preamble_setup: None,
    },
    FormulaItem {
        glyph: "dy/dx",
        label: "Derivata (physics)  \\dv{f}{x}",
        insert: "\\dv{f}{x}",
        context: FormulaContext::InlineMath,
        required_package: Some("physics"),
        preamble_setup: None,
    },
    FormulaItem {
        glyph: "∂f/∂x",
        label: "Derivata parziale  \\frac{\\partial f}{\\partial x}",
        insert: "\\frac{\\partial f}{\\partial x}",
        context: FormulaContext::InlineMath,
        required_package: None,
        preamble_setup: None,
    },
    FormulaItem {
        glyph: "nCk",
        label: "Coefficiente binomiale  \\binom{n}{k}",
        insert: "\\binom{n}{k}",
        context: FormulaContext::InlineMath,
        required_package: Some("amsmath"),
        preamble_setup: None,
    },
    FormulaItem {
        glyph: "⟨φ|ψ⟩",
        label: "Notazione di Dirac  \\langle \\phi \\mid \\psi \\rangle",
        insert: "\\langle \\phi \\mid \\psi \\rangle",
        context: FormulaContext::InlineMath,
        required_package: None,
        preamble_setup: None,
    },
    FormulaItem {
        glyph: "P / C",
        label: "Regola di inferenza",
        insert: "  \\frac{P_1 \\quad P_2}{C}",
        context: FormulaContext::DisplayMath,
        required_package: None,
        preamble_setup: None,
    },
    FormulaItem {
        glyph: "{P}C{Q}",
        label: "Tripla di Hoare",
        insert: "\\{ P \\}\\; C \\;\\{ Q \\}",
        context: FormulaContext::InlineMath,
        required_package: None,
        preamble_setup: None,
    },
    FormulaItem {
        glyph: "a = b",
        label: "Ambiente align*",
        insert: "\\begin{align*}\n  a &= b \\\\\n\\end{align*}\n",
        context: FormulaContext::Block,
        required_package: Some("amsmath"),
        preamble_setup: None,
    },
    FormulaItem {
        glyph: "{ x",
        label: "Sistema / cases",
        insert: "\\begin{cases}\n  x & \\text{se } x \\ge 0 \\\\\n  -x & \\text{altrimenti}\n\\end{cases}",
        context: FormulaContext::DisplayMath,
        required_package: Some("amsmath"),
        preamble_setup: None,
    },
    FormulaItem {
        glyph: "[a b]",
        label: "Matrice 2x2 (bmatrix)",
        insert: "\\begin{bmatrix}\n  a & b \\\\\n  c & d\n\\end{bmatrix}",
        context: FormulaContext::DisplayMath,
        required_package: Some("amsmath"),
        preamble_setup: None,
    },
    FormulaItem {
        glyph: "[3×3]",
        label: "Matrice 3x3 (pmatrix)",
        insert: "\\begin{pmatrix}\n  a_{11} & a_{12} & a_{13} \\\\\n  a_{21} & a_{22} & a_{23} \\\\\n  a_{31} & a_{32} & a_{33}\n\\end{pmatrix}",
        context: FormulaContext::DisplayMath,
        required_package: Some("amsmath"),
        preamble_setup: None,
    },
    FormulaItem {
        glyph: "□",
        label: "Ambiente proof",
        insert: "\\begin{proof}\n  \n\\end{proof}\n",
        context: FormulaContext::Block,
        required_package: Some("amsthm"),
        preamble_setup: None,
    },
    FormulaItem {
        glyph: "Thm",
        label: "Ambiente theorem",
        insert: "\\begin{theorem}\n  \n\\end{theorem}\n",
        context: FormulaContext::Block,
        required_package: Some("amsthm"),
        preamble_setup: Some("\\newtheorem{theorem}{Teorema}"),
    },
    FormulaItem {
        glyph: "Def",
        label: "Ambiente definition",
        insert: "\\begin{definition}\n  \n\\end{definition}\n",
        context: FormulaContext::Block,
        required_package: Some("amsthm"),
        preamble_setup: Some("\\newtheorem{definition}{Definizione}"),
    },
    FormulaItem {
        glyph: "A → B",
        label: "Diagramma commutativo (tikz-cd)",
        insert: "\\begin{tikzcd}\n  A \\arrow[r, \"f\"] \\arrow[d, \"g\"'] & B \\arrow[d, \"h\"] \\\\\n  C \\arrow[r, \"k\"'] & D\n\\end{tikzcd}",
        context: FormulaContext::DisplayMath,
        required_package: Some("tikz-cd"),
        preamble_setup: None,
    },
    FormulaItem {
        glyph: "f(x)",
        label: "Grafico funzione (pgfplots)",
        insert: "\\begin{tikzpicture}\n  \\begin{axis}[xlabel=$x$, ylabel=$f(x)$, grid=major]\n    \\addplot[domain=-2:2, samples=100, thick] {x^2};\n  \\end{axis}\n\\end{tikzpicture}\n",
        context: FormulaContext::Block,
        required_package: Some("pgfplots"),
        preamble_setup: Some("\\pgfplotsset{compat=1.18}"),
    },
    FormulaItem {
        glyph: "Algo",
        label: "Algoritmo (algorithm2e)",
        insert: "\\begin{algorithm}[H]\n  \\caption{Titolo algoritmo}\n  \\KwIn{Input}\n  \\KwOut{Output}\n  \\Return{risultato}\\;\n\\end{algorithm}\n",
        context: FormulaContext::Block,
        required_package: Some("algorithm2e"),
        preamble_setup: None,
    },
];

pub const MATH_LIBRARIES: &[MathLibrary] = &[
    // Core, Algebra e Teoremi
    MathLibrary {
        package: "amsmath",
        description: "Ambienti equazioni avanzati (align, gather, cases, matrici)",
        category: LibraryCategory::CoreMath,
        texlive_group: "texlive-latex",
    },
    MathLibrary {
        package: "amssymb",
        description: "Simboli matematici AMS (\\mathbb, \\vdash, \\models, \\Box)",
        category: LibraryCategory::CoreMath,
        texlive_group: "texlive-basic",
    },
    MathLibrary {
        package: "amsthm",
        description: "Ambienti theorem, lemma, proposition, definition e proof",
        category: LibraryCategory::CoreMath,
        texlive_group: "texlive-latex",
    },
    MathLibrary {
        package: "mathtools",
        description: "Estensioni di amsmath (\\coloneqq, dcases, delimitatori)",
        category: LibraryCategory::CoreMath,
        texlive_group: "texlive-latexrecommended",
    },
    MathLibrary {
        package: "bm",
        description: "Grassetto matematico per vettori, matrici e lettere greche (\\bm)",
        category: LibraryCategory::CoreMath,
        texlive_group: "texlive-latex",
    },
    MathLibrary {
        package: "mathrsfs",
        description: "Lettere calligrafiche eleganti (\\mathscr{L}, \\mathscr{F})",
        category: LibraryCategory::CoreMath,
        texlive_group: "texlive-fontsrecommended",
    },
    MathLibrary {
        package: "dsfont",
        description: "Blackboard bold esteso (\\mathds{1} per funzioni indicatrici)",
        category: LibraryCategory::CoreMath,
        texlive_group: "texlive-fontsextra",
    },
    MathLibrary {
        package: "cancel",
        description: "Semplificazioni e barre di cancellazione (\\cancel, \\cancelto)",
        category: LibraryCategory::CoreMath,
        texlive_group: "texlive-latexextra",
    },
    MathLibrary {
        package: "nicematrix",
        description: "Matrici evolute con blocchi, linee tratteggiate e annotazioni",
        category: LibraryCategory::CoreMath,
        texlive_group: "texlive-mathscience",
    },
    MathLibrary {
        package: "thmtools",
        description: "Teoremi personalizzati, elenchi teoremi e restatable",
        category: LibraryCategory::CoreMath,
        texlive_group: "texlive-latexextra",
    },
    // Analisi, Calcolo e Fisica
    MathLibrary {
        package: "physics",
        description: "Derivate (\\dv, \\pdv), vettori, operatori e notazione Dirac",
        category: LibraryCategory::AnalysisPhysics,
        texlive_group: "texlive-mathscience",
    },
    MathLibrary {
        package: "derivative",
        description: "Derivate ordinarie e parziali moderne (\\odv, \\pdv)",
        category: LibraryCategory::AnalysisPhysics,
        texlive_group: "texlive-mathscience",
    },
    MathLibrary {
        package: "esint",
        description: "Integrali multipli e chiusi estesi (\\oiint, \\fint, \\iint)",
        category: LibraryCategory::AnalysisPhysics,
        texlive_group: "texlive-fontsextra",
    },
    MathLibrary {
        package: "siunitx",
        description: "Unità di misura SI e formattazione scientifica (\\qty, \\num)",
        category: LibraryCategory::AnalysisPhysics,
        texlive_group: "texlive-mathscience",
    },
    MathLibrary {
        package: "braket",
        description: "Notazione di Dirac leggera (\\bra, \\ket, \\braket, \\set)",
        category: LibraryCategory::AnalysisPhysics,
        texlive_group: "texlive-mathscience",
    },
    MathLibrary {
        package: "tensor",
        description: "Indici covarianti e controvarianti per calcolo tensoriale",
        category: LibraryCategory::AnalysisPhysics,
        texlive_group: "texlive-mathscience",
    },
    MathLibrary {
        package: "diffcoeff",
        description: "Coefficienti differenziali e derivate parziali (\\diff, \\diffp)",
        category: LibraryCategory::AnalysisPhysics,
        texlive_group: "texlive-mathscience",
    },
    // Logica e Informatica
    MathLibrary {
        package: "stmaryrd",
        description: "Parentesi semantiche (\\llbracket, \\rrbracket) e simboli logici",
        category: LibraryCategory::LogicCs,
        texlive_group: "texlive-fontsextra",
    },
    MathLibrary {
        package: "bussproofs",
        description: "Alberi di derivazione e calcolo dei sequenti",
        category: LibraryCategory::LogicCs,
        texlive_group: "texlive-mathscience",
    },
    MathLibrary {
        package: "ebproof",
        description: "Alberi di prova in stile deduzione naturale (prooftree)",
        category: LibraryCategory::LogicCs,
        texlive_group: "texlive-mathscience",
    },
    MathLibrary {
        package: "algorithm2e",
        description: "Pseudocodice strutturato e algoritmi numerati",
        category: LibraryCategory::LogicCs,
        texlive_group: "texlive-mathscience",
    },
    MathLibrary {
        package: "algpseudocode",
        description: "Pseudocodice classico (algorithmic, \\State, \\For, \\While)",
        category: LibraryCategory::LogicCs,
        texlive_group: "texlive-mathscience",
    },
    MathLibrary {
        package: "listings",
        description: "Evidenziazione e impaginazione di codice sorgente",
        category: LibraryCategory::LogicCs,
        texlive_group: "texlive-latexrecommended",
    },
    // Grafici, Diagrammi e Box
    MathLibrary {
        package: "tikz",
        description: "Grafici vettoriali, automi, alberi e figure geometriche",
        category: LibraryCategory::GraphicsDiagrams,
        texlive_group: "texlive-pictures",
    },
    MathLibrary {
        package: "tikz-cd",
        description: "Diagrammi commutativi per algebra e teoria delle categorie",
        category: LibraryCategory::GraphicsDiagrams,
        texlive_group: "texlive-pictures",
    },
    MathLibrary {
        package: "pgfplots",
        description: "Plot 2D/3D di funzioni matematiche e dati sperimentali",
        category: LibraryCategory::GraphicsDiagrams,
        texlive_group: "texlive-pictures",
    },
    MathLibrary {
        package: "xcolor",
        description: "Colori personalizzati per formule, testo e diagrammi",
        category: LibraryCategory::GraphicsDiagrams,
        texlive_group: "texlive-latexrecommended",
    },
    MathLibrary {
        package: "tcolorbox",
        description: "Riquadri eleganti e colorati per teoremi, definizioni ed esempi",
        category: LibraryCategory::GraphicsDiagrams,
        texlive_group: "texlive-latexextra",
    },
    // Bibliografia e Pubblicazione
    MathLibrary {
        package: "biblatex",
        description: "Bibliografia moderna con Biber (\\addbibresource, \\parencite, \\printbibliography)",
        category: LibraryCategory::Bibliography,
        texlive_group: "texlive-bibtexextra",
    },
    MathLibrary {
        package: "natbib",
        description: "Citazioni accademiche autore-anno/numeriche con BibTeX (\\citet, \\citep)",
        category: LibraryCategory::Bibliography,
        texlive_group: "texlive-latexrecommended",
    },
    MathLibrary {
        package: "csquotes",
        description: "Virgolette tipografiche multilingua (richiesto/consigliato da biblatex)",
        category: LibraryCategory::Bibliography,
        texlive_group: "texlive-latexrecommended",
    },
    MathLibrary {
        package: "booktabs",
        description: "Tabelle scientifiche professionali (\\toprule, \\midrule, \\bottomrule)",
        category: LibraryCategory::Bibliography,
        texlive_group: "texlive-latexrecommended",
    },
    MathLibrary {
        package: "subcaption",
        description: "Sottofigure e sottotabelle affiancate con etichette (subfigure)",
        category: LibraryCategory::Bibliography,
        texlive_group: "texlive-latexextra",
    },
    MathLibrary {
        package: "glossaries",
        description: "Glossari, indici terminologici e acronimi automatici (\\gls)",
        category: LibraryCategory::Bibliography,
        texlive_group: "texlive-latexextra",
    },
    MathLibrary {
        package: "minted",
        description: "Codice sorgente evidenziato con Pygments (richiede --shell-escape)",
        category: LibraryCategory::Bibliography,
        texlive_group: "texlive-latexextra",
    },
    MathLibrary {
        package: "orcidlink",
        description: "Badge cliccabile ORCID per gli autori di articoli e paper (\\orcidlink)",
        category: LibraryCategory::Bibliography,
        texlive_group: "texlive-latexextra",
    },
    // Tipografia e Riferimenti
    MathLibrary {
        package: "hyperref",
        description: "Collegamenti ipertestuali cliccabili su equazioni e sezioni",
        category: LibraryCategory::Typography,
        texlive_group: "texlive-latex",
    },
    MathLibrary {
        package: "cleveref",
        description: "Riferimenti intelligenti automatici (\\cref{thm:1})",
        category: LibraryCategory::Typography,
        texlive_group: "texlive-latexextra",
    },
    MathLibrary {
        package: "enumitem",
        description: "Liste personalizzabili per ipotesi e dimostrazioni",
        category: LibraryCategory::Typography,
        texlive_group: "texlive-latexextra",
    },
    MathLibrary {
        package: "geometry",
        description: "Controllo preciso di margini, formato pagina e layout",
        category: LibraryCategory::Typography,
        texlive_group: "texlive-latexrecommended",
    },
];

const EXTRA_COMMANDS: &[&str] = &[
    "\\begin{document}",
    "\\begin{align*}",
    "\\begin{align}",
    "\\begin{equation*}",
    "\\begin{equation}",
    "\\begin{gather*}",
    "\\begin{proof}",
    "\\begin{theorem}",
    "\\begin{lemma}",
    "\\begin{proposition}",
    "\\begin{corollary}",
    "\\begin{definition}",
    "\\begin{remark}",
    "\\begin{itemize}",
    "\\begin{enumerate}",
    "\\begin{cases}",
    "\\begin{dcases}",
    "\\begin{bmatrix}",
    "\\begin{pmatrix}",
    "\\begin{vmatrix}",
    "\\begin{tikzpicture}",
    "\\begin{tikzcd}",
    "\\begin{prooftree}",
    "\\begin{algorithm}",
    "\\section{}",
    "\\subsection{}",
    "\\subsubsection{}",
    "\\textbf{}",
    "\\textit{}",
    "\\emph{}",
    "\\text{}",
    "\\textcolor{}{}",
    "\\frac{}{}",
    "\\dfrac{}{}",
    "\\tfrac{}{}",
    "\\binom{}{}",
    "\\sqrt{}",
    "\\overline{}",
    "\\underline{}",
    "\\overbrace{}^{}",
    "\\underbrace{}_{}",
    "\\widehat{}",
    "\\widetilde{}",
    "\\hat{}",
    "\\tilde{}",
    "\\vec{}",
    "\\bm{}",
    "\\mathbb{}",
    "\\mathcal{}",
    "\\mathscr{}",
    "\\mathfrak{}",
    "\\mathds{}",
    "\\mathrm{}",
    "\\mathtt{}",
    "\\mathbf{}",
    "\\operatorname{}",
    "\\DeclareMathOperator{}{}",
    "\\newtheorem{}{}",
    "\\dv{}{}",
    "\\pdv{}{}",
    "\\odv{}{}",
    "\\qty{}{}",
    "\\num{}",
    "\\bra{}",
    "\\ket{}",
    "\\braket{}{}",
    "\\cancel{}",
    "\\cancelto{}{}",
    "\\left(  \\right)",
    "\\left[  \\right]",
    "\\left\\{  \\right\\}",
    "\\usepackage{}",
    "\\newcommand{}{}",
    "\\label{}",
    "\\ref{}",
    "\\eqref{}",
    "\\cref{}",
    "\\cite{}",
    "\\citet{}",
    "\\citep{}",
    "\\parencite{}",
    "\\textcite{}",
    "\\autocite{}",
    "\\nocite{}",
    "\\bibliography{}",
    "\\bibliographystyle{}",
    "\\addbibresource{}",
    "\\printbibliography",
    "\\gls{}",
    "\\newacronym{}{}{}",
    "\\toprule",
    "\\midrule",
    "\\bottomrule",
    "\\llbracket",
    "\\rrbracket",
    "\\coloneqq",
    "\\subseteq",
    "\\supseteq",
    "\\sqsubseteq",
    "\\sqsupseteq",
    "\\rightsquigarrow",
    "\\rightrightarrows",
    "\\rightharpoonup",
    "\\implies",
    "\\iff",
    "\\xrightarrow{}",
    "\\xRightarrow{}",
    "\\substack{}",
    "\\quad",
    "\\qquad",
];

pub fn suggest_commands(prefix: &str) -> Vec<String> {
    if !prefix.starts_with('\\') || prefix.len() < 2 {
        return Vec::new();
    }

    let lower = prefix.to_lowercase();
    let mut matches: Vec<String> = Vec::new();

    let all_sources = LOGIC_SYMBOLS
        .iter()
        .chain(GREEK_SYMBOLS.iter())
        .chain(MATH_SYMBOLS.iter())
        .map(|s| s.insert.trim())
        .chain(EXTRA_COMMANDS.iter().copied());

    for cmd in all_sources {
        if cmd.to_lowercase().starts_with(&lower) && cmd != prefix {
            if !matches.iter().any(|m| m == cmd) {
                matches.push(cmd.to_string());
                if matches.len() >= 8 {
                    break;
                }
            }
        }
    }

    matches
}
