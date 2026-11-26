mod compiler;
mod document;
mod matcher;
#[cfg(target_os = "linux")]
mod poppler_ffi;
mod project;
mod quality;
mod recent;
mod recipes;
mod settings;
mod symbols;
mod syntax;
mod theme;
mod ui;

use compiler::{
    CompileDiagnostic, CompileResult, PdfSyncTarget, RenderedPage, RenderedPageData,
    ReverseSyncResult,
};
use document::Document;
use iced::{clipboard, keyboard};
use iced::widget::{image, scrollable, text_editor, text_input};
use iced::{event, window, Element, Event, Point, Size, Subscription, Task, Theme};
use matcher::MatchCandidate;
use project::{
    BibEntry, CompletionContext, FolderSearchHit, LabelEntry, OutlineFilter, OutlineItem,
    SearchOptions, SuggestionItem, TextMatch,
};
use quality::{LintWarning, SpellCheckOutcome, WordCountStats};
use recipes::{RecipeConfig, RecipeStep};
use settings::UserSettings;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use symbols::{FormulaItem, PaletteTab, MATH_LIBRARIES};

/// Maximum number of decoded PDF pages retained in memory & GPU atlas at once.
/// Keeps GPU VRAM small and prevents `iced_wgpu` O(N^2) texture-array reallocations.
const MAX_CACHED_PDF_PAGES: usize = 12;

pub fn main() -> iced::Result {
    iced::application(App::title, App::update, App::view)
        .window(window::Settings {
            size: Size::new(1280.0, 800.0),
            min_size: Some(Size::new(760.0, 500.0)),
            ..Default::default()
        })
        .exit_on_close_request(false)
        .default_font(theme::ui_font())
        .antialiasing(true)
        .theme(App::theme)
        .subscription(App::subscription)
        .run_with(App::init)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Welcome,
    Editor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogTab {
    Compilation,
    Linter,
    Spellcheck,
}

#[derive(Debug, Clone, Copy)]
pub struct PdfContextMenu {
    pub page: usize,   // 1-based
    pub x_ratio: f32,  // 0.0..=1.0 across page width
    pub y_ratio: f32,  // 0.0..=1.0 down page height
    pub anchor: Point, // window-space click position
}

#[derive(Debug, Clone)]
pub struct StagedBuild {
    pub text_hash: u64,
    pub result: CompileResult,
}

pub struct TabState {
    pub id: u64,
    pub doc: Document,
    pub compile_log: String,
    pub diagnostics: Vec<CompileDiagnostic>,
    pub error_lines: HashSet<usize>, // 0-based line indices
    pub sync_highlight_line: Option<usize>, // 0-based line highlighted after reverse sync
    pub show_raw_log: bool,
    pub last_edited_line: Option<usize>, // 1-based line of last user modification
    pub last_compile_ok: Option<bool>,
    pub last_pdf: Option<PathBuf>,
    pub pdf_total_pages: usize,
    pub pdf_aspect_ratio: f32,
    pub pdf_page_width_pts: f32,
    pub pdf_page_height_pts: f32,
    pub pdf_sync_page: Option<usize>,
    pub pdf_scroll_offset: scrollable::AbsoluteOffset,
    pub preview_note: Option<String>,
    pub editor_scroll_offset: scrollable::AbsoluteOffset,
    pub staged_build: Option<StagedBuild>,
    pub word_count: Option<WordCountStats>,
    pub lint_warnings: Vec<LintWarning>,
    pub spell_outcome: Option<SpellCheckOutcome>,
}

impl TabState {
    fn new(id: u64) -> Self {
        Self {
            id,
            doc: Document::default(),
            compile_log: "Premi Compila (o Ctrl+B) per eseguire pdflatex.".to_string(),
            diagnostics: Vec::new(),
            error_lines: HashSet::new(),
            sync_highlight_line: None,
            show_raw_log: false,
            last_edited_line: None,
            last_compile_ok: None,
            last_pdf: None,
            pdf_total_pages: 0,
            pdf_aspect_ratio: 1.4142,
            pdf_page_width_pts: 595.28,
            pdf_page_height_pts: 841.89,
            pdf_sync_page: None,
            pdf_scroll_offset: scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
            preview_note: None,
            editor_scroll_offset: scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
            staged_build: None,
            word_count: None,
            lint_warnings: Vec::new(),
            spell_outcome: None,
        }
    }

    fn is_untouched_placeholder(&self) -> bool {
        self.doc.path.is_none() && !self.doc.is_dirty && self.last_compile_ok.is_none()
    }

    fn clear_pdf(&mut self) {
        self.last_pdf = None;
        self.pdf_total_pages = 0;
        self.pdf_sync_page = None;
        self.pdf_scroll_offset = scrollable::AbsoluteOffset { x: 0.0, y: 0.0 };
        self.preview_note = None;
    }
}

pub struct App {
    pub screen: Screen,
    pub tabs: Vec<TabState>,
    pub active_tab: usize,
    next_tab_id: u64,
    pub recent_files: Vec<PathBuf>,
    pub folder_files: Vec<PathBuf>,
    pub show_folder_menu: bool,
    pub recipe_config: RecipeConfig,
    pub show_recipe_menu: bool,
    pub show_new_recipe_builder: bool,
    pub new_recipe_name: String,
    pub new_recipe_steps: Vec<RecipeStep>,
    pub status: String,
    pub is_compiling: bool,
    pub live_compile: bool,
    pub is_live_compiling: bool,
    live_revision: u64,
    live_in_flight_hash: Option<(u64, u64)>, // (tab_id, text_hash)
    live_started_at: Option<std::time::Instant>,
    live_debounce_ms: u64,
    live_pending_requeue: bool,
    promote_live_on_finish: bool,
    live_task_handle: Option<iced::task::Handle>,
    pub show_log: bool,
    pub log_tab: LogTab,
    pub is_linting: bool,
    pub is_spellchecking: bool,
    pub spell_lang: String,
    pub show_pdf: bool,
    pub show_symbols: bool,
    pub show_match: bool,
    pub show_outline: bool,
    pub show_assets: bool,
    pub figure_width_ratio: f32,
    pub table_rows: usize,
    pub table_cols: usize,
    pub table_align: char,
    pub table_booktabs: bool,
    pub table_header: bool,
    pub table_caption_env: bool,
    pub bib_import_input: String,
    pub is_fetching_bib: bool,
    pub last_imported_bib: Option<BibEntry>,
    pub cached_bib_entries: Vec<BibEntry>,
    pub cached_label_entries: Vec<LabelEntry>,
    pub outline_filter: OutlineFilter,
    pub outline_items: Vec<OutlineItem>,
    pub master_tex_path: Option<PathBuf>,
    pub show_find_bar: bool,
    pub show_replace_row: bool,
    pub find_in_folder: bool,
    pub find_query: String,
    pub replace_query: String,
    pub find_case_sensitive: bool,
    pub find_use_regex: bool,
    pub find_matches: Vec<TextMatch>,
    pub find_active_idx: Option<usize>,
    pub find_error: Option<String>,
    pub folder_search_hits: Vec<FolderSearchHit>,
    pub find_input_id: text_input::Id,
    pub replace_input_id: text_input::Id,
    pub show_goto_bar: bool,
    pub goto_line_input: String,
    pub goto_input_id: text_input::Id,
    pub dark_mode: bool,
    pub auto_close: bool,
    persist_settings_enabled: bool,
    pub palette_tab: PaletteTab,
    pub suggestions: Vec<SuggestionItem>,
    pub selected_suggestion: usize,
    pub active_prefix: Option<String>,
    pub pdf_generation: u64,
    pub pdf_page_cache: HashMap<usize, image::Handle>,
    pub pdf_lru: VecDeque<usize>,
    pub pdf_rendering: HashSet<usize>,
    pub pdf_cache_width: u32,
    pdf_in_flight_width: u32,
    pdf_resize_rev: u64,
    pub pdf_scroll_id: scrollable::Id,
    pub pdf_viewport_height: f32,
    pub pdf_context_menu: Option<PdfContextMenu>,
    pub editor_font_size: f32,
    pub pdf_zoom: f32,
    pub editor_scroll_id: scrollable::Id,
    pub editor_viewport_height: f32,
    pub editor_viewport_width: f32,
    pub editor_dragging: bool,
    editor_drag_local: Option<Point>,
    pub window_size: Size,
    pub window_scale_factor: f32,
    pub installed_tex_packages: Option<HashSet<String>>,
    pub match_strokes: Vec<Vec<Point>>,
    pub match_results: Vec<MatchCandidate>,
    internal_clipboard: Option<String>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            screen: Screen::Welcome,
            tabs: vec![TabState::new(1)],
            active_tab: 0,
            next_tab_id: 2,
            recent_files: recent::load_recent(),
            folder_files: Vec::new(),
            show_folder_menu: false,
            recipe_config: recipes::load_recipe_config(),
            show_recipe_menu: false,
            show_new_recipe_builder: false,
            new_recipe_name: String::new(),
            new_recipe_steps: Vec::new(),
            status: "Pronto".to_string(),
            is_compiling: false,
            live_compile: true,
            is_live_compiling: false,
            live_revision: 0,
            live_in_flight_hash: None,
            live_started_at: None,
            live_debounce_ms: 0,
            live_pending_requeue: false,
            promote_live_on_finish: false,
            live_task_handle: None,
            show_log: false,
            log_tab: LogTab::Compilation,
            is_linting: false,
            is_spellchecking: false,
            spell_lang: "it_IT".to_string(),
            show_pdf: false,
            show_symbols: false,
            show_match: false,
            show_outline: false,
            show_assets: false,
            figure_width_ratio: 0.8,
            table_rows: 3,
            table_cols: 3,
            table_align: 'c',
            table_booktabs: true,
            table_header: true,
            table_caption_env: true,
            bib_import_input: String::new(),
            is_fetching_bib: false,
            last_imported_bib: None,
            cached_bib_entries: Vec::new(),
            cached_label_entries: Vec::new(),
            outline_filter: OutlineFilter::All,
            outline_items: Vec::new(),
            master_tex_path: None,
            show_find_bar: false,
            show_replace_row: false,
            find_in_folder: false,
            find_query: String::new(),
            replace_query: String::new(),
            find_case_sensitive: false,
            find_use_regex: false,
            find_matches: Vec::new(),
            find_active_idx: None,
            find_error: None,
            folder_search_hits: Vec::new(),
            find_input_id: text_input::Id::unique(),
            replace_input_id: text_input::Id::unique(),
            show_goto_bar: false,
            goto_line_input: String::new(),
            goto_input_id: text_input::Id::unique(),
            dark_mode: false,
            auto_close: true,
            persist_settings_enabled: false,
            palette_tab: PaletteTab::Logic,
            suggestions: Vec::new(),
            selected_suggestion: 0,
            active_prefix: None,
            pdf_generation: 0,
            pdf_page_cache: HashMap::new(),
            pdf_lru: VecDeque::new(),
            pdf_rendering: HashSet::new(),
            pdf_cache_width: 0,
            pdf_in_flight_width: 0,
            pdf_resize_rev: 0,
            pdf_scroll_id: scrollable::Id::unique(),
            pdf_viewport_height: 700.0,
            pdf_context_menu: None,
            editor_font_size: 16.0,
            pdf_zoom: 1.0,
            editor_scroll_id: scrollable::Id::unique(),
            editor_viewport_height: 640.0,
            editor_viewport_width: 0.0,
            editor_dragging: false,
            editor_drag_local: None,
            window_size: Size::new(1280.0, 800.0),
            window_scale_factor: 1.0,
            installed_tex_packages: None,
            match_strokes: Vec::new(),
            match_results: Vec::new(),
            internal_clipboard: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    GoHome,
    SelectTab(usize),
    CloseTab(usize),
    CloseActiveTab,
    NextTab,
    PrevTab,
    ToggleFolderMenu,
    ToggleRecipeMenu,
    SelectRecipe(String),
    ToggleShellEscape,
    ToggleNewRecipeBuilder,
    NewRecipeNameChanged(String),
    NewRecipeAddStep(RecipeStep),
    NewRecipePopStep,
    NewRecipeClearSteps,
    SaveCustomRecipe,
    DeleteCustomRecipe(String),
    DismissOverlays,
    ToggleLog,
    SelectLogTab(LogTab),
    ToggleRawLog,
    TogglePdf,
    ToggleSymbols,
    ToggleMatch,
    ToggleOutline,
    ToggleAssets,
    SetFigureWidth(f32),
    PickFigureImage,
    FigureImagePicked(Option<PathBuf>),
    FileDropped(PathBuf),
    SetTableRows(usize),
    SetTableCols(usize),
    SetTableAlign(char),
    ToggleTableBooktabs,
    ToggleTableHeader,
    ToggleTableCaptionEnv,
    InsertGeneratedTable,
    BibImportInputChanged(String),
    FetchBibFromIdentifier,
    BibEntryFetched(Result<(String, BibEntry), String>),
    InsertCitationKey(String),
    RefreshWordCount,
    WordCountUpdated(u64, Option<WordCountStats>),
    RunChktex,
    ChktexFinished(u64, Result<Vec<LintWarning>, String>),
    SetSpellLang(String),
    RunSpellcheck,
    SpellcheckFinished(u64, SpellCheckOutcome),
    RunLatexdiffGit,
    RunLatexdiffFile,
    LatexdiffTexReady(u64, Result<PathBuf, String>),
    SetOutlineFilter(OutlineFilter),
    TogglePinMaster,
    ClearMaster,
    OutlineJump {
        file_path: Option<PathBuf>,
        line_1based: usize,
    },
    OpenFindBar {
        replace: bool,
    },
    CloseFindBar,
    ToggleReplaceRow,
    FindQueryChanged(String),
    ReplaceQueryChanged(String),
    ToggleFindCase,
    ToggleFindRegex,
    ToggleFindInFolder,
    FindNext,
    FindPrev,
    ReplaceCurrent,
    ReplaceAll,
    OpenGotoLine,
    CloseGotoLine,
    GotoLineChanged(String),
    GotoLineSubmit,
    ToggleComment,
    IndentSelection,
    UnindentSelection,
    ToggleTheme,
    ToggleAutoClose,
    ToggleLiveCompile,
    LiveCompileFinished(u64, u64, u64, Result<CompileResult, String>),
    StagedPromoteFinished(u64, bool, Result<CompileResult, String>),
    OpenPdfExternal,
    SyncPdfToLastEdit,
    PdfSynced(Option<PdfSyncTarget>),
    PdfLeftClicked {
        page: usize,
        x_ratio: f32,
        y_ratio: f32,
    },
    PdfRightClicked {
        page: usize,
        x_ratio: f32,
        y_ratio: f32,
        anchor: Point,
    },
    PdfViewSource,
    ReverseSyncResolved(Option<ReverseSyncResult>),
    JumpToLine(usize),
    MatchStrokeCompleted(Vec<Point>),
    MatchUndo,
    MatchClear,
    MatchInsert(usize),
    ZoomInEditor,
    ZoomOutEditor,
    ResetZoomEditor,
    ZoomInPdf,
    ZoomOutPdf,
    ResetZoomPdf,
    SelectPaletteTab(PaletteTab),
    InsertSnippet(&'static str),
    InsertFormula(&'static FormulaItem),
    AddLibrary(&'static str),
    AddLibraryPreset(&'static str, &'static [&'static str]),
    InstalledPackagesDetected(Option<HashSet<String>>),
    ApplySuggestion(usize),
    AcceptSuggestion,
    NextSuggestion,
    PrevSuggestion,
    DismissSuggestions,
    EditorAction(text_editor::Action),
    EditorDragMoved { local: Point, viewport: Size },
    EditorDragTick,
    EditorDragEnded,
    EditorCopy,
    EditorCut,
    EditorPasteRequest,
    EditorPasteReady(Option<String>),
    EditorUndo,
    EditorRedo,
    PreamblePreloaded,
    EditorScrolled(scrollable::Viewport),
    PdfScrolled(scrollable::Viewport),
    PdfPagesRendered(Result<(u64, Vec<RenderedPage>), String>),
    PdfResizeDebounced(u64),
    WindowOpened(window::Id),
    WindowResized(window::Id, Size),
    ScaleFactorDetected(f32),
    CloseRequested(window::Id),
    New,
    Open,
    OpenRecent(PathBuf),
    FileOpened(Result<(PathBuf, String), String>),
    Save,
    FileSaved(u64, Result<PathBuf, String>),
    Compile,
    CompileAfterSave(u64, Result<PathBuf, String>),
    CompileFinished(u64, Result<CompileResult, String>),
    CleanBuild,
    BuildCleaned(u64, Result<String, String>),
}

fn write_wl_clipboard_bg(text: String) {
    // Fallback for Wayland / KDE Plasma 6 where `smithay-clipboard` can be unavailable.
    std::thread::spawn(move || {
        use std::io::Write;
        use std::process::{Command, Stdio};
        if let Ok(mut child) = Command::new("wl-copy")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
        }
    });
}

async fn read_wl_clipboard() -> Option<String> {
    let out = tokio::process::Command::new("wl-paste")
        .arg("--no-newline")
        .output()
        .await
        .ok()?;
    if out.status.success() {
        String::from_utf8(out.stdout).ok()
    } else {
        None
    }
}

fn paths_match(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

impl App {
    pub fn tab(&self) -> &TabState {
        &self.tabs[self.active_tab]
    }

    pub fn tab_mut(&mut self) -> &mut TabState {
        &mut self.tabs[self.active_tab]
    }

    fn alloc_tab_id(&mut self) -> u64 {
        let id = self.next_tab_id;
        self.next_tab_id = self.next_tab_id.wrapping_add(1);
        id
    }

    fn find_tab_by_id(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    fn find_tab_by_path(&self, path: &Path) -> Option<usize> {
        self.tabs
            .iter()
            .position(|t| t.doc.path.as_deref().is_some_and(|p| paths_match(p, path)))
    }

    fn refresh_folder_files(&mut self) {
        let dir = self
            .tab()
            .doc
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf);
        self.folder_files = match dir {
            Some(d) => recent::list_folder_tex_files(&d),
            None => Vec::new(),
        };
    }

    fn init() -> (Self, Task<Message>) {
        let mut app = Self::default();
        let saved = settings::load_settings();
        app.dark_mode = saved.dark_mode;
        app.editor_font_size = saved.editor_font_size;
        app.auto_close = saved.auto_close;
        app.live_compile = saved.live_compile;
        app.spell_lang = saved.spell_lang;
        app.persist_settings_enabled = true;

        let detect_pkgs = Task::perform(
            compiler::detect_installed_packages(),
            Message::InstalledPackagesDetected,
        );
        let detect_scale = window::get_oldest().then(|opt| match opt {
            Some(id) => window::get_scale_factor(id).map(Message::ScaleFactorDetected),
            None => Task::none(),
        });
        (app, Task::batch([detect_pkgs, detect_scale]))
    }

    fn persist_user_settings(&self) {
        if !self.persist_settings_enabled {
            return;
        }
        settings::save_settings(&UserSettings {
            dark_mode: self.dark_mode,
            editor_font_size: self.editor_font_size,
            auto_close: self.auto_close,
            live_compile: self.live_compile,
            spell_lang: self.spell_lang.clone(),
        });
    }

    fn refresh_bib_cache(&mut self) {
        let root_dir = self.project_search_root_dir();
        let open_buffers = self.collect_open_buffers();
        let active_tex = self.tab().doc.text();
        self.cached_bib_entries =
            project::collect_project_bib_entries(root_dir.as_deref(), &open_buffers, &active_tex);
    }

    fn refresh_label_cache(&mut self) {
        let root_path = self
            .effective_master_path()
            .or_else(|| self.tab().doc.path.clone());
        let open_buffers = self.collect_open_buffers();
        let fallback_text = self.tab().doc.text();
        let fallback_name = self.tab().doc.file_name();
        self.cached_label_entries = project::collect_project_labels(
            root_path.as_deref(),
            &fallback_text,
            &fallback_name,
            &open_buffers,
        );
    }

    fn spawn_word_count_task(&self) -> Task<Message> {
        let tab = self.tab();
        let tab_id = tab.id;
        let root_file = self
            .effective_master_path()
            .or_else(|| tab.doc.path.clone());
        let use_disk_inc = root_file.is_some() && !tab.doc.is_dirty;
        let unsaved_text = tab.doc.text();
        Task::perform(
            quality::run_texcount(root_file, use_disk_inc, unsaved_text),
            move |stats| Message::WordCountUpdated(tab_id, stats),
        )
    }

    fn spawn_chktex_task(&mut self) -> Task<Message> {
        let tab = self.tab();
        let tab_id = tab.id;
        let source = tab.doc.text();
        self.is_linting = true;
        Task::perform(quality::run_chktex(source), move |res| {
            Message::ChktexFinished(tab_id, res)
        })
    }

    fn insert_image_asset(&mut self, img_path: PathBuf) -> Task<Message> {
        let active_tex = self.tab().doc.path.clone();
        let width_ratio = self.figure_width_ratio;
        match project::prepare_figure_snippet(&img_path, active_tex.as_deref(), width_ratio) {
            Ok((snippet, pkg)) => {
                let added_pkg = self.tab_mut().doc.add_package(pkg);
                self.tab_mut().doc.insert_snippet(&snippet);
                let cur_l = self.tab().doc.content.cursor_position().0 + 1;
                self.tab_mut().last_edited_line = Some(cur_l);
                self.tab_mut().sync_highlight_line = None;
                self.refresh_outline();
                self.refresh_label_cache();
                self.refresh_suggestions();
                let img_name = img_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("immagine");
                self.status = if added_pkg {
                    format!("Figura '{img_name}' inserita (+ \\usepackage{{{pkg}}})")
                } else {
                    format!("Figura '{img_name}' inserita")
                };
                let scroll_task = self.ensure_cursor_visible();
                let live_task = self.schedule_live_compile(650, false);
                Task::batch([scroll_task, live_task])
            }
            Err(err) => {
                self.status = err;
                Task::none()
            }
        }
    }

    fn title(&self) -> String {
        match self.screen {
            Screen::Welcome => "FeatherTex".to_string(),
            Screen::Editor => format!("FeatherTex — {}", self.tab().doc.display_name()),
        }
    }

    fn theme(&self) -> Theme {
        if self.dark_mode {
            Theme::Dark
        } else {
            Theme::Light
        }
    }

    fn reset_gpu_pdf_cache(&mut self) {
        self.pdf_generation = self.pdf_generation.wrapping_add(1);
        self.pdf_page_cache.clear();
        self.pdf_lru.clear();
        self.pdf_rendering.clear();
        self.pdf_cache_width = 0;
        self.pdf_in_flight_width = 0;
        self.pdf_context_menu = None;
    }

    fn clear_active_pdf_state(&mut self) {
        self.reset_gpu_pdf_cache();
        self.tab_mut().clear_pdf();
    }

    fn cancel_live_compile(&mut self) {
        self.live_revision = self.live_revision.wrapping_add(1);
        if let Some(handle) = self.live_task_handle.take() {
            handle.abort();
        }
        self.is_live_compiling = false;
        self.live_in_flight_hash = None;
        self.live_started_at = None;
        self.live_debounce_ms = 0;
        self.live_pending_requeue = false;
        self.promote_live_on_finish = false;
    }

    /// Schedules an asynchronous background pre-compilation for the active tab using a smart
    /// coalescing policy:
    /// - If a previous task is still waiting in debounce, resets the debounce cleanly.
    /// - If `pdflatex` is ALREADY actively running for the same tab, lets it finish instead of
    ///   killing it mid-run and marks `live_pending_requeue = true` for one fresh follow-up.
    pub fn effective_master_for_tab(&self, idx: usize) -> Option<PathBuf> {
        let tab = self.tabs.get(idx)?;
        let magic = recipes::parse_magic_comments(&tab.doc.text());
        if magic.root.is_some() {
            if let Some(cur_path) = tab.doc.path.as_deref() {
                let resolved = recipes::resolve_root_path(cur_path, &magic);
                if resolved.is_file() {
                    return Some(resolved);
                }
            }
        }
        let master = self.master_tex_path.as_ref()?;
        if !master.is_file() {
            return None;
        }
        let Some(master_dir) = master.parent() else {
            return Some(master.clone());
        };
        match tab.doc.path.as_deref() {
            None => Some(master.clone()),
            Some(cur_p) => {
                let canon_master_dir =
                    std::fs::canonicalize(master_dir).unwrap_or_else(|_| master_dir.to_path_buf());
                let canon_cur =
                    std::fs::canonicalize(cur_p).unwrap_or_else(|_| cur_p.to_path_buf());
                if canon_cur.starts_with(&canon_master_dir) || cur_p.starts_with(master_dir) {
                    Some(master.clone())
                } else {
                    None
                }
            }
        }
    }

    pub fn effective_master_path(&self) -> Option<PathBuf> {
        self.effective_master_for_tab(self.active_tab)
    }

    pub fn is_active_tab_pinned_master(&self) -> bool {
        let Some(m) = self.master_tex_path.as_deref() else {
            return false;
        };
        self.tab()
            .doc
            .path
            .as_deref()
            .is_some_and(|p| paths_match(p, m))
    }

    fn collect_open_buffers(&self) -> Vec<(PathBuf, String)> {
        self.tabs
            .iter()
            .filter_map(|t| t.doc.path.as_ref().map(|p| (p.clone(), t.doc.text())))
            .collect()
    }

    pub fn refresh_outline(&mut self) {
        if !self.show_outline {
            return;
        }
        let root_path = self
            .effective_master_path()
            .or_else(|| self.tab().doc.path.clone());
        let open_buffers = self.collect_open_buffers();
        let fallback_text = self.tab().doc.text();
        let fallback_name = self.tab().doc.file_name();
        self.outline_items = project::build_project_outline(
            root_path.as_deref(),
            &fallback_text,
            &fallback_name,
            &open_buffers,
        );
    }

    fn project_search_root_dir(&self) -> Option<PathBuf> {
        self.effective_master_path()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .or_else(|| {
                self.tab()
                    .doc
                    .path
                    .as_ref()
                    .and_then(|p| p.parent().map(Path::to_path_buf))
            })
    }

    fn refresh_find_state(&mut self, select_nearest: bool) -> Task<Message> {
        if !self.show_find_bar || self.find_query.is_empty() {
            self.find_matches.clear();
            self.find_active_idx = None;
            self.find_error = None;
            self.folder_search_hits.clear();
            return Task::none();
        }

        let opts = SearchOptions {
            case_sensitive: self.find_case_sensitive,
            use_regex: self.find_use_regex,
        };
        let text = self.tab().doc.text();
        match project::find_matches(&text, &self.find_query, opts) {
            Ok(matches) => {
                self.find_error = None;
                self.find_matches = matches;
                if self.find_in_folder {
                    if let Some(dir) = self.project_search_root_dir() {
                        let open_buffers = self.collect_open_buffers();
                        if let Ok(hits) =
                            project::search_in_folder(&dir, &self.find_query, opts, &open_buffers)
                        {
                            self.folder_search_hits = hits;
                        }
                    } else {
                        self.folder_search_hits.clear();
                    }
                } else {
                    self.folder_search_hits.clear();
                }

                if self.find_matches.is_empty() {
                    self.find_active_idx = None;
                    return Task::none();
                }

                let cur_byte = self.tab().doc.cursor_byte_offset();
                let idx = self
                    .find_matches
                    .iter()
                    .position(|m| m.start_byte >= cur_byte)
                    .unwrap_or(0);
                self.find_active_idx = Some(idx);

                if select_nearest {
                    let m = self.find_matches[idx].clone();
                    let line_0 = self.tab_mut().doc.select_byte_range(m.start_byte, m.end_byte);
                    return self.ensure_line_visible(line_0);
                }
                Task::none()
            }
            Err(err) => {
                self.find_error = Some(err);
                self.find_matches.clear();
                self.find_active_idx = None;
                self.folder_search_hits.clear();
                Task::none()
            }
        }
    }

    fn step_find_match(&mut self, forward: bool) -> Task<Message> {
        if self.find_query.is_empty() {
            return Task::none();
        }
        if self.find_matches.is_empty() {
            let _ = self.refresh_find_state(false);
        }
        if self.find_matches.is_empty() {
            return Task::none();
        }

        let len = self.find_matches.len();
        let next_idx = match self.find_active_idx {
            Some(cur) => {
                if forward {
                    (cur + 1) % len
                } else if cur == 0 {
                    len - 1
                } else {
                    cur - 1
                }
            }
            None => 0,
        };
        self.find_active_idx = Some(next_idx);
        let m = self.find_matches[next_idx].clone();
        let line_0 = self.tab_mut().doc.select_byte_range(m.start_byte, m.end_byte);
        self.ensure_line_visible(line_0)
    }

    pub fn can_use_fast_live_pdflatex(&self) -> bool {
        if self.recipe_config.shell_escape {
            return false;
        }
        if !self.recipe_config.active_recipe().is_fast_single_pdflatex() {
            return false;
        }
        let magic = recipes::parse_magic_comments(&self.tab().doc.text());
        if magic.root.is_some() || magic.bib_program.is_some() {
            return false;
        }
        if let Some(prog) = magic.program.as_deref() {
            if prog != "pdflatex" {
                return false;
            }
        }
        if let Some(master) = self.effective_master_path() {
            let same = self
                .tab()
                .doc
                .path
                .as_deref()
                .is_some_and(|p| paths_match(p, &master));
            if !same {
                return false;
            }
        }
        true
    }

    fn schedule_live_compile(&mut self, debounce_ms: u64, force: bool) -> Task<Message> {
        self.tab_mut().staged_build = None;

        if !self.live_compile
            || self.is_compiling
            || self.screen != Screen::Editor
            || !self.can_use_fast_live_pdflatex()
        {
            self.cancel_live_compile();
            return Task::none();
        }

        let tab = self.tab();
        let Some(tex_path) = tab.doc.path.clone() else {
            self.cancel_live_compile();
            return Task::none();
        };
        let tab_id = tab.id;

        if !force
            && self.is_live_compiling
            && !self.promote_live_on_finish
            && self.live_in_flight_hash.is_some_and(|(id, _)| id == tab_id)
            && self
                .live_started_at
                .is_some_and(|t| t.elapsed() >= std::time::Duration::from_millis(self.live_debounce_ms))
        {
            self.live_pending_requeue = true;
            return Task::none();
        }

        self.cancel_live_compile();

        let contents = self.tab().doc.text();
        let text_hash = compiler::text_fingerprint(&contents);
        let rev = self.live_revision;
        self.is_live_compiling = true;
        self.live_in_flight_hash = Some((tab_id, text_hash));
        self.live_started_at = Some(std::time::Instant::now());
        self.live_debounce_ms = debounce_ms;
        self.live_pending_requeue = false;

        let (task, handle) = Task::perform(
            compiler::compile_latex_live(tex_path, contents, debounce_ms),
            move |res| Message::LiveCompileFinished(tab_id, rev, text_hash, res),
        )
        .abortable();

        self.live_task_handle = Some(handle);
        task
    }

    fn spawn_promote_task(
        &self,
        tab_id: u64,
        idx: usize,
        tex_path: PathBuf,
        result: CompileResult,
        instant_swap: bool,
    ) -> Task<Message> {
        let sync_line = self.tabs[idx]
            .last_edited_line
            .unwrap_or_else(|| self.tabs[idx].doc.content.cursor_position().0 + 1);
        let fallback_page = if idx == self.active_tab {
            self.current_pdf_start_page()
        } else {
            1
        };
        let target_w = self.target_pdf_render_width();
        Task::perform(
            compiler::finalize_promoted_live_result(
                result,
                tex_path,
                sync_line,
                fallback_page,
                target_w,
            ),
            move |res| Message::StagedPromoteFinished(tab_id, instant_swap, res),
        )
    }

    fn apply_compile_result(
        &mut self,
        idx: usize,
        result: CompileResult,
        instant_swap: bool,
    ) -> Task<Message> {
        let is_active = idx == self.active_tab;
        let tab = &mut self.tabs[idx];
        tab.compile_log = result.log;
        tab.last_compile_ok = Some(result.success);

        if result.success {
            tab.diagnostics.clear();
            tab.error_lines.clear();
            tab.last_pdf = result.pdf_path;
            tab.pdf_total_pages = result.total_pages;
            tab.pdf_aspect_ratio = result.page_aspect_ratio;
            tab.pdf_page_width_pts = result.page_width_pts;
            tab.pdf_page_height_pts = result.page_height_pts;
            tab.pdf_sync_page = result.sync_target.map(|t| t.page);
            tab.preview_note = result.preview_note;

            if is_active {
                let target_w = self.target_pdf_render_width();
                self.reset_gpu_pdf_cache();
                self.show_pdf = true;
                self.show_log = false;

                if let Some(target) = result.sync_target {
                    let target_y = self.pdf_scroll_y_for_target(target);
                    let x = self.tab().pdf_scroll_offset.x;
                    self.tab_mut().pdf_scroll_offset.y = target_y;
                    self.insert_rendered_pages(result.initial_pages, target_w);
                    self.status = if instant_swap {
                        format!("Compilato istantaneamente · pag. {}", target.page)
                    } else {
                        format!(
                            "Compilazione riuscita ({:.1}s) · pag. {}",
                            result.elapsed_secs, target.page
                        )
                    };
                    let scroll_task = scrollable::scroll_to(
                        self.pdf_scroll_id.clone(),
                        scrollable::AbsoluteOffset { x, y: target_y },
                    );
                    return Task::batch([scroll_task, self.request_visible_pdf_pages()]);
                } else {
                    self.insert_rendered_pages(result.initial_pages, target_w);
                    self.status = if instant_swap {
                        "Compilato istantaneamente".to_string()
                    } else {
                        format!("Compilazione riuscita ({:.1}s)", result.elapsed_secs)
                    };
                    return self.request_visible_pdf_pages();
                }
            } else {
                self.status = format!(
                    "Compilazione di {} riuscita ({:.1}s)",
                    self.tabs[idx].doc.file_name(),
                    result.elapsed_secs
                );
            }
        } else {
            tab.diagnostics = result.diagnostics;
            tab.error_lines = tab
                .diagnostics
                .iter()
                .filter_map(|d| d.line.map(|l| l.saturating_sub(1)))
                .collect();
            tab.show_raw_log = tab.diagnostics.is_empty();
            let err_count = tab.diagnostics.len();
            let first_line = tab.diagnostics.iter().find_map(|d| d.line);

            if is_active {
                self.show_pdf = false;
                self.show_log = true;
                self.status = if err_count == 1 {
                    "1 errore di compilazione".to_string()
                } else if err_count > 1 {
                    format!("{err_count} errori di compilazione")
                } else {
                    "Errori di compilazione".to_string()
                };

                if let Some(line) = first_line {
                    return self.ensure_line_visible(line.saturating_sub(1));
                }
            } else {
                self.status = format!(
                    "Errori di compilazione in {}",
                    self.tabs[idx].doc.file_name()
                );
            }
        }
        Task::none()
    }

    fn switch_to_tab(&mut self, index: usize) -> Task<Message> {
        if self.tabs.is_empty() {
            return Task::none();
        }
        self.cancel_live_compile();
        self.editor_dragging = false;
        self.editor_drag_local = None;
        let target = index.min(self.tabs.len() - 1);
        self.active_tab = target;
        self.suggestions.clear();
        self.selected_suggestion = 0;
        self.active_prefix = None;
        self.show_folder_menu = false;
        self.show_recipe_menu = false;
        self.reset_gpu_pdf_cache();
        self.refresh_folder_files();
        self.refresh_outline();
        self.refresh_bib_cache();
        self.refresh_label_cache();
        if self.show_find_bar {
            let _ = self.refresh_find_state(false);
        }

        let editor_off = self.tab().editor_scroll_offset;
        let pdf_off = self.tab().pdf_scroll_offset;
        let has_pdf = self.tab().last_pdf.is_some();

        let mut tasks = vec![
            scrollable::scroll_to(self.editor_scroll_id.clone(), editor_off),
            self.spawn_word_count_task(),
        ];
        if self.show_pdf && has_pdf {
            tasks.push(scrollable::scroll_to(self.pdf_scroll_id.clone(), pdf_off));
            tasks.push(self.request_visible_pdf_pages());
        }
        Task::batch(tasks)
    }

    /// Saves every dirty open tab synchronously before exit (or tab close).
    /// Named files overwrite their disk path; unsaved untitled tabs are persisted
    /// into `~/.config/feathertex/autosave/` and recorded in recent files so no work is lost.
    fn save_all_open_files(&mut self) -> usize {
        let mut saved = 0usize;
        for tab in &mut self.tabs {
            if !tab.doc.is_dirty {
                continue;
            }
            if let Some(path) = tab.doc.path.clone() {
                if std::fs::write(&path, tab.doc.text()).is_ok() {
                    tab.doc.is_dirty = false;
                    saved += 1;
                }
            } else if let Some(dir) = recent::autosave_dir() {
                if std::fs::create_dir_all(&dir).is_ok() {
                    let autosave_path = dir.join(format!("senza_titolo_{}.tex", tab.id));
                    if std::fs::write(&autosave_path, tab.doc.text()).is_ok() {
                        tab.doc.path = Some(autosave_path.clone());
                        tab.doc.is_dirty = false;
                        recent::record_recent(&mut self.recent_files, autosave_path);
                        saved += 1;
                    }
                }
            }
        }
        saved
    }

    pub fn editor_font_scale(&self) -> f32 {
        let pane_w = ui::editor_pane_width(self);
        (pane_w / 680.0).clamp(0.65, 1.0)
    }

    pub fn effective_editor_font_size(&self) -> f32 {
        let scale = self.editor_font_scale();
        let scaled = (self.editor_font_size * scale).round();
        let min_floor = if self.editor_font_size >= 14.0 {
            12.0
        } else {
            11.0
        };
        scaled.clamp(min_floor, 30.0)
    }

    fn pdf_scroll_y_for_target(&self, target: PdfSyncTarget) -> f32 {
        let tab = self.tab();
        if tab.pdf_total_pages == 0 {
            return 0.0;
        }
        let fit_w = ui::pdf_fit_base_width(self);
        let (_, page_h) = ui::pdf_page_dimensions(fit_w, self.pdf_zoom, tab.pdf_aspect_ratio);
        let stride = ui::pdf_page_stride(fit_w, self.pdf_zoom, tab.pdf_aspect_ratio);
        let page_idx = target.page.saturating_sub(1).min(tab.pdf_total_pages - 1);
        let point_y = ui::PDF_LIST_PADDING
            + (page_idx as f32) * stride
            + ui::PDF_CARD_PADDING
            + target.v_ratio.clamp(0.0, 1.0) * page_h;

        let vh = self.pdf_viewport_height.max(400.0);
        let total_h = ui::PDF_LIST_PADDING * 2.0
            + (tab.pdf_total_pages as f32) * stride
            - ui::PDF_PAGE_SPACING;
        let max_y = (total_h - vh).max(0.0);
        (point_y - vh * 0.30).clamp(0.0, max_y).round()
    }

    fn scroll_pdf_to_target(&mut self, target: PdfSyncTarget) -> Task<Message> {
        let target_y = self.pdf_scroll_y_for_target(target);
        let x = self.tab().pdf_scroll_offset.x;
        self.tab_mut().pdf_scroll_offset.y = target_y;
        self.tab_mut().pdf_sync_page = Some(target.page);
        let scroll_task = scrollable::scroll_to(
            self.pdf_scroll_id.clone(),
            scrollable::AbsoluteOffset { x, y: target_y },
        );
        Task::batch([scroll_task, self.request_visible_pdf_pages()])
    }

    fn run_reverse_sync(&mut self, page: usize, x_ratio: f32, y_ratio: f32) -> Task<Message> {
        let tab = self.tab();
        let Some(pdf_path) = tab.last_pdf.clone() else {
            return Task::none();
        };
        let x_pts = x_ratio.clamp(0.0, 1.0) * tab.pdf_page_width_pts;
        let y_pts = y_ratio.clamp(0.0, 1.0) * tab.pdf_page_height_pts;
        self.status = format!("Salto al codice sorgente (pag. {page})...");
        Task::perform(
            async move { compiler::synctex_reverse_search(&pdf_path, page, x_pts, y_pts).await },
            Message::ReverseSyncResolved,
        )
    }

    fn ensure_line_visible(&mut self, line_0based: usize) -> Task<Message> {
        let line_height = ui::editor_line_height(self.effective_editor_font_size());
        let line_top = ui::EDITOR_PADDING + (line_0based as f32) * line_height;
        let line_bottom = line_top + line_height;

        let view_top = self.tab().editor_scroll_offset.y;
        let view_bottom = view_top + self.editor_viewport_height;

        if line_top < view_top + ui::EDITOR_PADDING || line_bottom > view_bottom - ui::EDITOR_PADDING
        {
            let new_y = (line_top - self.editor_viewport_height * 0.30)
                .max(0.0)
                .round();
            self.tab_mut().editor_scroll_offset.y = new_y;
            self.tab_mut().editor_scroll_offset.x = 0.0;
            scrollable::scroll_to(
                self.editor_scroll_id.clone(),
                scrollable::AbsoluteOffset { x: 0.0, y: new_y },
            )
        } else {
            Task::none()
        }
    }

    fn insert_rendered_pages(&mut self, pages: Vec<RenderedPage>, rendered_width: u32) {
        if !pages.is_empty() && rendered_width > 0 {
            self.pdf_cache_width = rendered_width;
        }
        for page in pages {
            let num = page.page_number;
            let handle = match page.data {
                RenderedPageData::Png(bytes) => image::Handle::from_bytes(bytes),
                RenderedPageData::Rgba {
                    width,
                    height,
                    bytes,
                } => image::Handle::from_rgba(width, height, bytes),
            };
            self.pdf_page_cache.insert(num, handle);
            self.pdf_lru.retain(|&p| p != num);
            self.pdf_lru.push_back(num);
        }

        let (vis_first, vis_last) = self.visible_pdf_page_range();
        while self.pdf_page_cache.len() > MAX_CACHED_PDF_PAGES {
            let evict_pos = self
                .pdf_lru
                .iter()
                .position(|&p| p < vis_first || p > vis_last);
            if let Some(pos) = evict_pos {
                if let Some(evicted) = self.pdf_lru.remove(pos) {
                    self.pdf_page_cache.remove(&evicted);
                }
            } else {
                break;
            }
        }
    }

    /// Returns the inclusive 1-based range of PDF pages currently visible (plus 1 buffer page each side).
    pub fn visible_pdf_page_range(&self) -> (usize, usize) {
        let tab = self.tab();
        if tab.pdf_total_pages == 0 {
            return (1, 0);
        }
        let fit_w = ui::pdf_fit_base_width(self);
        let stride = ui::pdf_page_stride(fit_w, self.pdf_zoom, tab.pdf_aspect_ratio);
        let y = (tab.pdf_scroll_offset.y - ui::PDF_LIST_PADDING).max(0.0);
        let vh = self.pdf_viewport_height.max(400.0);

        let first_zero = (y / stride).floor() as usize;
        let last_zero = ((y + vh) / stride).ceil() as usize;

        let first = first_zero.saturating_sub(1) + 1;
        let last = (last_zero + 2).min(tab.pdf_total_pages);
        (first.min(tab.pdf_total_pages), last)
    }

    fn current_pdf_start_page(&self) -> usize {
        let tab = self.tab();
        if tab.pdf_total_pages == 0 {
            return 1;
        }
        let fit_w = ui::pdf_fit_base_width(self);
        let stride = ui::pdf_page_stride(fit_w, self.pdf_zoom, tab.pdf_aspect_ratio);
        let y = (tab.pdf_scroll_offset.y - ui::PDF_LIST_PADDING).max(0.0);
        ((y / stride).floor() as usize + 1).clamp(1, tab.pdf_total_pages)
    }

    fn target_pdf_render_size(&self) -> (u32, u32) {
        let fit_w = ui::pdf_fit_base_width(self);
        let (logical_w, logical_h) =
            ui::pdf_page_dimensions(fit_w, self.pdf_zoom, self.tab().pdf_aspect_ratio);
        let phys_w = (logical_w * self.window_scale_factor)
            .round()
            .clamp(240.0, 2800.0) as u32;
        let phys_h = (logical_h * self.window_scale_factor)
            .round()
            .clamp(240.0, 4000.0) as u32;
        (phys_w, phys_h)
    }

    pub fn target_pdf_render_width(&self) -> u32 {
        self.target_pdf_render_size().0
    }

    fn handle_pdf_width_change(&mut self, old_fit: f32, debounce: bool) -> Task<Message> {
        let new_fit = ui::pdf_fit_base_width(self);
        if !self.show_pdf || self.tab().pdf_total_pages == 0 || (new_fit - old_fit).abs() < 1.0 {
            return Task::none();
        }

        let aspect = self.tab().pdf_aspect_ratio;
        let old_stride = ui::pdf_page_stride(old_fit, self.pdf_zoom, aspect);
        let new_stride = ui::pdf_page_stride(new_fit, self.pdf_zoom, aspect);
        let old_y = (self.tab().pdf_scroll_offset.y - ui::PDF_LIST_PADDING).max(0.0);
        let new_y = (old_y * (new_stride / old_stride) + ui::PDF_LIST_PADDING).round();

        self.tab_mut().pdf_scroll_offset.y = new_y;
        if self.pdf_zoom <= 1.001 {
            self.tab_mut().pdf_scroll_offset.x = 0.0;
        }
        let scroll_offset = self.tab().pdf_scroll_offset;
        let scroll_task = scrollable::scroll_to(self.pdf_scroll_id.clone(), scroll_offset);

        if debounce {
            self.pdf_resize_rev = self.pdf_resize_rev.wrapping_add(1);
            let rev = self.pdf_resize_rev;
            let debounce_task = Task::perform(
                async move {
                    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                    rev
                },
                Message::PdfResizeDebounced,
            );
            Task::batch([scroll_task, debounce_task])
        } else {
            if self.pdf_cache_width != self.target_pdf_render_width() {
                self.reset_gpu_pdf_cache();
            }
            Task::batch([scroll_task, self.request_visible_pdf_pages()])
        }
    }

    fn set_pdf_zoom(&mut self, new_zoom: f32) -> Task<Message> {
        let clamped = new_zoom.clamp(0.5, 2.5);
        if (clamped - self.pdf_zoom).abs() < 0.001 {
            return Task::none();
        }
        let old_zoom = self.pdf_zoom;
        let fit_w = ui::pdf_fit_base_width(self);
        let aspect = self.tab().pdf_aspect_ratio;
        let old_stride = ui::pdf_page_stride(fit_w, old_zoom, aspect);
        let new_stride = ui::pdf_page_stride(fit_w, clamped, aspect);
        let old_y = (self.tab().pdf_scroll_offset.y - ui::PDF_LIST_PADDING).max(0.0);
        let new_y = (old_y * (new_stride / old_stride) + ui::PDF_LIST_PADDING).round();

        self.pdf_zoom = clamped;
        self.tab_mut().pdf_scroll_offset.y = new_y;
        if clamped <= 1.001 {
            self.tab_mut().pdf_scroll_offset.x = 0.0;
        }
        self.reset_gpu_pdf_cache();

        let scroll_task = scrollable::scroll_to(
            self.pdf_scroll_id.clone(),
            scrollable::AbsoluteOffset {
                x: self.tab().pdf_scroll_offset.x,
                y: new_y,
            },
        );
        Task::batch([scroll_task, self.request_visible_pdf_pages()])
    }

    fn request_visible_pdf_pages(&mut self) -> Task<Message> {
        let tab = self.tab();
        if !self.show_pdf || tab.pdf_total_pages == 0 || !self.pdf_rendering.is_empty() {
            return Task::none();
        }
        let Some(pdf_path) = tab.last_pdf.clone() else {
            return Task::none();
        };

        let (first, last) = self.visible_pdf_page_range();
        let mut missing: Vec<usize> = Vec::new();
        for p in first..=last {
            if !self.pdf_page_cache.contains_key(&p) && !self.pdf_rendering.contains(&p) {
                missing.push(p);
                if missing.len() >= 4 {
                    break;
                }
            }
        }

        if missing.is_empty() {
            return Task::none();
        }

        for &p in &missing {
            self.pdf_rendering.insert(p);
        }

        let gen = self.pdf_generation;
        let (target_w, target_h) = self.target_pdf_render_size();
        self.pdf_in_flight_width = target_w;
        Task::perform(
            compiler::render_pdf_page_batch(pdf_path, missing, gen, target_w, target_h),
            Message::PdfPagesRendered,
        )
    }

    fn refresh_suggestions(&mut self) {
        match self.tab().doc.completion_context() {
            Some(CompletionContext::Citation { prefix }) => {
                if self.cached_bib_entries.is_empty() {
                    self.refresh_bib_cache();
                }
                self.active_prefix = Some(prefix.clone());
                self.suggestions = project::suggest_citations(&self.cached_bib_entries, &prefix);
            }
            Some(CompletionContext::Reference { prefix }) => {
                if prefix.is_empty() || self.cached_label_entries.is_empty() {
                    self.refresh_label_cache();
                }
                self.active_prefix = Some(prefix.clone());
                self.suggestions = project::suggest_labels(&self.cached_label_entries, &prefix);
            }
            Some(CompletionContext::Command { prefix }) => {
                self.active_prefix = Some(prefix.clone());
                self.suggestions = symbols::suggest_commands(&prefix)
                    .into_iter()
                    .map(|cmd| SuggestionItem {
                        insert_text: cmd.clone(),
                        display_label: cmd,
                        detail: None,
                        is_arg_completion: false,
                    })
                    .collect();
            }
            None => {
                self.active_prefix = None;
                self.suggestions.clear();
            }
        }
        self.selected_suggestion = 0;
    }

    fn ensure_cursor_visible(&mut self) -> Task<Message> {
        let font_size = self.effective_editor_font_size();
        let line_height = ui::editor_line_height(font_size);
        let char_w = ui::editor_char_width(font_size);

        let tab = self.tab();
        let (cursor_line, _) = tab.doc.content.cursor_position();
        let visual_col = tab.doc.cursor_visual_col();

        let cursor_top = ui::EDITOR_PADDING + (cursor_line as f32) * line_height;
        let cursor_bottom = cursor_top + line_height;
        let view_top = tab.editor_scroll_offset.y.max(0.0);
        let view_bottom = view_top + self.editor_viewport_height;

        let new_y = if cursor_top < view_top + ui::EDITOR_PADDING {
            (cursor_top - ui::EDITOR_PADDING).max(0.0).round()
        } else if cursor_bottom > view_bottom - ui::EDITOR_PADDING {
            (cursor_bottom - self.editor_viewport_height + ui::EDITOR_PADDING * 2.0)
                .max(0.0)
                .round()
        } else {
            view_top.round()
        };

        let cursor_x = ui::EDITOR_PADDING + (visual_col as f32) * char_w;
        let view_w = if self.editor_viewport_width > 50.0 {
            self.editor_viewport_width
        } else {
            ui::editor_code_viewport_width(self)
        };
        let view_left = tab.editor_scroll_offset.x.max(0.0);
        let view_right = view_left + view_w;
        let max_chars = tab.doc.max_line_chars().max(visual_col);
        let needed_w = ui::editor_needed_text_width(max_chars, font_size);
        let max_scroll_x = (needed_w - view_w).max(0.0);
        let right_margin = (char_w * 4.0).max(32.0);
        let left_margin = ui::EDITOR_PADDING + (char_w * 2.0);

        let new_x = if max_scroll_x <= 0.5 || visual_col == 0 {
            0.0
        } else if cursor_x > view_right - right_margin {
            (cursor_x - view_w + right_margin + char_w * 4.0)
                .clamp(0.0, max_scroll_x)
                .round()
        } else if cursor_x < view_left + left_margin {
            (cursor_x - left_margin - char_w * 4.0)
                .clamp(0.0, max_scroll_x)
                .round()
        } else {
            view_left.clamp(0.0, max_scroll_x).round()
        };

        if (new_x - view_left).abs() >= 1.0 || (new_y - view_top).abs() >= 1.0 {
            let offset = scrollable::AbsoluteOffset { x: new_x, y: new_y };
            self.tab_mut().editor_scroll_offset = offset;
            scrollable::scroll_to(self.editor_scroll_id.clone(), offset)
        } else {
            Task::none()
        }
    }

    fn apply_drag_selection_at(&mut self, local: Point, viewport: Size) {
        let offset = self.tab().editor_scroll_offset;
        let content_x = (offset.x + local.x.clamp(0.0, viewport.width + 24.0)
            - ui::EDITOR_PADDING)
            .max(0.0);
        let content_y =
            (offset.y + local.y.clamp(0.0, viewport.height) - ui::EDITOR_PADDING).max(0.0);
        self.tab_mut()
            .doc
            .content
            .perform(text_editor::Action::Drag(Point::new(content_x, content_y)));
    }

    fn step_editor_drag_autoscroll(&mut self) -> Task<Message> {
        if !self.editor_dragging {
            return Task::none();
        }
        let Some(local) = self.editor_drag_local else {
            return Task::none();
        };

        let vw = if self.editor_viewport_width > 50.0 {
            self.editor_viewport_width
        } else {
            ui::editor_code_viewport_width(self)
        };
        let vh = self.editor_viewport_height.max(100.0);

        let (dx, dy) = compute_drag_scroll_step(local, Size::new(vw, vh));
        if dx == 0.0 && dy == 0.0 {
            return Task::none();
        }

        let font_size = self.effective_editor_font_size();
        let max_chars = self.tab().doc.max_line_chars();
        let line_count = self.tab().doc.line_count();
        let max_scroll_x = (ui::editor_needed_text_width(max_chars, font_size) - vw).max(0.0);
        let max_scroll_y = (ui::editor_total_content_height(line_count, font_size) - vh).max(0.0);

        let old = self.tab().editor_scroll_offset;
        let new_x = (old.x + dx).clamp(0.0, max_scroll_x).round();
        let new_y = (old.y + dy).clamp(0.0, max_scroll_y).round();

        if (new_x - old.x).abs() < 1.0 && (new_y - old.y).abs() < 1.0 {
            return Task::none();
        }

        let offset = scrollable::AbsoluteOffset { x: new_x, y: new_y };
        self.tab_mut().editor_scroll_offset = offset;
        self.apply_drag_selection_at(local, Size::new(vw, vh));
        scrollable::scroll_to(self.editor_scroll_id.clone(), offset)
    }

    fn subscription(&self) -> Subscription<Message> {
        let keys = event::listen_with(|ev, status, _window| {
            if let Event::Window(window::Event::FileDropped(path)) = ev {
                return Some(Message::FileDropped(path));
            }
            if status != event::Status::Ignored {
                return None;
            }
            let Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modified_key,
                physical_key,
                modifiers,
                ..
            }) = ev
            else {
                return None;
            };

            if modifiers.is_empty()
                && matches!(key.as_ref(), keyboard::Key::Named(keyboard::key::Named::Escape))
            {
                return Some(Message::DismissOverlays);
            }
            if !modifiers.command() || modifiers.alt() {
                return None;
            }

            if matches!(key.as_ref(), keyboard::Key::Character("+" | "="))
                || matches!(modified_key.as_ref(), keyboard::Key::Character("+" | "="))
                || physical_key == keyboard::key::Physical::Code(keyboard::key::Code::NumpadAdd)
            {
                return Some(Message::ZoomInEditor);
            }
            if matches!(key.as_ref(), keyboard::Key::Character("-" | "_"))
                || matches!(modified_key.as_ref(), keyboard::Key::Character("-" | "_"))
                || physical_key == keyboard::key::Physical::Code(keyboard::key::Code::NumpadSubtract)
            {
                return Some(Message::ZoomOutEditor);
            }
            if matches!(key.as_ref(), keyboard::Key::Character("0"))
                || matches!(modified_key.as_ref(), keyboard::Key::Character("0"))
                || physical_key == keyboard::key::Physical::Code(keyboard::key::Code::Numpad0)
            {
                return Some(Message::ResetZoomEditor);
            }

            if matches!(key.as_ref(), keyboard::Key::Character("/"))
                || matches!(modified_key.as_ref(), keyboard::Key::Character("/"))
                || physical_key
                    == keyboard::key::Physical::Code(keyboard::key::Code::Slash)
                || physical_key
                    == keyboard::key::Physical::Code(keyboard::key::Code::NumpadDivide)
                || (matches!(key.as_ref(), keyboard::Key::Character("7")) && modifiers.shift())
            {
                return Some(Message::ToggleComment);
            }

            match key.as_ref() {
                keyboard::Key::Character("n" | "N") => Some(Message::New),
                keyboard::Key::Character("o" | "O") => Some(Message::Open),
                keyboard::Key::Character("s" | "S") => Some(Message::Save),
                keyboard::Key::Character("w" | "W") => Some(Message::CloseActiveTab),
                keyboard::Key::Character("b" | "B") => Some(Message::Compile),
                keyboard::Key::Character("f" | "F") => Some(Message::OpenFindBar { replace: false }),
                keyboard::Key::Character("h" | "H") => Some(Message::OpenFindBar { replace: true }),
                keyboard::Key::Character("g" | "G") => Some(Message::OpenGotoLine),
                keyboard::Key::Named(keyboard::key::Named::Tab) => {
                    if modifiers.shift() {
                        Some(Message::PrevTab)
                    } else {
                        Some(Message::NextTab)
                    }
                }
                keyboard::Key::Named(keyboard::key::Named::PageDown) => Some(Message::NextTab),
                keyboard::Key::Named(keyboard::key::Named::PageUp) => Some(Message::PrevTab),
                _ => None,
            }
        });

        let opens = window::open_events().map(Message::WindowOpened);
        let resizes = window::resize_events().map(|(id, size)| Message::WindowResized(id, size));
        let closes = window::close_requests().map(Message::CloseRequested);

        if self.editor_dragging {
            let drag_ticks = iced::time::every(std::time::Duration::from_millis(25))
                .map(|_| Message::EditorDragTick);
            Subscription::batch([keys, opens, resizes, closes, drag_ticks])
        } else {
            Subscription::batch([keys, opens, resizes, closes])
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::InstalledPackagesDetected(pkgs) => {
                self.installed_tex_packages = pkgs;
                Task::none()
            }
            Message::GoHome => {
                self.screen = Screen::Welcome;
                self.suggestions.clear();
                self.selected_suggestion = 0;
                self.show_folder_menu = false;
                self.show_recipe_menu = false;
                self.pdf_context_menu = None;
                self.editor_dragging = false;
                self.editor_drag_local = None;
                self.status = "Pronto".to_string();
                Task::none()
            }
            Message::SelectTab(index) => {
                if index < self.tabs.len() && index != self.active_tab {
                    self.status = format!("Passato a {}", self.tabs[index].doc.file_name());
                    return self.switch_to_tab(index);
                }
                self.show_folder_menu = false;
                Task::none()
            }
            Message::NextTab => {
                if self.screen == Screen::Editor && self.tabs.len() > 1 {
                    let next = (self.active_tab + 1) % self.tabs.len();
                    return self.switch_to_tab(next);
                }
                Task::none()
            }
            Message::PrevTab => {
                if self.screen == Screen::Editor && self.tabs.len() > 1 {
                    let prev = if self.active_tab == 0 {
                        self.tabs.len() - 1
                    } else {
                        self.active_tab - 1
                    };
                    return self.switch_to_tab(prev);
                }
                Task::none()
            }
            Message::CloseActiveTab => {
                if self.screen == Screen::Editor {
                    return self.update(Message::CloseTab(self.active_tab));
                }
                Task::none()
            }
            Message::CloseTab(index) => {
                if index >= self.tabs.len() {
                    return Task::none();
                }
                // Auto-save named dirty file before closing its tab so edits are never lost.
                if self.tabs[index].doc.is_dirty {
                    if let Some(path) = self.tabs[index].doc.path.clone() {
                        let _ = std::fs::write(&path, self.tabs[index].doc.text());
                        self.tabs[index].doc.is_dirty = false;
                    }
                }
                let closed_name = self.tabs[index].doc.file_name();
                self.editor_dragging = false;
                self.editor_drag_local = None;
                self.tabs.remove(index);
                if self.tabs.is_empty() {
                    let id = self.alloc_tab_id();
                    self.tabs.push(TabState::new(id));
                    self.active_tab = 0;
                    self.cancel_live_compile();
                    self.reset_gpu_pdf_cache();
                    self.show_pdf = false;
                    self.show_log = false;
                    self.show_folder_menu = false;
                    self.suggestions.clear();
                    self.selected_suggestion = 0;
                    self.active_prefix = None;
                    self.screen = Screen::Welcome;
                    self.status = format!("Chiuso {closed_name}");
                    return Task::none();
                } else if self.active_tab >= self.tabs.len() {
                    self.active_tab = self.tabs.len() - 1;
                } else if index < self.active_tab {
                    self.active_tab -= 1;
                }
                self.status = format!("Chiuso {closed_name}");
                let target = self.active_tab;
                self.switch_to_tab(target)
            }
            Message::ToggleFolderMenu => {
                self.pdf_context_menu = None;
                self.show_recipe_menu = false;
                self.show_folder_menu = !self.show_folder_menu;
                if self.show_folder_menu {
                    self.refresh_folder_files();
                }
                Task::none()
            }
            Message::ToggleRecipeMenu => {
                self.pdf_context_menu = None;
                self.show_folder_menu = false;
                self.show_recipe_menu = !self.show_recipe_menu;
                Task::none()
            }
            Message::SelectRecipe(id) => {
                self.recipe_config.active_recipe_id = id;
                recipes::save_recipe_config(&self.recipe_config);
                let name = self.recipe_config.active_recipe().name;
                self.show_recipe_menu = false;
                self.status = format!("Ricetta attiva: {name}");
                if self.can_use_fast_live_pdflatex() {
                    return self.schedule_live_compile(150, false);
                } else {
                    self.cancel_live_compile();
                    self.tab_mut().staged_build = None;
                }
                Task::none()
            }
            Message::ToggleShellEscape => {
                self.recipe_config.shell_escape = !self.recipe_config.shell_escape;
                recipes::save_recipe_config(&self.recipe_config);
                self.status = if self.recipe_config.shell_escape {
                    "--shell-escape attivato per la compilazione".to_string()
                } else {
                    "--shell-escape disattivato".to_string()
                };
                if !self.can_use_fast_live_pdflatex() {
                    self.cancel_live_compile();
                    self.tab_mut().staged_build = None;
                }
                Task::none()
            }
            Message::ToggleNewRecipeBuilder => {
                self.show_new_recipe_builder = !self.show_new_recipe_builder;
                Task::none()
            }
            Message::NewRecipeNameChanged(name) => {
                self.new_recipe_name = name;
                Task::none()
            }
            Message::NewRecipeAddStep(step) => {
                if self.new_recipe_steps.len() < 8 {
                    self.new_recipe_steps.push(step);
                }
                Task::none()
            }
            Message::NewRecipePopStep => {
                self.new_recipe_steps.pop();
                Task::none()
            }
            Message::NewRecipeClearSteps => {
                self.new_recipe_steps.clear();
                Task::none()
            }
            Message::SaveCustomRecipe => {
                let name = std::mem::take(&mut self.new_recipe_name);
                let steps = std::mem::take(&mut self.new_recipe_steps);
                if self.recipe_config.add_custom_recipe(name, steps).is_some() {
                    recipes::save_recipe_config(&self.recipe_config);
                    let saved_name = self.recipe_config.active_recipe().name;
                    self.show_new_recipe_builder = false;
                    self.status = format!("Ricetta personalizzata '{saved_name}' salvata");
                    if !self.can_use_fast_live_pdflatex() {
                        self.cancel_live_compile();
                        self.tab_mut().staged_build = None;
                    }
                } else {
                    self.status = "Inserisci un nome e almeno un passo".to_string();
                }
                Task::none()
            }
            Message::DeleteCustomRecipe(id) => {
                if self.recipe_config.remove_custom_recipe(&id) {
                    recipes::save_recipe_config(&self.recipe_config);
                    self.status = "Ricetta personalizzata rimossa".to_string();
                }
                Task::none()
            }
            Message::DismissOverlays => {
                if self.pdf_context_menu.is_some()
                    || self.show_folder_menu
                    || self.show_recipe_menu
                {
                    self.pdf_context_menu = None;
                    self.show_folder_menu = false;
                    self.show_recipe_menu = false;
                } else if self.show_goto_bar {
                    self.show_goto_bar = false;
                } else if self.show_find_bar {
                    self.show_find_bar = false;
                }
                self.editor_dragging = false;
                self.editor_drag_local = None;
                Task::none()
            }
            Message::ToggleLog => {
                self.pdf_context_menu = None;
                self.show_log = !self.show_log;
                if self.show_log {
                    self.show_pdf = false;
                }
                self.editor_viewport_width = 0.0;
                Task::none()
            }
            Message::SelectLogTab(lt) => {
                self.log_tab = lt;
                self.show_log = true;
                self.show_pdf = false;
                if lt == LogTab::Linter && self.tab().lint_warnings.is_empty() && !self.is_linting {
                    return self.spawn_chktex_task();
                }
                Task::none()
            }
            Message::ToggleRawLog => {
                let tab = self.tab_mut();
                tab.show_raw_log = !tab.show_raw_log;
                Task::none()
            }
            Message::TogglePdf => {
                self.pdf_context_menu = None;
                self.show_pdf = !self.show_pdf;
                self.editor_viewport_width = 0.0;
                if self.show_pdf {
                    self.show_log = false;
                    if self.pdf_cache_width != 0
                        && self.pdf_cache_width != self.target_pdf_render_width()
                    {
                        self.reset_gpu_pdf_cache();
                    }
                    let mut pdf_off = self.tab().pdf_scroll_offset;
                    if self.pdf_zoom <= 1.001 {
                        pdf_off.x = 0.0;
                        self.tab_mut().pdf_scroll_offset.x = 0.0;
                    }
                    return Task::batch([
                        scrollable::scroll_to(self.pdf_scroll_id.clone(), pdf_off),
                        self.request_visible_pdf_pages(),
                    ]);
                }
                Task::none()
            }
            Message::ToggleSymbols => {
                let old_fit = ui::pdf_fit_base_width(self);
                self.show_symbols = !self.show_symbols;
                if self.show_symbols {
                    self.show_match = false;
                    self.show_outline = false;
                    self.show_assets = false;
                }
                self.editor_viewport_width = 0.0;
                self.handle_pdf_width_change(old_fit, false)
            }
            Message::ToggleMatch => {
                let old_fit = ui::pdf_fit_base_width(self);
                self.show_match = !self.show_match;
                if self.show_match {
                    self.show_symbols = false;
                    self.show_outline = false;
                    self.show_assets = false;
                }
                self.editor_viewport_width = 0.0;
                self.handle_pdf_width_change(old_fit, false)
            }
            Message::ToggleOutline => {
                let old_fit = ui::pdf_fit_base_width(self);
                self.show_outline = !self.show_outline;
                if self.show_outline {
                    self.show_symbols = false;
                    self.show_match = false;
                    self.show_assets = false;
                    self.refresh_outline();
                }
                self.editor_viewport_width = 0.0;
                self.handle_pdf_width_change(old_fit, false)
            }
            Message::ToggleAssets => {
                let old_fit = ui::pdf_fit_base_width(self);
                self.show_assets = !self.show_assets;
                if self.show_assets {
                    self.show_symbols = false;
                    self.show_match = false;
                    self.show_outline = false;
                }
                self.editor_viewport_width = 0.0;
                self.handle_pdf_width_change(old_fit, false)
            }
            Message::SetFigureWidth(w) => {
                self.figure_width_ratio = w.clamp(0.2, 1.0);
                Task::none()
            }
            Message::PickFigureImage => {
                Task::perform(project::pick_image_file(), Message::FigureImagePicked)
            }
            Message::FigureImagePicked(Some(path)) => {
                self.screen = Screen::Editor;
                self.insert_image_asset(path)
            }
            Message::FigureImagePicked(None) => Task::none(),
            Message::FileDropped(path) => {
                if project::is_supported_image_file(&path) {
                    self.screen = Screen::Editor;
                    return self.insert_image_asset(path);
                }
                let is_tex_like = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|ext| {
                        matches!(ext.to_ascii_lowercase().as_str(), "tex" | "bib" | "sty" | "cls")
                    });
                if is_tex_like {
                    return self.update(Message::OpenRecent(path));
                }
                self.status = "Formato file trascinato non supportato".to_string();
                Task::none()
            }
            Message::SetTableRows(r) => {
                self.table_rows = r.clamp(1, 20);
                Task::none()
            }
            Message::SetTableCols(c) => {
                self.table_cols = c.clamp(1, 12);
                Task::none()
            }
            Message::SetTableAlign(a) => {
                self.table_align = a;
                Task::none()
            }
            Message::ToggleTableBooktabs => {
                self.table_booktabs = !self.table_booktabs;
                Task::none()
            }
            Message::ToggleTableHeader => {
                self.table_header = !self.table_header;
                Task::none()
            }
            Message::ToggleTableCaptionEnv => {
                self.table_caption_env = !self.table_caption_env;
                Task::none()
            }
            Message::InsertGeneratedTable => {
                let snippet = project::generate_latex_table(
                    self.table_rows,
                    self.table_cols,
                    self.table_align,
                    self.table_booktabs,
                    self.table_header,
                    self.table_caption_env,
                );
                let added_booktabs = if self.table_booktabs {
                    self.tab_mut().doc.add_package("booktabs")
                } else {
                    false
                };
                self.tab_mut().doc.insert_snippet(&snippet);
                let cur_l = self.tab().doc.content.cursor_position().0 + 1;
                self.tab_mut().last_edited_line = Some(cur_l);
                self.tab_mut().sync_highlight_line = None;
                self.refresh_outline();
                self.refresh_label_cache();
                self.refresh_suggestions();
                self.status = if added_booktabs {
                    format!(
                        "Tabella {}×{} inserita (+ \\usepackage{{booktabs}})",
                        self.table_rows, self.table_cols
                    )
                } else {
                    format!("Tabella {}×{} inserita", self.table_rows, self.table_cols)
                };
                let scroll_task = self.ensure_cursor_visible();
                let live_task = self.schedule_live_compile(650, false);
                Task::batch([scroll_task, live_task])
            }
            Message::BibImportInputChanged(val) => {
                self.bib_import_input = val;
                Task::none()
            }
            Message::FetchBibFromIdentifier => {
                if self.is_fetching_bib {
                    return Task::none();
                }
                let raw = self.bib_import_input.trim().to_string();
                if raw.is_empty() {
                    self.status = "Inserisci un DOI o un arXiv ID".to_string();
                    return Task::none();
                }
                self.is_fetching_bib = true;
                self.status = format!("Ricerca voce BibTeX per '{raw}'...");
                Task::perform(project::fetch_bibtex_entry(raw), Message::BibEntryFetched)
            }
            Message::BibEntryFetched(Ok((bib_raw, entry))) => {
                self.is_fetching_bib = false;
                let Some(proj_dir) = self.project_search_root_dir() else {
                    // Unsaved document: copy BibTeX to clipboard and notify
                    self.internal_clipboard = Some(bib_raw.clone());
                    write_wl_clipboard_bg(bib_raw.clone());
                    self.last_imported_bib = Some(entry.clone());
                    self.status = format!(
                        "Voce '{}' scaricata e copiata negli appunti (salva prima il .tex per creare references.bib)",
                        entry.key
                    );
                    return clipboard::write(bib_raw);
                };
                let active_tex = self.tab().doc.text();
                match project::append_bibtex_to_project(&proj_dir, &active_tex, &bib_raw, &entry.key)
                {
                    Ok((bib_path, was_added)) => {
                        // Sync open `.bib` tab if currently open in FeatherTex
                        if let Some(bib_tab_idx) = self.find_tab_by_path(&bib_path) {
                            if let Ok(disk_txt) = std::fs::read_to_string(&bib_path) {
                                self.tabs[bib_tab_idx].doc.load_text(bib_path.clone(), disk_txt);
                            }
                        }
                        self.bib_import_input.clear();
                        self.last_imported_bib = Some(entry.clone());
                        self.refresh_folder_files();
                        self.refresh_bib_cache();
                        let bib_name = bib_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("references.bib");
                        self.status = if was_added {
                            format!("Aggiunta '{}' in {bib_name}", entry.key)
                        } else {
                            format!("La chiave '{}' era già presente in {bib_name}", entry.key)
                        };
                    }
                    Err(err) => {
                        self.status = err;
                    }
                }
                Task::none()
            }
            Message::BibEntryFetched(Err(err)) => {
                self.is_fetching_bib = false;
                self.status = err;
                Task::none()
            }
            Message::InsertCitationKey(key) => {
                let cite_str = format!("\\cite{{{key}}}");
                self.tab_mut().doc.insert_snippet(&cite_str);
                let cur_l = self.tab().doc.content.cursor_position().0 + 1;
                self.tab_mut().last_edited_line = Some(cur_l);
                self.refresh_suggestions();
                self.status = format!("Inserito {cite_str}");
                let scroll_task = self.ensure_cursor_visible();
                let live_task = self.schedule_live_compile(650, false);
                Task::batch([scroll_task, live_task])
            }
            Message::RefreshWordCount => {
                if let Some(wc) = self.tab().word_count {
                    self.status = format!(
                        "Conteggio texcount: {} parole ({} testo + {} titoli + {} didascalie · {} math inline, {} display)",
                        wc.total_words(),
                        wc.body_words,
                        wc.header_words,
                        wc.caption_words,
                        wc.inline_math,
                        wc.display_math
                    );
                } else {
                    self.status = "Calcolo conteggio parole (texcount)...".to_string();
                }
                self.spawn_word_count_task()
            }
            Message::WordCountUpdated(tab_id, stats) => {
                if let Some(idx) = self.find_tab_by_id(tab_id) {
                    self.tabs[idx].word_count = stats;
                }
                Task::none()
            }
            Message::RunChktex => self.spawn_chktex_task(),
            Message::ChktexFinished(tab_id, res) => {
                self.is_linting = false;
                if let Some(idx) = self.find_tab_by_id(tab_id) {
                    match res {
                        Ok(warnings) => {
                            let n = warnings.len();
                            self.tabs[idx].lint_warnings = warnings;
                            if self.show_log && self.log_tab == LogTab::Linter {
                                self.status = if n == 0 {
                                    "Linter chktex: nessun avviso tipografico".to_string()
                                } else {
                                    format!("Linter chktex: {n} suggerimenti tipografici")
                                };
                            }
                        }
                        Err(err) => {
                            self.status = err;
                        }
                    }
                }
                Task::none()
            }
            Message::SetSpellLang(lang) => {
                self.spell_lang = lang;
                self.persist_user_settings();
                if self.show_log && self.log_tab == LogTab::Spellcheck {
                    return self.update(Message::RunSpellcheck);
                }
                Task::none()
            }
            Message::RunSpellcheck => {
                if self.is_spellchecking {
                    return Task::none();
                }
                self.is_spellchecking = true;
                self.show_log = true;
                self.show_pdf = false;
                self.log_tab = LogTab::Spellcheck;
                let tab_id = self.tab().id;
                let source = self.tab().doc.text();
                let lang = self.spell_lang.clone();
                self.status = format!("Controllo ortografico ({lang})...");
                Task::perform(quality::run_hunspell(source, lang), move |out| {
                    Message::SpellcheckFinished(tab_id, out)
                })
            }
            Message::SpellcheckFinished(tab_id, outcome) => {
                self.is_spellchecking = false;
                if let Some(idx) = self.find_tab_by_id(tab_id) {
                    match &outcome {
                        SpellCheckOutcome::Ok(list) => {
                            self.status = if list.is_empty() {
                                "Controllo ortografico: nessun errore rilevato".to_string()
                            } else {
                                format!("Controllo ortografico: {} parole segnalate", list.len())
                            };
                        }
                        SpellCheckOutcome::MissingDictionary { lang, .. } => {
                            self.status = format!("Dizionario hunspell '{lang}' mancante");
                        }
                        SpellCheckOutcome::Error(err) => {
                            self.status = err.clone();
                        }
                    }
                    self.tabs[idx].spell_outcome = Some(outcome);
                }
                Task::none()
            }
            Message::RunLatexdiffGit => {
                if self.is_compiling {
                    return Task::none();
                }
                let Some(cur_path) = self
                    .effective_master_path()
                    .or_else(|| self.tab().doc.path.clone())
                else {
                    self.status = "Salva prima il documento .tex per usare latexdiff".to_string();
                    return Task::none();
                };
                if self.tab().doc.is_dirty {
                    if let Some(p) = self.tab().doc.path.as_ref() {
                        let _ = std::fs::write(p, self.tab().doc.text());
                        self.tab_mut().doc.is_dirty = false;
                    }
                }
                let tab_id = self.tab().id;
                self.is_compiling = true;
                self.status = "Generazione differenze rispetto a Git HEAD (latexdiff)...".to_string();
                Task::perform(
                    quality::generate_latexdiff_against_git_head(cur_path),
                    move |res| Message::LatexdiffTexReady(tab_id, res),
                )
            }
            Message::RunLatexdiffFile => {
                if self.is_compiling {
                    return Task::none();
                }
                let Some(cur_path) = self
                    .effective_master_path()
                    .or_else(|| self.tab().doc.path.clone())
                else {
                    self.status = "Salva prima il documento .tex per usare latexdiff".to_string();
                    return Task::none();
                };
                if self.tab().doc.is_dirty {
                    if let Some(p) = self.tab().doc.path.as_ref() {
                        let _ = std::fs::write(p, self.tab().doc.text());
                        self.tab_mut().doc.is_dirty = false;
                    }
                }
                let tab_id = self.tab().id;
                self.status = "Seleziona la versione precedente .tex...".to_string();
                Task::perform(quality::pick_old_tex_and_latexdiff(cur_path), move |res| {
                    Message::LatexdiffTexReady(tab_id, res)
                })
            }
            Message::LatexdiffTexReady(tab_id, Ok(diff_tex_path)) => {
                self.is_compiling = true;
                self.status = "Compilazione PDF delle modifiche (latexdiff)...".to_string();
                let target_w = self.target_pdf_render_width();
                let default_recipe = recipes::builtin_recipes().into_iter().next().unwrap();
                Task::perform(
                    compiler::compile_with_recipe(
                        diff_tex_path,
                        None,
                        default_recipe,
                        false,
                        1,
                        1,
                        target_w,
                    ),
                    move |res| Message::CompileFinished(tab_id, res),
                )
            }
            Message::LatexdiffTexReady(_, Err(err)) => {
                self.is_compiling = false;
                self.status = err;
                Task::none()
            }
            Message::SetOutlineFilter(filter) => {
                self.outline_filter = filter;
                Task::none()
            }
            Message::TogglePinMaster => {
                if self.is_active_tab_pinned_master() {
                    self.master_tex_path = None;
                    self.status = "File Master rimosso".to_string();
                } else if let Some(path) = self.tab().doc.path.clone() {
                    let name = self.tab().doc.file_name();
                    self.master_tex_path = Some(path);
                    self.status = format!("File Master impostato: {name}");
                } else {
                    self.status = "Salva prima il file per impostarlo come Master".to_string();
                    return Task::none();
                }
                self.refresh_outline();
                if !self.can_use_fast_live_pdflatex() {
                    self.cancel_live_compile();
                    self.tab_mut().staged_build = None;
                }
                Task::none()
            }
            Message::ClearMaster => {
                self.master_tex_path = None;
                self.refresh_outline();
                self.status = "File Master rimosso".to_string();
                Task::none()
            }
            Message::OutlineJump {
                file_path,
                line_1based,
            } => {
                let target_line = line_1based.max(1);
                let mut switch_task = Task::none();
                if let Some(target_path) = file_path {
                    if let Some(existing_idx) = self.find_tab_by_path(&target_path) {
                        if existing_idx != self.active_tab {
                            switch_task = self.switch_to_tab(existing_idx);
                        }
                    } else if target_path.is_file() {
                        if let Ok(bytes) = std::fs::read(&target_path) {
                            let content = String::from_utf8_lossy(&bytes).into_owned();
                            recent::record_recent(&mut self.recent_files, target_path.clone());
                            let id = self.alloc_tab_id();
                            let mut new_tab = TabState::new(id);
                            new_tab.doc.load_text(target_path, content);
                            self.tabs.push(new_tab);
                            let new_idx = self.tabs.len() - 1;
                            switch_task = self.switch_to_tab(new_idx);
                        }
                    }
                }
                let file_name = self.tab().doc.file_name();
                self.tab_mut().doc.jump_to_line(target_line);
                self.tab_mut().sync_highlight_line = Some(target_line.saturating_sub(1));
                self.refresh_suggestions();
                self.status = format!("{file_name} · riga {target_line}");
                let scroll_task = self.ensure_line_visible(target_line.saturating_sub(1));
                Task::batch([switch_task, scroll_task])
            }
            Message::OpenFindBar { replace } => {
                if self.screen != Screen::Editor {
                    return Task::none();
                }
                self.show_goto_bar = false;
                self.show_find_bar = true;
                if replace {
                    self.show_replace_row = true;
                }
                if let Some(sel) = self
                    .tab()
                    .doc
                    .content
                    .selection()
                    .filter(|s| !s.is_empty() && !s.contains('\n'))
                {
                    if !self.find_use_regex {
                        self.find_query = sel;
                    }
                }
                let find_task = self.refresh_find_state(false);
                let focus_task = text_input::focus(self.find_input_id.clone());
                let sel_task = text_input::select_all(self.find_input_id.clone());
                Task::batch([find_task, focus_task, sel_task])
            }
            Message::CloseFindBar => {
                self.show_find_bar = false;
                Task::none()
            }
            Message::ToggleReplaceRow => {
                self.show_replace_row = !self.show_replace_row;
                if self.show_replace_row {
                    return text_input::focus(self.replace_input_id.clone());
                }
                Task::none()
            }
            Message::FindQueryChanged(q) => {
                self.find_query = q;
                self.refresh_find_state(true)
            }
            Message::ReplaceQueryChanged(r) => {
                self.replace_query = r;
                Task::none()
            }
            Message::ToggleFindCase => {
                self.find_case_sensitive = !self.find_case_sensitive;
                self.refresh_find_state(true)
            }
            Message::ToggleFindRegex => {
                self.find_use_regex = !self.find_use_regex;
                self.refresh_find_state(true)
            }
            Message::ToggleFindInFolder => {
                self.find_in_folder = !self.find_in_folder;
                self.refresh_find_state(false)
            }
            Message::FindNext => self.step_find_match(true),
            Message::FindPrev => self.step_find_match(false),
            Message::ReplaceCurrent => {
                if self.find_query.is_empty() {
                    return Task::none();
                }
                if self.find_matches.is_empty() {
                    let _ = self.refresh_find_state(false);
                }
                let Some(idx) = self.find_active_idx.filter(|&i| i < self.find_matches.len()) else {
                    self.status = "Nessuna corrispondenza da sostituire".to_string();
                    return Task::none();
                };
                let m = self.find_matches[idx].clone();
                let source = self.tab().doc.text();
                let expanded = project::expand_replacement(
                    &source,
                    &m,
                    &self.replace_query,
                    self.find_use_regex,
                );
                let line_0 = self
                    .tab_mut()
                    .doc
                    .replace_byte_range(m.start_byte, m.end_byte, &expanded);
                self.tab_mut().last_edited_line = Some(line_0 + 1);
                self.tab_mut().sync_highlight_line = None;
                self.refresh_outline();
                let find_task = self.refresh_find_state(true);
                let scroll_task = self.ensure_line_visible(line_0);
                let live_task = self.schedule_live_compile(650, false);
                self.status = "Sostituita 1 occorrenza".to_string();
                Task::batch([find_task, scroll_task, live_task])
            }
            Message::ReplaceAll => {
                if self.find_query.is_empty() {
                    return Task::none();
                }
                let opts = SearchOptions {
                    case_sensitive: self.find_case_sensitive,
                    use_regex: self.find_use_regex,
                };
                let source = self.tab().doc.text();
                match project::replace_all_matches(&source, &self.find_query, &self.replace_query, opts)
                {
                    Ok((updated, count)) if count > 0 => {
                        self.tab_mut().doc.replace_entire_text(updated);
                        let cur_l = self.tab().doc.content.cursor_position().0 + 1;
                        self.tab_mut().last_edited_line = Some(cur_l);
                        self.tab_mut().sync_highlight_line = None;
                        self.refresh_outline();
                        let _ = self.refresh_find_state(false);
                        self.status = format!("Sostituite {count} occorrenze");
                        self.schedule_live_compile(650, false)
                    }
                    Ok(_) => {
                        self.status = "Nessuna corrispondenza trovata".to_string();
                        Task::none()
                    }
                    Err(err) => {
                        self.find_error = Some(err);
                        Task::none()
                    }
                }
            }
            Message::OpenGotoLine => {
                if self.screen != Screen::Editor {
                    return Task::none();
                }
                self.show_find_bar = false;
                self.show_goto_bar = true;
                let cur_line = self.tab().doc.content.cursor_position().0 + 1;
                self.goto_line_input = cur_line.to_string();
                Task::batch([
                    text_input::focus(self.goto_input_id.clone()),
                    text_input::select_all(self.goto_input_id.clone()),
                ])
            }
            Message::CloseGotoLine => {
                self.show_goto_bar = false;
                Task::none()
            }
            Message::GotoLineChanged(val) => {
                self.goto_line_input = val.chars().filter(|c| c.is_ascii_digit()).collect();
                Task::none()
            }
            Message::GotoLineSubmit => {
                let Ok(line) = self.goto_line_input.trim().parse::<usize>() else {
                    self.status = "Inserisci un numero di riga valido".to_string();
                    return Task::none();
                };
                let clamped = line.clamp(1, self.tab().doc.line_count());
                self.show_goto_bar = false;
                self.tab_mut().doc.jump_to_line(clamped);
                self.tab_mut().sync_highlight_line = Some(clamped.saturating_sub(1));
                self.refresh_suggestions();
                self.status = format!("Riga {clamped}");
                self.ensure_line_visible(clamped.saturating_sub(1))
            }
            Message::ToggleComment => {
                if self.screen != Screen::Editor {
                    return Task::none();
                }
                let count = self.tab_mut().doc.toggle_comment();
                let cur_l = self.tab().doc.content.cursor_position().0 + 1;
                self.tab_mut().last_edited_line = Some(cur_l);
                self.tab_mut().sync_highlight_line = None;
                self.refresh_suggestions();
                self.refresh_outline();
                self.status = if count == 1 {
                    "Riga commentata/decommentata".to_string()
                } else {
                    format!("{count} righe commentate/decommentate")
                };
                self.schedule_live_compile(650, false)
            }
            Message::IndentSelection => {
                if self.screen != Screen::Editor {
                    return Task::none();
                }
                let changed = self.tab_mut().doc.indent_lines(false);
                if changed > 0 {
                    let cur_l = self.tab().doc.content.cursor_position().0 + 1;
                    self.tab_mut().last_edited_line = Some(cur_l);
                    self.refresh_suggestions();
                    let scroll_task = self.ensure_cursor_visible();
                    let live_task = self.schedule_live_compile(650, false);
                    return Task::batch([scroll_task, live_task]);
                }
                Task::none()
            }
            Message::UnindentSelection => {
                if self.screen != Screen::Editor {
                    return Task::none();
                }
                let changed = self.tab_mut().doc.indent_lines(true);
                if changed > 0 {
                    let cur_l = self.tab().doc.content.cursor_position().0 + 1;
                    self.tab_mut().last_edited_line = Some(cur_l);
                    self.refresh_suggestions();
                    let scroll_task = self.ensure_cursor_visible();
                    let live_task = self.schedule_live_compile(650, false);
                    return Task::batch([scroll_task, live_task]);
                }
                Task::none()
            }
            Message::ToggleTheme => {
                self.dark_mode = !self.dark_mode;
                self.persist_user_settings();
                Task::none()
            }
            Message::ToggleAutoClose => {
                self.auto_close = !self.auto_close;
                self.persist_user_settings();
                self.status = if self.auto_close {
                    "Auto-chiusura parentesi e ambienti attivata".to_string()
                } else {
                    "Auto-chiusura disattivata".to_string()
                };
                Task::none()
            }
            Message::ToggleLiveCompile => {
                self.live_compile = !self.live_compile;
                self.persist_user_settings();
                if self.live_compile {
                    if self.tab().doc.path.is_some() {
                        self.status = "Pre-compilazione in background attivata".to_string();
                        return self.schedule_live_compile(0, true);
                    } else {
                        self.status =
                            "Background attivo (salva il file per pre-compilare)".to_string();
                    }
                } else {
                    self.cancel_live_compile();
                    self.tab_mut().staged_build = None;
                    self.status = "Pre-compilazione in background disattivata".to_string();
                }
                Task::none()
            }
            Message::LiveCompileFinished(tab_id, rev, text_hash, Ok(result)) => {
                if rev != self.live_revision {
                    return Task::none();
                }
                self.is_live_compiling = false;
                self.live_task_handle = None;
                self.live_in_flight_hash = None;
                self.live_started_at = None;
                self.live_debounce_ms = 0;
                let promote_now = std::mem::take(&mut self.promote_live_on_finish);
                let requeue = std::mem::take(&mut self.live_pending_requeue);

                let Some(idx) = self.find_tab_by_id(tab_id) else {
                    if promote_now {
                        self.is_compiling = false;
                    }
                    return Task::none();
                };

                // Discard if the user edited the document again while this build finished.
                if compiler::text_fingerprint(&self.tabs[idx].doc.text()) != text_hash {
                    if promote_now {
                        // Buffer changed after pressing Compila; kick off a fresh compile now.
                        let path = self.tabs[idx].doc.path.clone();
                        let contents = self.tabs[idx].doc.text();
                        return Task::perform(document::save_file(path, contents), move |res| {
                            Message::CompileAfterSave(tab_id, res)
                        });
                    }
                    if requeue && idx == self.active_tab {
                        return self.schedule_live_compile(250, false);
                    }
                    return Task::none();
                }

                if promote_now {
                    if let Some(tex_path) = self.tabs[idx].doc.path.clone() {
                        return self.spawn_promote_task(tab_id, idx, tex_path, result, false);
                    }
                    self.is_compiling = false;
                    return Task::none();
                }

                // Stage silently in background WITHOUT altering the user's current PDF view or Log.
                self.tabs[idx].staged_build = Some(StagedBuild { text_hash, result });
                if requeue && idx == self.active_tab {
                    return self.schedule_live_compile(250, false);
                }
                Task::none()
            }
            Message::LiveCompileFinished(_, rev, _, Err(err)) => {
                if rev == self.live_revision {
                    self.is_live_compiling = false;
                    self.live_task_handle = None;
                    self.live_in_flight_hash = None;
                    self.live_started_at = None;
                    self.live_debounce_ms = 0;
                    let requeue = std::mem::take(&mut self.live_pending_requeue);
                    if std::mem::take(&mut self.promote_live_on_finish) {
                        self.is_compiling = false;
                        self.status = err;
                    } else if requeue {
                        return self.schedule_live_compile(250, false);
                    }
                }
                Task::none()
            }
            Message::StagedPromoteFinished(tab_id, instant_swap, Ok(promoted)) => {
                self.is_compiling = false;
                let Some(idx) = self.find_tab_by_id(tab_id) else {
                    return Task::none();
                };
                let ui_task = self.apply_compile_result(idx, promoted, instant_swap);
                let wc_task = self.spawn_word_count_task();
                let lint_task = self.spawn_chktex_task();
                Task::batch([ui_task, wc_task, lint_task])
            }
            Message::StagedPromoteFinished(_, _, Err(err)) => {
                self.is_compiling = false;
                self.status = err;
                Task::none()
            }
            Message::OpenPdfExternal => {
                self.pdf_context_menu = None;
                if let Some(path) = &self.tab().last_pdf {
                    compiler::open_pdf_external(path);
                }
                Task::none()
            }
            Message::SyncPdfToLastEdit => {
                let tab = self.tab();
                let (Some(tex_path), Some(pdf_path)) = (tab.doc.path.clone(), tab.last_pdf.clone())
                else {
                    return Task::none();
                };
                let sync_line = tab
                    .last_edited_line
                    .unwrap_or_else(|| tab.doc.content.cursor_position().0 + 1);
                let height_pts = tab.pdf_page_height_pts;
                let total_pages = tab.pdf_total_pages;
                Task::perform(
                    async move {
                        compiler::synctex_forward_search(
                            &tex_path,
                            &pdf_path,
                            sync_line,
                            height_pts,
                            total_pages,
                        )
                        .await
                    },
                    Message::PdfSynced,
                )
            }
            Message::PdfSynced(Some(target)) => {
                self.status = format!("PDF sincronizzato alla pag. {}", target.page);
                self.scroll_pdf_to_target(target)
            }
            Message::PdfSynced(None) => {
                self.status = "Sincronizzazione PDF non disponibile per questa riga".to_string();
                Task::none()
            }
            Message::PdfLeftClicked {
                page,
                x_ratio,
                y_ratio,
            } => {
                self.pdf_context_menu = None;
                self.show_folder_menu = false;
                self.run_reverse_sync(page, x_ratio, y_ratio)
            }
            Message::PdfRightClicked {
                page,
                x_ratio,
                y_ratio,
                anchor,
            } => {
                self.show_folder_menu = false;
                self.pdf_context_menu = Some(PdfContextMenu {
                    page,
                    x_ratio,
                    y_ratio,
                    anchor,
                });
                Task::none()
            }
            Message::PdfViewSource => {
                let Some(menu) = self.pdf_context_menu.take() else {
                    return Task::none();
                };
                self.run_reverse_sync(menu.page, menu.x_ratio, menu.y_ratio)
            }
            Message::ReverseSyncResolved(Some(res)) => {
                let target_line = res.line.max(1);
                let mut switch_task = Task::none();

                if let Some(existing_idx) = self.find_tab_by_path(&res.file_path) {
                    if existing_idx != self.active_tab {
                        switch_task = self.switch_to_tab(existing_idx);
                    }
                } else if res.file_path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&res.file_path) {
                        recent::record_recent(&mut self.recent_files, res.file_path.clone());
                        let id = self.alloc_tab_id();
                        let mut new_tab = TabState::new(id);
                        new_tab.doc.load_text(res.file_path.clone(), content);
                        self.tabs.push(new_tab);
                        let new_idx = self.tabs.len() - 1;
                        switch_task = self.switch_to_tab(new_idx);
                    }
                }

                let file_name = self.tab().doc.file_name();
                self.tab_mut().doc.jump_to_line(target_line);
                self.tab_mut().sync_highlight_line = Some(target_line.saturating_sub(1));
                self.refresh_suggestions();
                self.status = format!("Codice sorgente · {file_name}, riga {target_line}");
                let scroll_task = self.ensure_line_visible(target_line.saturating_sub(1));
                Task::batch([switch_task, scroll_task])
            }
            Message::ReverseSyncResolved(None) => {
                self.status =
                    "Posizione sorgente non trovata (ricompila il PDF per aggiornare SyncTeX)"
                        .to_string();
                Task::none()
            }
            Message::JumpToLine(line_1based) => {
                self.tab_mut().doc.jump_to_line(line_1based);
                self.tab_mut().sync_highlight_line = None;
                self.refresh_suggestions();
                self.status = format!("Riga {line_1based}");
                self.ensure_line_visible(line_1based.saturating_sub(1))
            }
            Message::MatchStrokeCompleted(stroke) => {
                self.match_strokes.push(stroke);
                self.match_results = matcher::recognize(&self.match_strokes, 10);
                Task::none()
            }
            Message::MatchUndo => {
                self.match_strokes.pop();
                self.match_results = matcher::recognize(&self.match_strokes, 10);
                Task::none()
            }
            Message::MatchClear => {
                self.match_strokes.clear();
                self.match_results.clear();
                Task::none()
            }
            Message::MatchInsert(idx) => {
                let Some(cand) = self.match_results.get(idx).cloned() else {
                    return Task::none();
                };
                matcher::learn(&self.match_strokes, cand.display_cmd);
                self.match_strokes.clear();
                self.match_results.clear();

                let tab = self.tab_mut();
                tab.doc.insert_snippet(cand.insert);
                tab.last_edited_line = Some(tab.doc.content.cursor_position().0 + 1);
                tab.sync_highlight_line = None;
                let has_pkg = cand.package.is_none_or(|pkg| tab.doc.has_package(pkg));
                self.refresh_suggestions();
                self.status = match cand.package {
                    Some(pkg) if !has_pkg => format!(
                        "Inserito {} (richiede \\usepackage{{{pkg}}})",
                        cand.display_cmd
                    ),
                    _ => format!("Inserito {}", cand.display_cmd),
                };
                let scroll_task = self.ensure_cursor_visible();
                let live_task = self.schedule_live_compile(650, false);
                Task::batch([scroll_task, live_task])
            }
            Message::ZoomInEditor => {
                let before = self.effective_editor_font_size();
                while self.editor_font_size < 30.0 {
                    self.editor_font_size = (self.editor_font_size + 1.0).min(30.0);
                    if self.effective_editor_font_size() > before {
                        break;
                    }
                }
                self.persist_user_settings();
                Task::none()
            }
            Message::ZoomOutEditor => {
                let before = self.effective_editor_font_size();
                while self.editor_font_size > 11.0 {
                    self.editor_font_size = (self.editor_font_size - 1.0).max(11.0);
                    if self.effective_editor_font_size() < before {
                        break;
                    }
                }
                self.persist_user_settings();
                Task::none()
            }
            Message::ResetZoomEditor => {
                self.editor_font_size = 16.0;
                self.persist_user_settings();
                Task::none()
            }
            Message::ZoomInPdf => {
                let target = self.pdf_zoom + 0.15;
                self.set_pdf_zoom(target)
            }
            Message::ZoomOutPdf => {
                let target = self.pdf_zoom - 0.15;
                self.set_pdf_zoom(target)
            }
            Message::ResetZoomPdf => self.set_pdf_zoom(1.0),
            Message::SelectPaletteTab(tab) => {
                self.palette_tab = tab;
                Task::none()
            }
            Message::InsertSnippet(snippet) => {
                let tab = self.tab_mut();
                tab.doc.insert_snippet(snippet);
                tab.last_edited_line = Some(tab.doc.content.cursor_position().0 + 1);
                tab.sync_highlight_line = None;
                self.refresh_suggestions();
                let scroll_task = self.ensure_cursor_visible();
                let live_task = self.schedule_live_compile(650, false);
                Task::batch([scroll_task, live_task])
            }
            Message::InsertFormula(formula) => {
                let added_pkg = {
                    let tab = self.tab_mut();
                    let pkg = tab.doc.insert_formula(formula);
                    tab.last_edited_line = Some(tab.doc.content.cursor_position().0 + 1);
                    tab.sync_highlight_line = None;
                    pkg
                };
                if let Some(pkg) = added_pkg {
                    let missing_group = self
                        .installed_tex_packages
                        .as_ref()
                        .filter(|set| !set.contains(pkg))
                        .and_then(|_| {
                            MATH_LIBRARIES
                                .iter()
                                .find(|l| l.package == pkg)
                                .map(|l| l.texlive_group)
                        });
                    if let Some(group) = missing_group {
                        self.status = format!(
                            "Formula inserita + \\usepackage{{{pkg}}} (installa {group} su TeX Live)"
                        );
                    } else {
                        self.status =
                            format!("Formula inserita (+ \\usepackage{{{pkg}}} nel preambolo)");
                    }
                }
                self.refresh_suggestions();
                let scroll_task = self.ensure_cursor_visible();
                let live_task = self.schedule_live_compile(650, false);
                Task::batch([scroll_task, live_task])
            }
            Message::AddLibrary(pkg) => {
                let missing_group = self
                    .installed_tex_packages
                    .as_ref()
                    .filter(|set| !set.contains(pkg))
                    .and_then(|_| {
                        MATH_LIBRARIES
                            .iter()
                            .find(|l| l.package == pkg)
                            .map(|l| l.texlive_group)
                    });

                if self.tab_mut().doc.add_package(pkg) {
                    if let Some(group) = missing_group {
                        self.status = format!(
                            "Aggiunta \\usepackage{{{pkg}}} (installa {group} su TeX Live)"
                        );
                    } else {
                        self.status = format!("Aggiunta libreria \\usepackage{{{pkg}}}");
                    }
                    return self.schedule_live_compile(650, false);
                } else if let Some(group) = missing_group {
                    self.status =
                        format!("{pkg} è nel preambolo ma manca in TeX Live (installa {group})");
                } else {
                    self.status = format!("La libreria {pkg} è già presente nel preambolo");
                }
                Task::none()
            }
            Message::AddLibraryPreset(name, pkgs) => {
                let added = self.tab_mut().doc.add_packages(pkgs);
                if added > 0 {
                    self.status = format!("Pack '{name}': aggiunte {added} librerie");
                    return self.schedule_live_compile(650, false);
                } else {
                    self.status = format!("Tutte le librerie del pack '{name}' sono già incluse");
                }
                Task::none()
            }
            Message::NextSuggestion => {
                if !self.suggestions.is_empty() {
                    self.selected_suggestion =
                        (self.selected_suggestion + 1) % self.suggestions.len();
                }
                Task::none()
            }
            Message::PrevSuggestion => {
                if !self.suggestions.is_empty() {
                    self.selected_suggestion = if self.selected_suggestion == 0 {
                        self.suggestions.len() - 1
                    } else {
                        self.selected_suggestion - 1
                    };
                }
                Task::none()
            }
            Message::DismissSuggestions => {
                self.suggestions.clear();
                self.selected_suggestion = 0;
                Task::none()
            }
            Message::AcceptSuggestion => {
                if let (Some(prefix), Some(choice)) = (
                    self.active_prefix.clone(),
                    self.suggestions.get(self.selected_suggestion).cloned(),
                ) {
                    let auto_close = self.auto_close;
                    let tab = self.tab_mut();
                    if choice.is_arg_completion {
                        tab.doc
                            .complete_argument(prefix.chars().count(), &choice.insert_text);
                    } else {
                        tab.doc
                            .complete_prefix(prefix.chars().count(), &choice.insert_text, auto_close);
                    }
                    tab.last_edited_line = Some(tab.doc.content.cursor_position().0 + 1);
                    tab.sync_highlight_line = None;
                    self.refresh_suggestions();
                    let scroll_task = self.ensure_cursor_visible();
                    let live_task = self.schedule_live_compile(650, false);
                    return Task::batch([scroll_task, live_task]);
                }
                Task::none()
            }
            Message::ApplySuggestion(sug_idx) => {
                if let (Some(prefix), Some(choice)) = (
                    self.active_prefix.clone(),
                    self.suggestions.get(sug_idx).cloned(),
                ) {
                    let auto_close = self.auto_close;
                    let tab = self.tab_mut();
                    if choice.is_arg_completion {
                        tab.doc
                            .complete_argument(prefix.chars().count(), &choice.insert_text);
                    } else {
                        tab.doc
                            .complete_prefix(prefix.chars().count(), &choice.insert_text, auto_close);
                    }
                    tab.last_edited_line = Some(tab.doc.content.cursor_position().0 + 1);
                    tab.sync_highlight_line = None;
                    self.refresh_suggestions();
                    let scroll_task = self.ensure_cursor_visible();
                    let live_task = self.schedule_live_compile(650, false);
                    return Task::batch([scroll_task, live_task]);
                }
                Task::none()
            }
            Message::EditorAction(action) => {
                let auto_close = self.auto_close;
                let is_edit = matches!(action, text_editor::Action::Edit(_));
                let is_click = matches!(action, text_editor::Action::Click(_));
                let is_drag = matches!(action, text_editor::Action::Drag(_));
                let is_scroll = matches!(action, text_editor::Action::Scroll { .. });

                if is_edit || is_click {
                    self.pdf_context_menu = None;
                    self.show_folder_menu = false;
                    self.show_recipe_menu = false;
                }
                if is_click {
                    self.editor_dragging = true;
                    self.editor_drag_local = None;
                } else if is_edit {
                    self.editor_dragging = false;
                    self.editor_drag_local = None;
                }

                let tab = self.tab_mut();
                let prev_cursor = tab.doc.content.cursor_position();
                let prev_lines = tab.doc.line_count();
                tab.doc.apply_action(action, auto_close);
                let new_cursor = tab.doc.content.cursor_position();

                if is_edit || is_click {
                    tab.sync_highlight_line = None;
                }

                if is_edit {
                    tab.last_edited_line = Some(new_cursor.0 + 1);
                    if !tab.error_lines.is_empty() {
                        let new_lines = tab.doc.line_count();
                        let delta = new_lines as isize - prev_lines as isize;
                        if delta != 0 {
                            let pivot = prev_cursor.0;
                            tab.error_lines = tab
                                .error_lines
                                .iter()
                                .filter_map(|&l| {
                                    if l > pivot {
                                        let shifted = l as isize + delta;
                                        (shifted >= 0).then_some(shifted as usize)
                                    } else {
                                        Some(l)
                                    }
                                })
                                .collect();
                        }
                    }
                }

                self.refresh_suggestions();
                let scroll_task =
                    if !is_scroll && !is_click && !is_drag && new_cursor != prev_cursor {
                        self.ensure_cursor_visible()
                    } else {
                        Task::none()
                    };
                if is_edit {
                    self.refresh_outline();
                    if self.show_find_bar {
                        let _ = self.refresh_find_state(false);
                    }
                    let live_task = self.schedule_live_compile(650, false);
                    Task::batch([scroll_task, live_task])
                } else {
                    scroll_task
                }
            }
            Message::EditorDragMoved { local, viewport } => {
                if !self.editor_dragging {
                    return Task::none();
                }
                self.editor_drag_local = Some(local);
                if viewport.width > 50.0 {
                    self.editor_viewport_width = viewport.width;
                }
                if viewport.height > 50.0 {
                    self.editor_viewport_height = viewport.height;
                }
                if local.x < 0.0
                    || local.x > viewport.width - 10.0
                    || local.y < 0.0
                    || local.y > viewport.height
                {
                    self.apply_drag_selection_at(local, viewport);
                }
                Task::none()
            }
            Message::EditorDragTick => self.step_editor_drag_autoscroll(),
            Message::EditorDragEnded => {
                self.editor_dragging = false;
                self.editor_drag_local = None;
                Task::none()
            }
            Message::EditorCopy => {
                if let Some(sel) = self.tab().doc.content.selection() {
                    if !sel.is_empty() {
                        self.internal_clipboard = Some(sel.clone());
                        write_wl_clipboard_bg(sel.clone());
                        self.status = "Copiato".to_string();
                        return clipboard::write(sel);
                    }
                }
                Task::none()
            }
            Message::EditorCut => {
                if let Some(sel) = self.tab_mut().doc.cut_selection() {
                    self.internal_clipboard = Some(sel.clone());
                    write_wl_clipboard_bg(sel.clone());
                    let tab = self.tab_mut();
                    tab.last_edited_line = Some(tab.doc.content.cursor_position().0 + 1);
                    tab.sync_highlight_line = None;
                    self.refresh_suggestions();
                    self.status = "Tagliato".to_string();
                    let scroll_task = self.ensure_cursor_visible();
                    let live_task = self.schedule_live_compile(650, false);
                    return Task::batch([clipboard::write(sel), scroll_task, live_task]);
                }
                Task::none()
            }
            Message::EditorPasteRequest => clipboard::read().then(|iced_clip| match iced_clip {
                Some(s) if !s.is_empty() => Task::done(Message::EditorPasteReady(Some(s))),
                _ => Task::perform(read_wl_clipboard(), Message::EditorPasteReady),
            }),
            Message::EditorPasteReady(opt) => {
                let text = opt
                    .filter(|s| !s.is_empty())
                    .or_else(|| self.internal_clipboard.clone());
                if let Some(text) = text {
                    let tab = self.tab_mut();
                    tab.doc.insert_snippet(&text);
                    tab.last_edited_line = Some(tab.doc.content.cursor_position().0 + 1);
                    tab.sync_highlight_line = None;
                    self.refresh_suggestions();
                    let scroll_task = self.ensure_cursor_visible();
                    let live_task = self.schedule_live_compile(650, false);
                    return Task::batch([scroll_task, live_task]);
                }
                Task::none()
            }
            Message::EditorUndo => {
                if self.tab_mut().doc.undo() {
                    let tab = self.tab_mut();
                    tab.last_edited_line = Some(tab.doc.content.cursor_position().0 + 1);
                    tab.sync_highlight_line = None;
                    self.refresh_suggestions();
                    self.status = "Annullato".to_string();
                    let scroll_task = self.ensure_cursor_visible();
                    let live_task = self.schedule_live_compile(650, false);
                    return Task::batch([scroll_task, live_task]);
                }
                Task::none()
            }
            Message::EditorRedo => {
                if self.tab_mut().doc.redo() {
                    let tab = self.tab_mut();
                    tab.last_edited_line = Some(tab.doc.content.cursor_position().0 + 1);
                    tab.sync_highlight_line = None;
                    self.refresh_suggestions();
                    self.status = "Ripristinato".to_string();
                    let scroll_task = self.ensure_cursor_visible();
                    let live_task = self.schedule_live_compile(650, false);
                    return Task::batch([scroll_task, live_task]);
                }
                Task::none()
            }
            Message::PreamblePreloaded => Task::none(),
            Message::EditorScrolled(viewport) => {
                self.tab_mut().editor_scroll_offset = viewport.absolute_offset();
                let bounds = viewport.bounds();
                if bounds.height > 50.0 {
                    self.editor_viewport_height = bounds.height;
                }
                if bounds.width > 50.0 {
                    self.editor_viewport_width = bounds.width;
                }
                Task::none()
            }
            Message::PdfScrolled(viewport) => {
                self.pdf_context_menu = None;
                self.tab_mut().pdf_scroll_offset = viewport.absolute_offset();
                let h = viewport.bounds().height;
                if h > 50.0 {
                    self.pdf_viewport_height = h;
                }
                self.request_visible_pdf_pages()
            }
            Message::PdfPagesRendered(Ok((gen, pages))) => {
                if gen != self.pdf_generation {
                    return Task::none();
                }
                for p in &pages {
                    self.pdf_rendering.remove(&p.page_number);
                }
                self.pdf_rendering.clear();
                let rendered_w = self.pdf_in_flight_width;
                self.insert_rendered_pages(pages, rendered_w);
                self.request_visible_pdf_pages()
            }
            Message::PdfPagesRendered(Err(err)) => {
                self.pdf_rendering.clear();
                self.tab_mut().preview_note = Some(err);
                Task::none()
            }
            Message::PdfResizeDebounced(rev) => {
                if rev == self.pdf_resize_rev
                    && self.show_pdf
                    && self.tab().pdf_total_pages > 0
                    && self.pdf_cache_width != self.target_pdf_render_width()
                {
                    self.reset_gpu_pdf_cache();
                    return self.request_visible_pdf_pages();
                }
                Task::none()
            }
            Message::WindowOpened(id) => {
                window::get_scale_factor(id).map(Message::ScaleFactorDetected)
            }
            Message::WindowResized(id, size) => {
                let old_fit = ui::pdf_fit_base_width(self);
                self.window_size = size;
                self.editor_viewport_width = 0.0;
                self.editor_dragging = false;
                self.editor_drag_local = None;
                self.pdf_context_menu = None;
                if !self.show_pdf {
                    self.pdf_viewport_height = (size.height - 170.0).max(400.0);
                }
                let pdf_task = self.handle_pdf_width_change(old_fit, true);
                let sf_task = window::get_scale_factor(id).map(Message::ScaleFactorDetected);
                Task::batch([pdf_task, sf_task])
            }
            Message::ScaleFactorDetected(sf) => {
                let clamped = sf.clamp(1.0, 4.0);
                if (clamped - self.window_scale_factor).abs() > 0.01 {
                    self.window_scale_factor = clamped;
                    if self.show_pdf && self.tab().pdf_total_pages > 0 {
                        self.reset_gpu_pdf_cache();
                        return self.request_visible_pdf_pages();
                    }
                }
                Task::none()
            }
            Message::CloseRequested(window_id) => {
                self.cancel_live_compile();
                self.persist_user_settings();
                let _ = self.save_all_open_files();
                window::close(window_id)
            }
            Message::New => {
                self.show_folder_menu = false;
                self.pdf_context_menu = None;
                if self.screen == Screen::Welcome
                    && self.tabs.len() == 1
                    && self.tabs[0].is_untouched_placeholder()
                {
                    self.screen = Screen::Editor;
                    self.status = "Nuovo documento".to_string();
                    return scrollable::scroll_to(
                        self.editor_scroll_id.clone(),
                        scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
                    );
                }

                let id = self.alloc_tab_id();
                self.tabs.push(TabState::new(id));
                let new_idx = self.tabs.len() - 1;
                self.screen = Screen::Editor;
                self.status = "Nuovo documento".to_string();
                self.switch_to_tab(new_idx)
            }
            Message::Open => {
                self.show_folder_menu = false;
                Task::perform(document::pick_and_read_file(), Message::FileOpened)
            }
            Message::OpenRecent(path) => {
                self.show_folder_menu = false;
                if let Some(existing_idx) = self.find_tab_by_path(&path) {
                    self.screen = Screen::Editor;
                    self.status = format!("Passato a {}", self.tabs[existing_idx].doc.file_name());
                    return self.switch_to_tab(existing_idx);
                }
                Task::perform(document::read_file(path), Message::FileOpened)
            }
            Message::FileOpened(Ok((path, content))) => {
                recent::record_recent(&mut self.recent_files, path.clone());
                self.screen = Screen::Editor;

                if let Some(existing_idx) = self.find_tab_by_path(&path) {
                    self.status = format!("Passato a {}", self.tabs[existing_idx].doc.file_name());
                    return self.switch_to_tab(existing_idx);
                }

                let preload_task = Task::perform(
                    compiler::preload_preamble_fmt(path.clone(), content.clone()),
                    |_| Message::PreamblePreloaded,
                );

                let target_idx = if self.tabs.len() == 1 && self.tabs[0].is_untouched_placeholder()
                {
                    self.tabs[0].doc.load_text(path, content);
                    self.tabs[0].clear_pdf();
                    0
                } else {
                    let id = self.alloc_tab_id();
                    let mut new_tab = TabState::new(id);
                    new_tab.doc.load_text(path, content);
                    self.tabs.push(new_tab);
                    self.tabs.len() - 1
                };

                self.status = format!("Aperto {}", self.tabs[target_idx].doc.file_name());
                Task::batch([self.switch_to_tab(target_idx), preload_task])
            }
            Message::FileOpened(Err(err)) => {
                self.status = err;
                Task::none()
            }
            Message::Save => {
                if self.screen != Screen::Editor {
                    return Task::none();
                }
                let tab = self.tab();
                let tab_id = tab.id;
                let path = tab.doc.path.clone();
                let contents = tab.doc.text();
                Task::perform(document::save_file(path, contents), move |res| {
                    Message::FileSaved(tab_id, res)
                })
            }
            Message::FileSaved(tab_id, Ok(path)) => {
                recent::record_recent(&mut self.recent_files, path.clone());
                if let Some(idx) = self.find_tab_by_id(tab_id) {
                    self.tabs[idx].doc.path = Some(path);
                    self.tabs[idx].doc.is_dirty = false;
                }
                self.refresh_folder_files();
                self.refresh_outline();
                self.refresh_bib_cache();
                self.refresh_label_cache();
                self.status = "Salvato".to_string();
                self.spawn_word_count_task()
            }
            Message::FileSaved(_, Err(err)) => {
                self.status = err;
                Task::none()
            }
            Message::Compile => {
                if self.screen != Screen::Editor || self.is_compiling {
                    return Task::none();
                }
                self.pdf_context_menu = None;
                self.show_folder_menu = false;
                self.show_recipe_menu = false;
                let tab_id = self.tab().id;
                let path = self.tab().doc.path.clone();
                let contents = self.tab().doc.text();
                let current_hash = compiler::text_fingerprint(&contents);
                let use_fast_live = self.can_use_fast_live_pdflatex();

                // Fast path A (Overleaf-style instant swap): a background compile for this exact
                // buffer is already finished -> save `.tex`, atomically swap `.build/live/*.pdf`
                // over `.build/*.pdf`, and display immediately in ~0ms!
                if use_fast_live {
                    if let Some(existing_path) = path.clone() {
                        let has_matching_staged = self
                            .tab()
                            .staged_build
                            .as_ref()
                            .is_some_and(|s| s.text_hash == current_hash);
                        if has_matching_staged {
                            if let Err(e) = std::fs::write(&existing_path, &contents) {
                                self.status = format!("Errore scrittura file: {e}");
                                return Task::none();
                            }
                            recent::record_recent(&mut self.recent_files, existing_path.clone());
                            self.tab_mut().doc.is_dirty = false;
                            self.cancel_live_compile();

                            if let Some(staged) = self.tab_mut().staged_build.take() {
                                self.is_compiling = true;
                                let idx = self.active_tab;
                                return self.spawn_promote_task(
                                    tab_id,
                                    idx,
                                    existing_path,
                                    staged.result,
                                    true,
                                );
                            }
                        }

                        // Fast path B: a background compile for this exact buffer is already in flight
                        // -> save `.tex` now and promote its output as soon as it completes!
                        if self.is_live_compiling
                            && self.live_in_flight_hash == Some((tab_id, current_hash))
                        {
                            if let Err(e) = std::fs::write(&existing_path, &contents) {
                                self.status = format!("Errore scrittura file: {e}");
                                return Task::none();
                            }
                            recent::record_recent(&mut self.recent_files, existing_path.clone());
                            self.tab_mut().doc.is_dirty = false;
                            self.is_compiling = true;
                            self.promote_live_on_finish = true;
                            self.status = "Compilazione in corso...".to_string();
                            return Task::none();
                        }
                    }
                }

                self.cancel_live_compile();
                self.tab_mut().staged_build = None;
                self.is_compiling = true;
                let recipe_name = self.recipe_config.active_recipe().name;
                self.status = format!("Compilazione ({recipe_name})...");
                Task::perform(document::save_file(path, contents), move |res| {
                    Message::CompileAfterSave(tab_id, res)
                })
            }
            Message::CompileAfterSave(tab_id, Ok(path)) => {
                recent::record_recent(&mut self.recent_files, path.clone());
                let Some(idx) = self.find_tab_by_id(tab_id) else {
                    self.is_compiling = false;
                    return Task::none();
                };
                self.tabs[idx].doc.path = Some(path.clone());
                self.tabs[idx].doc.is_dirty = false;
                if idx == self.active_tab {
                    self.refresh_folder_files();
                }
                let sync_line = self.tabs[idx]
                    .last_edited_line
                    .unwrap_or_else(|| self.tabs[idx].doc.content.cursor_position().0 + 1);
                let fallback_page = if idx == self.active_tab {
                    self.current_pdf_start_page()
                } else {
                    1
                };
                let target_w = self.target_pdf_render_width();
                let recipe = self.recipe_config.active_recipe();
                let shell_escape = self.recipe_config.shell_escape;
                let master_override = self.effective_master_for_tab(idx);
                Task::perform(
                    compiler::compile_with_recipe(
                        path,
                        master_override,
                        recipe,
                        shell_escape,
                        sync_line,
                        fallback_page,
                        target_w,
                    ),
                    move |res| Message::CompileFinished(tab_id, res),
                )
            }
            Message::CompileAfterSave(_, Err(err)) => {
                self.is_compiling = false;
                self.status = format!("Compilazione annullata: {err}");
                Task::none()
            }
            Message::CompileFinished(tab_id, Ok(result)) => {
                self.is_compiling = false;
                let Some(idx) = self.find_tab_by_id(tab_id) else {
                    return Task::none();
                };
                let ui_task = self.apply_compile_result(idx, result, false);
                let wc_task = self.spawn_word_count_task();
                let lint_task = self.spawn_chktex_task();
                Task::batch([ui_task, wc_task, lint_task])
            }
            Message::CompileFinished(tab_id, Err(err)) => {
                self.is_compiling = false;
                if let Some(idx) = self.find_tab_by_id(tab_id) {
                    let tab = &mut self.tabs[idx];
                    tab.last_compile_ok = Some(false);
                    tab.diagnostics.clear();
                    tab.error_lines.clear();
                    tab.show_raw_log = true;
                    tab.compile_log = err;
                    if idx == self.active_tab {
                        self.show_pdf = false;
                        self.show_log = true;
                    }
                }
                self.status = "Errore compilatore".to_string();
                Task::none()
            }
            Message::CleanBuild => {
                let tab = self.tab();
                let tab_id = tab.id;
                let Some(path) = tab.doc.path.clone() else {
                    self.status = "Salva prima il file".to_string();
                    return Task::none();
                };
                Task::perform(compiler::clean_build_dir(path), move |res| {
                    Message::BuildCleaned(tab_id, res)
                })
            }
            Message::BuildCleaned(tab_id, Ok(msg)) => {
                if let Some(idx) = self.find_tab_by_id(tab_id) {
                    self.tabs[idx].clear_pdf();
                    self.tabs[idx].staged_build = None;
                    self.tabs[idx].last_compile_ok = None;
                    if idx == self.active_tab {
                        self.clear_active_pdf_state();
                    }
                }
                self.status = msg;
                Task::none()
            }
            Message::BuildCleaned(_, Err(err)) => {
                self.status = err;
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        ui::view(self)
    }
}

fn compute_drag_scroll_step(local: Point, viewport: Size) -> (f32, f32) {
    const EDGE: f32 = 28.0;
    const RIGHT_INSET: f32 = 10.0;

    let dy = if local.y < EDGE {
        let d = (EDGE - local.y).clamp(0.0, 260.0);
        -(4.0 + d * 0.35).min(48.0)
    } else if local.y > viewport.height - EDGE {
        let d = (local.y - (viewport.height - EDGE)).clamp(0.0, 260.0);
        (4.0 + d * 0.35).min(48.0)
    } else {
        0.0
    };

    let right_edge = (viewport.width - RIGHT_INSET - EDGE).max(EDGE);
    let dx = if local.x < EDGE {
        let d = (EDGE - local.x).clamp(0.0, 240.0);
        -(4.0 + d * 0.35).min(42.0)
    } else if local.x > right_edge {
        let d = (local.x - right_edge).clamp(0.0, 240.0);
        (4.0 + d * 0.35).min(42.0)
    } else {
        0.0
    };

    (dx, dy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_construct_without_panicking() {
        let mut app = App::default();
        let _ = app.view();

        app.screen = Screen::Editor;
        let _ = app.view();

        // Multiple tabs + folder menu + PDF context menu
        let id2 = app.alloc_tab_id();
        let mut tab2 = TabState::new(id2);
        tab2.doc
            .load_text(PathBuf::from("/tmp/chapter2.tex"), "\\section{Two}".to_string());
        app.tabs.push(tab2);
        app.show_folder_menu = true;
        let _ = app.view();
        app.show_folder_menu = false;

        app.show_recipe_menu = true;
        app.show_new_recipe_builder = true;
        app.new_recipe_name = "Test custom".to_string();
        app.new_recipe_steps = vec![RecipeStep::LuaLatex, RecipeStep::Biber];
        let _ = app.view();
        app.show_recipe_menu = false;

        app.show_outline = true;
        app.refresh_outline();
        app.show_find_bar = true;
        app.show_replace_row = true;
        app.find_in_folder = true;
        app.find_query = "section".to_string();
        let _ = app.refresh_find_state(false);
        let _ = app.view();
        app.show_find_bar = false;
        app.show_goto_bar = true;
        app.goto_line_input = "2".to_string();
        let _ = app.view();
        app.show_goto_bar = false;
        app.show_outline = false;

        app.show_assets = true;
        let _ = app.view();
        app.show_assets = false;

        app.show_log = true;
        app.log_tab = LogTab::Linter;
        app.tab_mut().lint_warnings = vec![LintWarning {
            line: 2,
            col: 5,
            code: 2,
            title: "Spazio indivisibile '~'".to_string(),
            detail: "Usa ~ prima di \\cite".to_string(),
        }];
        let _ = app.view();
        app.log_tab = LogTab::Spellcheck;
        app.tab_mut().spell_outcome = Some(SpellCheckOutcome::MissingDictionary {
            lang: "it_IT".to_string(),
            hint: "sudo pacman -S hunspell-it".to_string(),
        });
        let _ = app.view();
        app.log_tab = LogTab::Compilation;

        app.show_symbols = true;
        app.palette_tab = PaletteTab::Libraries;
        app.show_pdf = true;
        app.tab_mut().pdf_total_pages = 33;
        app.tab_mut().pdf_scroll_offset.y = 14000.0;
        app.pdf_context_menu = Some(PdfContextMenu {
            page: 12,
            x_ratio: 0.4,
            y_ratio: 0.5,
            anchor: Point::new(820.0, 340.0),
        });
        app.dark_mode = true;
        let _ = app.view();
        app.pdf_context_menu = None;

        app.show_pdf = false;
        app.show_log = true;
        app.tab_mut().last_compile_ok = Some(false);
        app.tab_mut().diagnostics = vec![CompileDiagnostic {
            line: Some(1),
            title: "Comando sconosciuto: \\foo".to_string(),
            explanation: "Controlla l'ortografia.".to_string(),
            raw_excerpt: "l.1 \\foo".to_string(),
        }];
        app.tab_mut().error_lines.insert(0);
        app.tab_mut().sync_highlight_line = Some(2);
        app.tab_mut().doc.insert_snippet(&"x".repeat(300));
        let _ = app.view();

        app.show_symbols = false;
        app.show_match = true;
        app.match_strokes = vec![vec![Point::new(40.0, 40.0), Point::new(120.0, 140.0)]];
        app.match_results = matcher::recognize(&app.match_strokes, 6);
        let _ = app.view();

        let big = (0..2000)
            .map(|i| format!("Line {i}: $\\forall x \\in \\mathbb{{R}}$"))
            .collect::<Vec<_>>()
            .join("\n");
        app.tab_mut().doc.load_text(PathBuf::from("big.tex"), big);
        app.tab_mut().error_lines.insert(500);
        app.tab_mut().editor_scroll_offset.y = 12000.0;
        let _ = app.view();
    }

    #[test]
    fn saves_all_dirty_named_tabs_on_close() {
        let dir = std::env::temp_dir().join("feathertex_autosave_test");
        let _ = std::fs::create_dir_all(&dir);
        let f1 = dir.join("doc1.tex");
        let f2 = dir.join("doc2.tex");
        std::fs::write(&f1, "old1").unwrap();
        std::fs::write(&f2, "old2").unwrap();

        let mut app = App::default();
        app.tabs[0].doc.load_text(f1.clone(), "old1".to_string());
        app.tabs[0].doc.insert_snippet(" NEW1");

        let id2 = app.alloc_tab_id();
        let mut t2 = TabState::new(id2);
        t2.doc.load_text(f2.clone(), "old2".to_string());
        t2.doc.insert_snippet(" NEW2");
        app.tabs.push(t2);

        let saved = app.save_all_open_files();
        assert_eq!(saved, 2);
        assert!(!app.tabs[0].doc.is_dirty);
        assert!(!app.tabs[1].doc.is_dirty);
        assert!(std::fs::read_to_string(&f1).unwrap().contains("NEW1"));
        assert!(std::fs::read_to_string(&f2).unwrap().contains("NEW2"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn closing_last_tab_returns_to_welcome_screen() {
        let mut app = App::default();
        app.screen = Screen::Editor;
        assert_eq!(app.tabs.len(), 1);
        let _ = app.update(Message::CloseTab(0));
        assert_eq!(app.screen, Screen::Welcome);
        assert_eq!(app.tabs.len(), 1);
    }

    #[test]
    fn responsive_scaling_adapts_pdf_and_font_to_window_width() {
        let mut app = App::default();
        app.show_pdf = true;

        app.window_size = Size::new(1920.0, 1080.0);
        let full_pdf = ui::pdf_fit_base_width(&app);
        let full_font = app.effective_editor_font_size();

        app.window_size = Size::new(1280.0, 800.0);
        let hd_pdf = ui::pdf_fit_base_width(&app);
        let hd_font = app.effective_editor_font_size();

        app.window_size = Size::new(950.0, 700.0);
        let win_pdf = ui::pdf_fit_base_width(&app);
        let win_font = app.effective_editor_font_size();

        app.window_size = Size::new(760.0, 500.0);
        let min_pdf = ui::pdf_fit_base_width(&app);
        let min_font = app.effective_editor_font_size();

        assert!(full_pdf > hd_pdf && hd_pdf > win_pdf && win_pdf > min_pdf);
        assert!(min_pdf >= 240.0);
        assert_eq!(full_font, 16.0);
        assert!(hd_font <= full_font && win_font < hd_font && min_font <= win_font);
        assert!(min_font >= 12.0);
    }

    #[test]
    fn cursor_follow_scrolls_horizontally_and_resets_at_col_zero() {
        let mut app = App::default();
        app.screen = Screen::Editor;
        app.show_pdf = true;
        app.window_size = Size::new(1000.0, 700.0);
        app.editor_viewport_width = 380.0;

        let long_line = "a".repeat(140);
        app.tab_mut()
            .doc
            .load_text(PathBuf::from("/tmp/long.tex"), long_line);
        for _ in 0..110 {
            app.tab_mut()
                .doc
                .apply_action(text_editor::Action::Move(text_editor::Motion::Right), false);
        }

        let _ = app.ensure_cursor_visible();
        assert!(
            app.tab().editor_scroll_offset.x > 0.0,
            "expected rightward horizontal scroll when cursor passes viewport edge"
        );

        app.tab_mut()
            .doc
            .apply_action(text_editor::Action::Move(text_editor::Motion::Home), false);
        let _ = app.ensure_cursor_visible();
        assert_eq!(app.tab().editor_scroll_offset.x, 0.0);
    }

    #[test]
    fn drag_autoscroll_steps_at_all_margins_and_extends_selection() {
        let vp = Size::new(400.0, 300.0);
        assert_eq!(compute_drag_scroll_step(Point::new(200.0, 150.0), vp), (0.0, 0.0));

        let (lx, _) = compute_drag_scroll_step(Point::new(-20.0, 150.0), vp);
        let (rx_near, _) = compute_drag_scroll_step(Point::new(375.0, 150.0), vp);
        let (rx_far, _) = compute_drag_scroll_step(Point::new(460.0, 150.0), vp);
        assert!(lx < 0.0);
        assert!(rx_near > 0.0 && rx_far > rx_near);

        let (_, ty) = compute_drag_scroll_step(Point::new(200.0, 5.0), vp);
        let (_, by_near) = compute_drag_scroll_step(Point::new(200.0, 285.0), vp);
        let (_, by_far) = compute_drag_scroll_step(Point::new(200.0, 380.0), vp);
        assert!(ty < 0.0);
        assert!(by_near > 0.0 && by_far > by_near);

        let mut app = App::default();
        app.screen = Screen::Editor;
        app.editor_viewport_width = 380.0;
        app.editor_viewport_height = 240.0;
        let many_lines = (0..120)
            .map(|i| format!("Riga numero {i} con testo abbastanza lungo per selezionare tutto {}", "x".repeat(90)))
            .collect::<Vec<_>>()
            .join("\n");
        app.tab_mut()
            .doc
            .load_text(PathBuf::from("/tmp/drag.tex"), many_lines);

        // Start selection near top-left
        app.tab_mut()
            .doc
            .apply_action(text_editor::Action::Click(Point::new(20.0, 5.0)), false);
        app.editor_dragging = true;
        app.editor_drag_local = Some(Point::new(450.0, 320.0));

        let _ = app.step_editor_drag_autoscroll();
        assert!(app.tab().editor_scroll_offset.y > 0.0);
        assert!(app.tab().editor_scroll_offset.x > 0.0);
        let sel = app
            .tab()
            .doc
            .content
            .selection()
            .expect("expected active selection after drag autoscroll");
        assert!(sel.contains('\n'), "expected multi-line selection across bottom margin");
    }

    #[test]
    fn zoom_in_and_out_always_change_effective_font_size() {
        let mut app = App::default();
        app.show_pdf = true;

        for width in [1920.0_f32, 900.0_f32] {
            app.window_size = Size::new(width, 750.0);
            let _ = app.update(Message::ResetZoomEditor);
            let base = app.effective_editor_font_size();

            let _ = app.update(Message::ZoomInEditor);
            let zoomed_in = app.effective_editor_font_size();
            assert!(
                zoomed_in > base,
                "expected ZoomInEditor at width={width} to increase font size ({base} -> {zoomed_in})"
            );

            let _ = app.update(Message::ZoomOutEditor);
            let zoomed_back = app.effective_editor_font_size();
            assert!(
                zoomed_back < zoomed_in,
                "expected ZoomOutEditor at width={width} to decrease font size ({zoomed_in} -> {zoomed_back})"
            );
        }
    }
}
