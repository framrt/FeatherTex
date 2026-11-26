use crate::matcher;
use crate::project::{OutlineFilter, OutlineKind};
use crate::quality::SpellCheckOutcome;
use crate::recipes::{self, RecipeStep};
use crate::symbols::{
    FormulaItem, LibraryCategory, PaletteTab, SymbolItem, FORMULAS, GREEK_SYMBOLS, LIBRARY_PRESETS,
    LOGIC_SYMBOLS, MATH_LIBRARIES, MATH_SYMBOLS,
};
use crate::syntax::{LatexHighlighter, Token};
use crate::theme;
use crate::{App, LogTab, Message, PdfContextMenu, Screen};
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::widget::{Operation, Tree, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::event::{self, Event};
use iced::mouse;
use iced::widget::text::{LineHeight, Shaping, Wrapping};
use iced::widget::{
    button, column, container, horizontal_rule, horizontal_space, image, mouse_area, row,
    scrollable, stack, text, text_editor, text_input, vertical_rule, Column, Row, Space,
};
use iced::{keyboard, Alignment, Color, Element, Length, Padding, Point, Rectangle, Size, Theme};

pub const EDITOR_PADDING: f32 = 16.0;
const EDITOR_BOTTOM_BREATHING: f32 = 180.0;
pub const MONO_CHAR_ADVANCE: f32 = 0.602;
pub const PDF_LIST_PADDING: f32 = 14.0;
pub const PDF_PAGE_SPACING: f32 = 18.0;
pub const PDF_CARD_PADDING: f32 = 4.0;
const GUTTER_BUFFER_LINES: usize = 4;

pub fn editor_line_height(font_size: f32) -> f32 {
    (font_size * 1.5).round()
}

pub fn editor_char_width(font_size: f32) -> f32 {
    font_size * MONO_CHAR_ADVANCE
}

pub fn symbols_panel_width(window_width: f32) -> f32 {
    (window_width * 0.28).clamp(256.0, 344.0).round()
}

pub fn pdf_panel_padding(window_width: f32) -> f32 {
    if window_width < 1050.0 {
        10.0
    } else {
        14.0
    }
}

pub fn editor_pane_width(app: &App) -> f32 {
    let left_w = if app.show_symbols || app.show_match || app.show_outline || app.show_assets {
        symbols_panel_width(app.window_size.width) + 1.0
    } else {
        0.0
    };
    let remaining = (app.window_size.width - left_w).max(260.0);
    if app.show_pdf {
        ((remaining - 1.0) * 0.5).max(160.0)
    } else if app.show_log {
        ((remaining - 1.0) * 0.6).max(160.0)
    } else {
        remaining
    }
}

pub fn pdf_panel_width(app: &App) -> f32 {
    let left_w = if app.show_symbols || app.show_match || app.show_outline || app.show_assets {
        symbols_panel_width(app.window_size.width) + 1.0
    } else {
        0.0
    };
    let remaining = (app.window_size.width - left_w).max(260.0);
    ((remaining - 1.0) * 0.5).max(240.0)
}

pub fn pdf_fit_base_width(app: &App) -> f32 {
    let pad = pdf_panel_padding(app.window_size.width);
    let chrome = pad * 2.0 + PDF_LIST_PADDING * 2.0 + PDF_CARD_PADDING * 2.0 + 12.0;
    (pdf_panel_width(app) - chrome).clamp(240.0, 960.0).floor()
}

pub fn pdf_page_dimensions(base_width: f32, zoom: f32, aspect_ratio: f32) -> (f32, f32) {
    let w = (base_width * zoom).round().max(180.0);
    let h = (w * aspect_ratio).round();
    (w, h)
}

pub fn pdf_page_stride(base_width: f32, zoom: f32, aspect_ratio: f32) -> f32 {
    let (_, h) = pdf_page_dimensions(base_width, zoom, aspect_ratio);
    h + PDF_CARD_PADDING * 2.0 + PDF_PAGE_SPACING
}

fn thin_scrollbar() -> scrollable::Scrollbar {
    scrollable::Scrollbar::new()
        .width(8)
        .margin(2)
        .scroller_width(6)
}

pub fn gutter_width(line_count: usize, font_size: f32) -> f32 {
    let digits = line_count.to_string().len().max(3);
    ((digits as f32) * (font_size * 0.62) + 28.0)
        .ceil()
        .max(52.0)
}

pub fn editor_code_viewport_width(app: &App) -> f32 {
    let font_size = app.effective_editor_font_size();
    let gutter_w = gutter_width(app.tab().doc.line_count(), font_size);
    // Subtract gutter, 1px vertical separator, and vertical scrollbar lane.
    (editor_pane_width(app) - gutter_w - 11.0).max(120.0)
}

pub fn editor_needed_text_width(max_chars: usize, font_size: f32) -> f32 {
    ((max_chars as f32) * editor_char_width(font_size) + EDITOR_PADDING + 72.0).ceil()
}

pub fn editor_total_content_height(line_count: usize, font_size: f32) -> f32 {
    EDITOR_PADDING + (line_count as f32) * editor_line_height(font_size) + EDITOR_BOTTOM_BREATHING
}

fn close_panel_button(on_press: Message) -> Element<'static, Message> {
    button(
        text("×")
            .font(theme::ui_font_bold())
            .shaping(Shaping::Advanced)
            .size(16),
    )
    .padding([1, 8])
    .style(theme::ghost_button)
    .on_press(on_press)
    .into()
}

pub fn view(app: &App) -> Element<'_, Message> {
    match app.screen {
        Screen::Welcome => view_welcome(app),
        Screen::Editor => view_editor(app),
    }
}

fn view_welcome(app: &App) -> Element<'_, Message> {
    let pal = theme::palette_for(app.dark_mode);
    let theme_label = if app.dark_mode {
        "Tema Chiaro"
    } else {
        "Tema Scuro"
    };

    let emblem = container(
        text("Fe")
            .font(theme::ui_font_bold())
            .size(24)
            .color(Color::WHITE),
    )
    .width(Length::Fixed(58.0))
    .height(Length::Fixed(58.0))
    .center_x(Length::Fixed(58.0))
    .center_y(Length::Fixed(58.0))
    .style(theme::brand_badge);

    let header = column![
        emblem,
        Space::with_height(6),
        text("FeatherTex")
            .font(theme::ui_font_bold())
            .size(32)
            .align_x(Alignment::Center)
            .color(pal.text_primary),
        text("Editor LaTeX leggero, elegante e pensato per la matematica")
            .size(14)
            .align_x(Alignment::Center)
            .color(pal.text_muted),
    ]
    .spacing(6)
    .width(Length::Fill)
    .align_x(Alignment::Center);

    let new_icon = container(
        text("+")
            .font(theme::ui_font_bold())
            .size(18)
            .color(pal.accent),
    )
    .width(Length::Fixed(38.0))
    .height(Length::Fixed(38.0))
    .center_x(Length::Fixed(38.0))
    .center_y(Length::Fixed(38.0))
    .style(theme::icon_chip);

    let new_shortcut = container(
        text("Ctrl+N")
            .font(theme::mono_font())
            .size(11)
            .color(pal.text_muted),
    )
    .padding([3, 8])
    .style(theme::shortcut_pill);

    let new_file_btn = button(
        row![
            new_icon,
            column![
                text("Crea nuovo documento")
                    .font(theme::ui_font_semibold())
                    .size(15)
                    .color(pal.text_primary),
                text("Template pronto con amsmath, amssymb, amsthm, mathtools e bm")
                    .size(12)
                    .color(pal.text_muted),
            ]
            .spacing(3),
            horizontal_space(),
            new_shortcut,
        ]
        .spacing(14)
        .align_y(Alignment::Center)
        .padding([12, 14]),
    )
    .width(Length::Fill)
    .style(theme::action_card_button)
    .on_press(Message::New);

    let open_icon = container(
        text("↗")
            .font(theme::math_symbol_font())
            .shaping(Shaping::Advanced)
            .size(17)
            .color(pal.accent),
    )
    .width(Length::Fixed(38.0))
    .height(Length::Fixed(38.0))
    .center_x(Length::Fixed(38.0))
    .center_y(Length::Fixed(38.0))
    .style(theme::icon_chip);

    let open_shortcut = container(
        text("Ctrl+O")
            .font(theme::mono_font())
            .size(11)
            .color(pal.text_muted),
    )
    .padding([3, 8])
    .style(theme::shortcut_pill);

    let open_file_btn = button(
        row![
            open_icon,
            column![
                text("Apri file esistente")
                    .font(theme::ui_font_semibold())
                    .size(15)
                    .color(pal.text_primary),
                text("Apri un documento .tex, .bib o .sty dal tuo computer")
                    .size(12)
                    .color(pal.text_muted),
            ]
            .spacing(3),
            horizontal_space(),
            open_shortcut,
        ]
        .spacing(14)
        .align_y(Alignment::Center)
        .padding([12, 14]),
    )
    .width(Length::Fill)
    .style(theme::action_card_button)
    .on_press(Message::Open);

    let mut card_content = column![
        header,
        horizontal_rule(1).style(theme::subtle_rule),
        column![new_file_btn, open_file_btn]
            .spacing(10)
            .width(Length::Fill),
    ]
    .spacing(20)
    .padding(34)
    .width(Length::Fixed(548.0))
    .align_x(Alignment::Center);

    if !app.recent_files.is_empty() {
        let mut recent_col = Column::new().spacing(4).width(Length::Fill);
        for path in &app.recent_files {
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("documento.tex")
                .to_string();
            let dir_str = path
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("")
                .to_string();

            let item = button(
                row![
                    text("•")
                        .shaping(Shaping::Advanced)
                        .size(13)
                        .color(pal.accent),
                    text(file_name)
                        .font(theme::ui_font_semibold())
                        .size(13)
                        .color(pal.text_primary),
                    horizontal_space(),
                    text(dir_str).size(11).color(pal.text_faint),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .padding([7, 10]),
            )
            .width(Length::Fill)
            .style(theme::ghost_button)
            .on_press(Message::OpenRecent(path.clone()));

            recent_col = recent_col.push(item);
        }

        card_content = card_content
            .push(horizontal_rule(1).style(theme::subtle_rule))
            .push(
                column![
                    text("FILE RECENTI")
                        .font(theme::ui_font_semibold())
                        .size(11)
                        .align_x(Alignment::Center)
                        .width(Length::Fill)
                        .color(pal.text_faint),
                    scrollable(recent_col)
                        .direction(scrollable::Direction::Vertical(thin_scrollbar()))
                        .style(theme::custom_scrollable)
                        .width(Length::Fill)
                        .height(Length::Shrink),
                ]
                .spacing(8)
                .width(Length::Fill)
                .align_x(Alignment::Center),
            );
    }

    if !app.status.is_empty() && app.status != "Pronto" {
        card_content = card_content.push(
            text(&app.status)
                .size(12)
                .align_x(Alignment::Center)
                .width(Length::Fill)
                .color(pal.text_muted),
        );
    }

    card_content = card_content.push(
        text("Ctrl+N Nuovo  ·  Ctrl+O Apri  ·  Ctrl+S Salva  ·  Ctrl+B Compila")
            .size(11)
            .align_x(Alignment::Center)
            .width(Length::Fill)
            .color(pal.text_faint),
    );

    let card = container(card_content).style(theme::welcome_card);

    let centered_card = container(card).center(Length::Fill).padding(24);

    let theme_overlay = container(
        row![
            horizontal_space(),
            button(text(theme_label).size(12))
                .padding([6, 12])
                .style(theme::ghost_button)
                .on_press(Message::ToggleTheme),
        ]
        .align_y(Alignment::Center),
    )
    .padding([14, 20])
    .width(Length::Fill);

    container(stack![centered_card, theme_overlay])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::app_background)
        .into()
}

fn view_editor(app: &App) -> Element<'_, Message> {
    let pal = theme::palette_for(app.dark_mode);
    let tab = app.tab();

    let compile_label = if app.is_compiling {
        "Compilazione..."
    } else {
        "Compila"
    };

    let compile_btn = if app.is_compiling {
        button(text(compile_label).font(theme::ui_font_semibold()).size(13))
            .padding([6, 15])
            .style(theme::primary_button)
    } else {
        button(text(compile_label).font(theme::ui_font_semibold()).size(13))
            .padding([6, 15])
            .style(theme::primary_button)
            .on_press(Message::Compile)
    };

    let brand_pill = container(
        text("Fe")
            .font(theme::ui_font_bold())
            .size(11)
            .color(Color::WHITE),
    )
    .padding([2, 6])
    .style(theme::small_brand_badge);

    let compact_bar = app.window_size.width < 1040.0;
    let bar_btn_pad: [u16; 2] = if compact_bar { [4, 7] } else { [5, 10] };
    let bar_spacing: u16 = if compact_bar { 3 } else { 5 };
    let bar_h_pad: u16 = if compact_bar { 10 } else { 14 };

    let active_recipe = app.recipe_config.active_recipe();
    let recipe_btn_label = if app.recipe_config.shell_escape {
        format!("{} · SE ▾", active_recipe.name)
    } else {
        format!("{} ▾", active_recipe.name)
    };
    let recipe_btn = button(
        text(recipe_btn_label)
            .font(theme::math_symbol_font())
            .shaping(Shaping::Advanced)
            .size(12),
    )
    .padding(bar_btn_pad)
    .style(theme::toggle_toolbar_button(app.show_recipe_menu))
    .on_press(Message::ToggleRecipeMenu);

    let mut brand_row = row![brand_pill].spacing(7).align_y(Alignment::Center);
    if app.window_size.width >= 920.0 {
        brand_row = brand_row.push(
            text("FeatherTex")
                .font(theme::ui_font_semibold())
                .size(13)
                .color(pal.text_primary),
        );
    }

    let brand_btn = button(brand_row)
        .padding([4, 8])
        .style(theme::ghost_button)
        .on_press(Message::GoHome);

    let theme_btn_label = if app.dark_mode { "Chiaro" } else { "Scuro" };

    let log_btn_label = if tab.lint_warnings.is_empty() {
        "Log".to_string()
    } else {
        format!("Log ({})", tab.lint_warnings.len())
    };

    let top_bar = container(
        row![
            brand_btn,
            horizontal_space(),
            button(text("Nuovo").size(13))
                .padding(bar_btn_pad)
                .style(theme::ghost_button)
                .on_press(Message::New),
            button(text("Apri").size(13))
                .padding(bar_btn_pad)
                .style(theme::ghost_button)
                .on_press(Message::Open),
            button(text("Salva").size(13))
                .padding(bar_btn_pad)
                .style(theme::ghost_button)
                .on_press(Message::Save),
            container(vertical_rule(1).style(theme::subtle_rule))
                .height(Length::Fixed(18.0))
                .padding([0, 4]),
            button(text("Struttura").size(13))
                .padding(bar_btn_pad)
                .style(theme::toggle_toolbar_button(app.show_outline))
                .on_press(Message::ToggleOutline),
            button(text("Asset").size(13))
                .padding(bar_btn_pad)
                .style(theme::toggle_toolbar_button(app.show_assets))
                .on_press(Message::ToggleAssets),
            button(text("Simboli").size(13))
                .padding(bar_btn_pad)
                .style(theme::toggle_toolbar_button(app.show_symbols))
                .on_press(Message::ToggleSymbols),
            button(text("Match").size(13))
                .padding(bar_btn_pad)
                .style(theme::toggle_toolbar_button(app.show_match))
                .on_press(Message::ToggleMatch),
            button(text("Cerca").size(13))
                .padding(bar_btn_pad)
                .style(theme::toggle_toolbar_button(app.show_find_bar))
                .on_press(if app.show_find_bar {
                    Message::CloseFindBar
                } else {
                    Message::OpenFindBar { replace: false }
                }),
            button(text("PDF").size(13))
                .padding(bar_btn_pad)
                .style(theme::toggle_toolbar_button(app.show_pdf))
                .on_press(Message::TogglePdf),
            button(text(log_btn_label).size(13))
                .padding(bar_btn_pad)
                .style(theme::toggle_toolbar_button(app.show_log))
                .on_press(Message::ToggleLog),
            button(text(theme_btn_label).size(12))
                .padding(bar_btn_pad)
                .style(theme::ghost_button)
                .on_press(Message::ToggleTheme),
            button(
                row![
                    text("●")
                        .font(theme::math_symbol_font())
                        .shaping(Shaping::Advanced)
                        .size(8)
                        .color(if app.is_live_compiling {
                            pal.accent
                        } else if tab.staged_build.is_some() {
                            pal.success
                        } else if app.live_compile {
                            pal.text_muted
                        } else {
                            pal.text_faint
                        }),
                    text("Pre-compila").size(12),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding(bar_btn_pad)
            .style(theme::toggle_toolbar_button(app.live_compile))
            .on_press(Message::ToggleLiveCompile),
            Space::with_width(if compact_bar { 2 } else { 4 }),
            recipe_btn,
            compile_btn,
        ]
        .spacing(bar_spacing)
        .align_y(Alignment::Center),
    )
    .padding([7, bar_h_pad])
    .width(Length::Fill)
    .clip(true)
    .style(theme::surface_bar);

    let file_tab_bar = view_file_tab_bar(app);

    let editor_area = view_editor_canvas(app);

    let mut workspace = Row::new().height(Length::Fill);

    if app.show_outline {
        workspace = workspace
            .push(view_outline_panel(app))
            .push(vertical_rule(1).style(theme::hairline_rule));
    } else if app.show_assets {
        workspace = workspace
            .push(view_assets_panel(app))
            .push(vertical_rule(1).style(theme::hairline_rule));
    } else if app.show_match {
        workspace = workspace
            .push(view_match_panel(app))
            .push(vertical_rule(1).style(theme::hairline_rule));
    } else if app.show_symbols {
        workspace = workspace
            .push(view_symbol_palette(app))
            .push(vertical_rule(1).style(theme::hairline_rule));
    }

    workspace = workspace.push(editor_area);

    if app.show_pdf {
        workspace = workspace
            .push(vertical_rule(1).style(theme::hairline_rule))
            .push(view_pdf_panel(app));
    } else if app.show_log {
        workspace = workspace
            .push(vertical_rule(1).style(theme::hairline_rule))
            .push(view_log_panel(app));
    }

    let (cursor_line, cursor_col) = tab.doc.content.cursor_position();
    let total_lines = tab.doc.line_count();

    let (dot_color, dot_bg) = if app.is_compiling {
        (pal.accent, pal.accent_soft)
    } else {
        match tab.last_compile_ok {
            Some(true) => (pal.success, pal.success_soft),
            Some(false) => (pal.error, pal.error_soft),
            None => (pal.text_muted, pal.bg_elevated),
        }
    };

    let status_badge = container(
        row![
            text("●")
                .font(theme::math_symbol_font())
                .shaping(Shaping::Advanced)
                .size(9)
                .color(dot_color),
            text(&app.status).size(12).color(pal.text_primary),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([2, 9])
    .style(theme::status_pill(dot_color, dot_bg));

    let auto_close_label = if app.auto_close {
        "Auto-chiusura: ON"
    } else {
        "Auto-chiusura: OFF"
    };

    let word_count_label = match tab.word_count {
        Some(wc) => format!("{} parole", wc.total_words()),
        None => "Parole...".to_string(),
    };

    let mut status_items = row![
        status_badge,
        horizontal_space(),
        button(
            text(word_count_label)
                .font(theme::mono_font())
                .size(11),
        )
        .padding([2, 7])
        .style(theme::ghost_button)
        .on_press(Message::RefreshWordCount),
        button(
            text(format!(
                "Riga {}, Col {}  ·  {} righe",
                cursor_line + 1,
                cursor_col + 1,
                total_lines
            ))
            .font(theme::mono_font())
            .size(11),
        )
        .padding([2, 7])
        .style(theme::toggle_toolbar_button(app.show_goto_bar))
        .on_press(if app.show_goto_bar {
            Message::CloseGotoLine
        } else {
            Message::OpenGotoLine
        }),
        button(text("% Commenta").size(11))
            .padding([2, 7])
            .style(theme::ghost_button)
            .on_press(Message::ToggleComment),
        container(vertical_rule(1).style(theme::subtle_rule))
            .height(Length::Fixed(14.0))
            .padding([0, 6]),
        button(text(auto_close_label).size(11))
            .padding([2, 8])
            .style(theme::toggle_toolbar_button(app.auto_close))
            .on_press(Message::ToggleAutoClose),
        container(vertical_rule(1).style(theme::subtle_rule))
            .height(Length::Fixed(14.0))
            .padding([0, 6]),
        button(text("A-").size(11))
            .padding([2, 6])
            .style(theme::ghost_button)
            .on_press(Message::ZoomOutEditor),
        button(text(format!("{}px", app.effective_editor_font_size() as i32)).size(11))
            .padding([2, 6])
            .style(theme::ghost_button)
            .on_press(Message::ResetZoomEditor),
        button(text("A+").size(11))
            .padding([2, 6])
            .style(theme::ghost_button)
            .on_press(Message::ZoomInEditor),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    if app.window_size.width >= 980.0 {
        status_items = status_items.push(
            text("  ·  Ctrl+S  ·  Ctrl+B")
                .size(11)
                .color(pal.text_faint),
        );
    }

    let status_bar = container(status_items)
        .padding([6, if compact_bar { 10 } else { 14 }])
        .width(Length::Fill)
        .clip(true)
        .style(theme::surface_bar);

    let main_layout = column![
        top_bar,
        horizontal_rule(1).style(theme::subtle_rule),
        file_tab_bar,
        horizontal_rule(1).style(theme::hairline_rule),
        workspace,
        horizontal_rule(1).style(theme::hairline_rule),
        status_bar,
    ];

    let overlay: Element<'_, Message> = if let Some(menu) = app.pdf_context_menu {
        view_pdf_context_menu(app, menu)
    } else if app.show_folder_menu {
        view_folder_menu_overlay(app)
    } else if app.show_recipe_menu {
        view_recipe_menu_overlay(app)
    } else {
        container(Space::new(0, 0)).into()
    };

    // Always keep `stack!` as the root so opening/closing overlays never resets
    // scrollable offsets or editor focus state inside `main_layout`.
    stack![main_layout, overlay].into()
}

fn view_file_tab_bar(app: &App) -> Element<'_, Message> {
    let pal = theme::palette_for(app.dark_mode);

    let folder_label = app
        .tab()
        .doc
        .path
        .as_ref()
        .and_then(|p| p.parent())
        .and_then(|d| d.file_name())
        .and_then(|n| n.to_str())
        .map(|n| format!("{n} ▾"))
        .unwrap_or_else(|| "File ▾".to_string());

    let folder_btn = button(
        text(folder_label)
            .font(theme::math_symbol_font())
            .shaping(Shaping::Advanced)
            .size(12),
    )
    .padding([4, 10])
    .style(theme::toggle_toolbar_button(app.show_folder_menu))
    .on_press(Message::ToggleFolderMenu);

    let mut tabs_row = Row::new().spacing(5).align_y(Alignment::Center);

    for (idx, t) in app.tabs.iter().enumerate() {
        let is_active = idx == app.active_tab;
        let has_error = !t.error_lines.is_empty();

        let mut label_row = row![text(t.doc.file_name())
            .font(if is_active {
                theme::ui_font_semibold()
            } else {
                theme::ui_font()
            })
            .size(12)
            .color(if has_error {
                pal.error
            } else if is_active {
                pal.text_primary
            } else {
                pal.text_muted
            })]
        .spacing(5)
        .align_y(Alignment::Center);

        if t.doc.is_dirty {
            label_row = label_row.push(
                text("●")
                    .font(theme::math_symbol_font())
                    .shaping(Shaping::Advanced)
                    .size(8)
                    .color(pal.accent),
            );
        }

        let select_btn = button(label_row)
            .padding([3, 8])
            .style(theme::file_tab_select_button(is_active))
            .on_press(Message::SelectTab(idx));

        let close_btn = button(
            text("×")
                .font(theme::ui_font_bold())
                .shaping(Shaping::Advanced)
                .size(13)
                .color(pal.text_faint),
        )
        .padding([1, 5])
        .style(theme::ghost_button)
        .on_press(Message::CloseTab(idx));

        let tab_chip = container(
            row![select_btn, close_btn]
                .spacing(1)
                .align_y(Alignment::Center),
        )
        .padding([2, 3])
        .style(theme::file_tab_card(is_active));

        tabs_row = tabs_row.push(tab_chip);
    }

    let add_tab_btn = button(
        text("+")
            .font(theme::ui_font_bold())
            .size(14)
            .color(pal.text_muted),
    )
    .padding([2, 8])
    .style(theme::ghost_button)
    .on_press(Message::New);

    tabs_row = tabs_row.push(add_tab_btn);

    let tabs_scroll = scrollable(tabs_row)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new()
                .width(4)
                .margin(0)
                .scroller_width(4),
        ))
        .style(theme::custom_scrollable)
        .width(Length::Fill);

    let master_widget: Element<'_, Message> = if let Some(master_path) = app.effective_master_path()
    {
        let master_name = master_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("main.tex")
            .to_string();
        let has_pinned = app.master_tex_path.is_some();
        let mut r = row![
            button(
                text(format!("Master: {master_name}"))
                    .font(theme::ui_font_semibold())
                    .size(11)
                    .color(pal.accent),
            )
            .padding([2, 7])
            .style(theme::suggestion_row(true))
            .on_press(Message::OutlineJump {
                file_path: Some(master_path),
                line_1based: 1,
            }),
        ]
        .spacing(2)
        .align_y(Alignment::Center);
        if has_pinned {
            r = r.push(
                button(text("×").size(12).color(pal.text_muted))
                    .padding([1, 5])
                    .style(theme::ghost_button)
                    .on_press(Message::ClearMaster),
            );
        }
        r.into()
    } else if app.tab().doc.path.is_some() {
        button(text("Fissa Master").size(11))
            .padding([3, 8])
            .style(theme::ghost_button)
            .on_press(Message::TogglePinMaster)
            .into()
    } else {
        Space::with_width(0).into()
    };

    container(
        row![
            folder_btn,
            container(vertical_rule(1).style(theme::subtle_rule))
                .height(Length::Fixed(18.0))
                .padding([0, 4]),
            tabs_scroll,
            master_widget,
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([5, 14])
    .width(Length::Fill)
    .clip(true)
    .style(theme::file_tab_strip)
    .into()
}

fn view_folder_menu_overlay(app: &App) -> Element<'_, Message> {
    let pal = theme::palette_for(app.dark_mode);
    let mut list = Column::new().spacing(4).padding(Padding {
        top: 2.0,
        right: 4.0,
        bottom: 4.0,
        left: 0.0,
    });

    list = list.push(
        text("FILE APERTI")
            .font(theme::ui_font_semibold())
            .size(10)
            .color(pal.text_faint),
    );

    for (idx, t) in app.tabs.iter().enumerate() {
        let is_active = idx == app.active_tab;
        let marker = if is_active { "✓" } else { "•" };
        let marker_color = if is_active {
            pal.accent
        } else {
            pal.text_faint
        };
        let mut r = row![
            text(marker)
                .font(theme::math_symbol_font())
                .shaping(Shaping::Advanced)
                .size(11)
                .color(marker_color),
            text(t.doc.file_name())
                .font(if is_active {
                    theme::ui_font_semibold()
                } else {
                    theme::ui_font()
                })
                .size(12)
                .color(pal.text_primary),
            horizontal_space(),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        if t.doc.is_dirty {
            r = r.push(
                text("modificato")
                    .size(10)
                    .color(pal.accent),
            );
        }

        list = list.push(
            button(r)
                .padding([6, 10])
                .width(Length::Fill)
                .style(theme::context_menu_item)
                .on_press(Message::SelectTab(idx)),
        );
    }

    if !app.folder_files.is_empty() {
        let dir_name = app
            .tab()
            .doc
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .and_then(|d| d.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("Cartella");
        list = list
            .push(Space::with_height(4))
            .push(horizontal_rule(1).style(theme::subtle_rule))
            .push(
                text(format!("CARTELLA · {}", dir_name.to_uppercase()))
                    .font(theme::ui_font_semibold())
                    .size(10)
                    .color(pal.text_faint),
            );

        for path in &app.folder_files {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file.tex")
                .to_string();
            let is_open = app
                .tabs
                .iter()
                .any(|t| t.doc.path.as_deref() == Some(path.as_path()));
            let mut r = row![
                text(name)
                    .font(theme::mono_font())
                    .size(12)
                    .color(pal.text_primary),
                horizontal_space(),
            ]
            .align_y(Alignment::Center);

            if is_open {
                r = r.push(text("aperto").size(10).color(pal.success));
            }

            list = list.push(
                button(r)
                    .padding([5, 10])
                    .width(Length::Fill)
                    .style(theme::context_menu_item)
                    .on_press(Message::OpenRecent(path.clone())),
            );
        }
    }

    if !app.recent_files.is_empty() {
        list = list
            .push(Space::with_height(4))
            .push(horizontal_rule(1).style(theme::subtle_rule))
            .push(
                text("FILE RECENTI")
                    .font(theme::ui_font_semibold())
                    .size(10)
                    .color(pal.text_faint),
            );

        for path in &app.recent_files {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("documento.tex")
                .to_string();
            list = list.push(
                button(
                    row![
                        text(name).size(12).color(pal.text_primary),
                        horizontal_space(),
                        text("↗")
                            .font(theme::math_symbol_font())
                            .shaping(Shaping::Advanced)
                            .size(11)
                            .color(pal.text_faint),
                    ]
                    .align_y(Alignment::Center),
                )
                .padding([5, 10])
                .width(Length::Fill)
                .style(theme::context_menu_item)
                .on_press(Message::OpenRecent(path.clone())),
            );
        }
    }

    let menu_card = container(
        column![
            scrollable(list)
                .direction(scrollable::Direction::Vertical(thin_scrollbar()))
                .style(theme::custom_scrollable)
                .width(Length::Fill)
                .height(Length::Shrink),
            horizontal_rule(1).style(theme::subtle_rule),
            row![
                button(text("+ Nuovo").size(12))
                    .padding([5, 10])
                    .width(Length::Fill)
                    .style(theme::ghost_button)
                    .on_press(Message::New),
                button(text("Apri dal disco...").size(12))
                    .padding([5, 10])
                    .width(Length::Fill)
                    .style(theme::ghost_button)
                    .on_press(Message::Open),
            ]
            .spacing(6),
        ]
        .spacing(8)
        .padding(10),
    )
    .width(Length::Fixed(310.0))
    .max_height(420.0)
    .style(theme::popup_box);

    let dismiss_backdrop = mouse_area(Space::new(Length::Fill, Length::Fill))
        .on_press(Message::DismissOverlays)
        .on_right_press(Message::DismissOverlays);

    let positioned = container(menu_card).padding(Padding {
        top: 84.0,
        left: 14.0,
        right: 0.0,
        bottom: 0.0,
    });

    stack![dismiss_backdrop, positioned].into()
}

fn view_recipe_menu_overlay(app: &App) -> Element<'_, Message> {
    let pal = theme::palette_for(app.dark_mode);
    let active_id = app.recipe_config.active_recipe().id;
    let recipes_list = app.recipe_config.all_recipes();
    let magic = recipes::parse_magic_comments(&app.tab().doc.text());

    let mut content = Column::new().spacing(5);
    content = content.push(
        row![
            text("RICETTE DI COMPILAZIONE")
                .font(theme::ui_font_semibold())
                .size(10)
                .color(pal.text_faint),
            horizontal_space(),
            text("Ctrl+B")
                .font(theme::mono_font())
                .size(10)
                .color(pal.text_faint),
        ]
        .align_y(Alignment::Center),
    );

    if magic.root.is_some() || magic.program.is_some() || magic.bib_program.is_some() {
        let mut parts = Vec::new();
        if let Some(r) = &magic.root {
            parts.push(format!("root: {r}"));
        }
        if let Some(p) = &magic.program {
            parts.push(format!("program: {p}"));
        }
        if let Some(b) = &magic.bib_program {
            parts.push(format!("bib: {b}"));
        }
        content = content.push(
            container(
                text(format!("Magic comments attivi: {}", parts.join(" · ")))
                    .size(11)
                    .color(pal.accent),
            )
            .padding([5, 8])
            .width(Length::Fill)
            .style(theme::log_inset),
        );
    }

    let mut list_col = Column::new().spacing(3);
    for recipe in &recipes_list {
        let is_active = recipe.id == active_id;
        let mark = if is_active { "✓" } else { " " };
        let summary = recipe.steps_summary();

        let select_btn = button(
            row![
                text(mark)
                    .font(theme::math_symbol_font())
                    .shaping(Shaping::Advanced)
                    .size(12)
                    .color(pal.accent)
                    .width(Length::Fixed(16.0)),
                column![
                    text(recipe.name.clone())
                        .font(if is_active {
                            theme::ui_font_semibold()
                        } else {
                            theme::ui_font()
                        })
                        .size(12)
                        .color(if is_active {
                            pal.accent
                        } else {
                            pal.text_primary
                        }),
                    text(summary)
                        .font(theme::mono_font())
                        .size(10)
                        .color(pal.text_faint),
                ]
                .spacing(1)
                .width(Length::Fill),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([6, 8])
        .width(Length::Fill)
        .style(theme::suggestion_row(is_active))
        .on_press(Message::SelectRecipe(recipe.id.clone()));

        if recipe.builtin {
            list_col = list_col.push(select_btn);
        } else {
            let del_btn = button(text("×").size(13).color(pal.text_muted))
                .padding([6, 8])
                .style(theme::ghost_button)
                .on_press(Message::DeleteCustomRecipe(recipe.id.clone()));
            list_col = list_col.push(
                row![select_btn, del_btn]
                    .spacing(4)
                    .align_y(Alignment::Center),
            );
        }
    }

    content = content.push(
        container(
            scrollable(list_col)
                .direction(scrollable::Direction::Vertical(thin_scrollbar()))
                .style(theme::custom_scrollable)
                .width(Length::Fill)
                .height(Length::Shrink),
        )
        .max_height(240.0)
        .width(Length::Fill),
    );

    content = content.push(horizontal_rule(1).style(theme::subtle_rule));

    let shell_badge = if app.recipe_config.shell_escape {
        "ON"
    } else {
        "OFF"
    };
    content = content.push(
        button(
            row![
                container(
                    text(shell_badge)
                        .font(theme::ui_font_semibold())
                        .size(10),
                )
                .padding([2, 6])
                .style(if app.recipe_config.shell_escape {
                    theme::installed_badge
                } else {
                    theme::shortcut_pill
                }),
                column![
                    text("--shell-escape")
                        .font(theme::mono_font())
                        .size(12)
                        .color(pal.text_primary),
                    text("Abilita minted, svg e externalize TikZ")
                        .size(10)
                        .color(pal.text_faint),
                ]
                .spacing(1),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([5, 8])
        .width(Length::Fill)
        .style(theme::context_menu_item)
        .on_press(Message::ToggleShellEscape),
    );

    content = content.push(horizontal_rule(1).style(theme::subtle_rule));

    let builder_toggle_label = if app.show_new_recipe_builder {
        "▾ Chiudi costruttore ricetta"
    } else {
        "+ Nuova ricetta personalizzata"
    };
    content = content.push(
        button(
            text(builder_toggle_label)
                .font(theme::math_symbol_font())
                .shaping(Shaping::Advanced)
                .size(12)
                .color(pal.accent),
        )
        .padding([5, 8])
        .width(Length::Fill)
        .style(theme::ghost_button)
        .on_press(Message::ToggleNewRecipeBuilder),
    );

    if app.show_new_recipe_builder {
        let seq_preview = if app.new_recipe_steps.is_empty() {
            "Seleziona i passi in sequenza qui sotto...".to_string()
        } else {
            app.new_recipe_steps
                .iter()
                .map(|s| s.label())
                .collect::<Vec<_>>()
                .join(" → ")
        };

        let can_save =
            !app.new_recipe_name.trim().is_empty() && !app.new_recipe_steps.is_empty();

        let step_btn = |step: RecipeStep| {
            button(text(format!("+ {}", step.short_label())).size(11))
                .padding([4, 7])
                .style(theme::symbol_button)
                .on_press(Message::NewRecipeAddStep(step))
        };

        let mut save_btn = button(
            text("Salva ricetta")
                .font(theme::ui_font_semibold())
                .size(12),
        )
        .padding([6, 10])
        .style(theme::primary_button);
        if can_save {
            save_btn = save_btn.on_press(Message::SaveCustomRecipe);
        }

        content = content.push(
            container(
                column![
                    text_input("Nome ricetta (es. Tesi completa)", &app.new_recipe_name)
                        .on_input(Message::NewRecipeNameChanged)
                        .size(12)
                        .padding([6, 8]),
                    container(
                        text(seq_preview)
                            .font(theme::mono_font())
                            .size(10)
                            .color(pal.text_muted),
                    )
                    .padding([5, 7])
                    .width(Length::Fill)
                    .style(theme::log_inset),
                    row![
                        step_btn(RecipeStep::PdfLatex),
                        step_btn(RecipeStep::LuaLatex),
                        step_btn(RecipeStep::XeLatex),
                        step_btn(RecipeStep::BibTex),
                    ]
                    .spacing(4),
                    row![
                        step_btn(RecipeStep::Biber),
                        step_btn(RecipeStep::MakeGlossaries),
                        step_btn(RecipeStep::MakeIndex),
                        step_btn(RecipeStep::LatexMkPdf),
                    ]
                    .spacing(4),
                    row![
                        step_btn(RecipeStep::LatexMkLua),
                        step_btn(RecipeStep::LatexMkXe),
                        horizontal_space(),
                        button(text("Indietro").size(11))
                            .padding([4, 7])
                            .style(theme::ghost_button)
                            .on_press(Message::NewRecipePopStep),
                        button(text("Svuota").size(11))
                            .padding([4, 7])
                            .style(theme::ghost_button)
                            .on_press(Message::NewRecipeClearSteps),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                    row![horizontal_space(), save_btn],
                ]
                .spacing(6),
            )
            .padding(8)
            .width(Length::Fill)
            .style(theme::log_inset),
        );
    }

    let menu_card = container(content.padding(10))
        .width(Length::Fixed(356.0))
        .max_height(560.0)
        .style(theme::popup_box);

    let dismiss_backdrop = mouse_area(Space::new(Length::Fill, Length::Fill))
        .on_press(Message::DismissOverlays)
        .on_right_press(Message::DismissOverlays);

    let positioned = container(row![horizontal_space(), menu_card]).padding(Padding {
        top: 48.0,
        left: 14.0,
        right: 14.0,
        bottom: 0.0,
    });

    stack![dismiss_backdrop, positioned].into()
}

fn view_pdf_context_menu(app: &App, menu: PdfContextMenu) -> Element<'_, Message> {
    let pal = theme::palette_for(app.dark_mode);
    let menu_w = 248.0_f32;
    let menu_h = 112.0_f32;

    let left = menu
        .anchor
        .x
        .clamp(8.0, (app.window_size.width - menu_w - 8.0).max(8.0))
        .round();
    let top = menu
        .anchor
        .y
        .clamp(8.0, (app.window_size.height - menu_h - 8.0).max(8.0))
        .round();

    let popup = container(
        column![
            container(
                text(format!("Pagina {} · SyncTeX", menu.page))
                    .font(theme::ui_font_semibold())
                    .size(10)
                    .color(pal.text_faint),
            )
            .padding([3, 10]),
            horizontal_rule(1).style(theme::subtle_rule),
            button(
                row![
                    text("↗")
                        .font(theme::math_symbol_font())
                        .shaping(Shaping::Advanced)
                        .size(13)
                        .color(pal.accent),
                    text("Visualizza codice sorgente")
                        .font(theme::ui_font_semibold())
                        .size(13),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([7, 10])
            .width(Length::Fill)
            .style(theme::context_menu_item)
            .on_press(Message::PdfViewSource),
            button(
                text("Apri PDF esterno")
                    .size(12)
                    .color(pal.text_muted),
            )
            .padding([6, 10])
            .width(Length::Fill)
            .style(theme::context_menu_item)
            .on_press(Message::OpenPdfExternal),
        ]
        .spacing(3)
        .padding(6),
    )
    .width(Length::Fixed(menu_w))
    .style(theme::popup_box);

    let dismiss_backdrop = mouse_area(Space::new(Length::Fill, Length::Fill))
        .on_press(Message::DismissOverlays)
        .on_right_press(Message::DismissOverlays);

    let positioned = container(popup).padding(Padding {
        top,
        left,
        right: 0.0,
        bottom: 0.0,
    });

    stack![dismiss_backdrop, positioned].into()
}

fn view_editor_canvas(app: &App) -> Element<'_, Message> {
    let pal = theme::palette_for(app.dark_mode);
    let tab = app.tab();
    let has_suggestions = !app.suggestions.is_empty();
    let font_size = app.effective_editor_font_size();
    let line_height = editor_line_height(font_size);
    let line_count = tab.doc.line_count();
    let (cursor_line, _) = tab.doc.content.cursor_position();
    let gutter_w = gutter_width(line_count, font_size);

    let available_text_w = if app.editor_viewport_width > 50.0 {
        app.editor_viewport_width
    } else {
        editor_code_viewport_width(app)
    };
    let max_chars = tab.doc.max_line_chars();
    let needed_text_w = editor_needed_text_width(max_chars, font_size);
    let needs_horizontal_scroll = needed_text_w > available_text_w;

    // Virtualize fixed left line numbers: only materialize rows visible in viewport,
    // shifted vertically by `gutter_offset_y` so they stay pinned horizontally on the left.
    let scroll_y = tab.editor_scroll_offset.y.max(0.0);
    let viewport_h = app.editor_viewport_height.max(400.0);
    let first_visible = (((scroll_y - EDITOR_PADDING) / line_height).floor().max(0.0) as usize)
        .saturating_sub(GUTTER_BUFFER_LINES)
        .min(line_count);
    let visible_lines_count =
        ((viewport_h / line_height).ceil() as usize) + GUTTER_BUFFER_LINES * 2 + 2;
    let last_visible = (first_visible + visible_lines_count).min(line_count);
    let gutter_offset_y = EDITOR_PADDING + (first_visible as f32) * line_height - scroll_y;

    let num_font_size = (font_size * 0.85).round().max(11.0);
    let mut line_nums = Column::new().spacing(0);

    for idx in first_visible..last_visible {
        let is_error = tab.error_lines.contains(&idx);
        let is_active = idx == cursor_line || tab.sync_highlight_line == Some(idx);
        let num_text = text(format!("{}", idx + 1))
            .font(if is_error || is_active {
                theme::mono_font_bold()
            } else {
                theme::mono_font()
            })
            .size(num_font_size)
            .color(if is_error {
                pal.error
            } else if is_active {
                pal.accent
            } else {
                pal.text_faint
            });

        let mut cell = container(num_text)
            .width(Length::Fill)
            .height(Length::Fixed(line_height))
            .align_x(Alignment::End)
            .align_y(Alignment::Center)
            .padding(Padding {
                top: 0.0,
                right: 10.0,
                bottom: 0.0,
                left: 6.0,
            });

        if is_error {
            cell = cell.style(theme::gutter_error_line);
        } else if is_active {
            cell = cell.style(theme::gutter_active_line);
        }

        line_nums = line_nums.push(cell);
    }

    let gutter_rows: Element<'_, Message> = container(line_nums)
        .width(Length::Fill)
        .padding([0, 4])
        .into();

    let fixed_gutter = container(VerticalOffset::new(gutter_rows, gutter_offset_y))
        .width(Length::Fixed(gutter_w))
        .height(Length::Fill)
        .style(theme::gutter_background);

    let has_selection = tab
        .doc
        .content
        .selection()
        .is_some_and(|s| !s.is_empty());

    let mut editor_widget = text_editor(&tab.doc.content)
        .on_action(Message::EditorAction)
        .highlight_with::<LatexHighlighter>(app.dark_mode, Token::to_format)
        .font(theme::mono_font())
        .size(font_size)
        .line_height(LineHeight::Absolute(line_height.into()))
        .wrapping(Wrapping::None)
        .style(theme::editor_style)
        .padding(Padding {
            top: EDITOR_PADDING,
            right: 24.0,
            bottom: EDITOR_BOTTOM_BREATHING,
            left: EDITOR_PADDING,
        })
        .height(Length::Shrink)
        .key_binding(move |key_press| {
            if key_press.status == text_editor::Status::Focused {
                if key_press.modifiers.command() && !key_press.modifiers.alt() {
                    if matches!(key_press.key.as_ref(), keyboard::Key::Character("+" | "="))
                        || matches!(key_press.text.as_deref(), Some("+" | "="))
                    {
                        return Some(text_editor::Binding::Custom(Message::ZoomInEditor));
                    }
                    if matches!(key_press.key.as_ref(), keyboard::Key::Character("-" | "_"))
                        || matches!(key_press.text.as_deref(), Some("-" | "_"))
                    {
                        return Some(text_editor::Binding::Custom(Message::ZoomOutEditor));
                    }
                    if matches!(key_press.key.as_ref(), keyboard::Key::Character("0"))
                        || matches!(key_press.text.as_deref(), Some("0"))
                    {
                        return Some(text_editor::Binding::Custom(Message::ResetZoomEditor));
                    }
                    if matches!(key_press.key.as_ref(), keyboard::Key::Character("/"))
                        || matches!(key_press.text.as_deref(), Some("/"))
                        || (matches!(key_press.key.as_ref(), keyboard::Key::Character("7"))
                            && key_press.modifiers.shift())
                    {
                        return Some(text_editor::Binding::Custom(Message::ToggleComment));
                    }
                    match key_press.key.as_ref() {
                        keyboard::Key::Character("f" | "F") => {
                            return Some(text_editor::Binding::Custom(Message::OpenFindBar {
                                replace: false,
                            }));
                        }
                        keyboard::Key::Character("h" | "H") => {
                            return Some(text_editor::Binding::Custom(Message::OpenFindBar {
                                replace: true,
                            }));
                        }
                        keyboard::Key::Character("g" | "G") => {
                            return Some(text_editor::Binding::Custom(Message::OpenGotoLine));
                        }
                        keyboard::Key::Character("c" | "C") => {
                            return Some(text_editor::Binding::Custom(Message::EditorCopy));
                        }
                        keyboard::Key::Character("x" | "X") => {
                            return Some(text_editor::Binding::Custom(Message::EditorCut));
                        }
                        keyboard::Key::Character("v" | "V") => {
                            return Some(text_editor::Binding::Custom(Message::EditorPasteRequest));
                        }
                        keyboard::Key::Character("z" | "Z") => {
                            if key_press.modifiers.shift() {
                                return Some(text_editor::Binding::Custom(Message::EditorRedo));
                            } else {
                                return Some(text_editor::Binding::Custom(Message::EditorUndo));
                            }
                        }
                        keyboard::Key::Character("y" | "Y") => {
                            return Some(text_editor::Binding::Custom(Message::EditorRedo));
                        }
                        keyboard::Key::Character("a" | "A") => {
                            return Some(text_editor::Binding::SelectAll);
                        }
                        _ => {}
                    }
                } else if key_press.modifiers.shift()
                    && !key_press.modifiers.command()
                    && !key_press.modifiers.alt()
                    && matches!(
                        key_press.key.as_ref(),
                        keyboard::Key::Named(keyboard::key::Named::Tab)
                    )
                {
                    return Some(text_editor::Binding::Custom(Message::UnindentSelection));
                } else if key_press.modifiers.is_empty() {
                    if has_suggestions {
                        match key_press.key.as_ref() {
                            keyboard::Key::Named(keyboard::key::Named::Tab)
                            | keyboard::Key::Named(keyboard::key::Named::Enter) => {
                                return Some(text_editor::Binding::Custom(
                                    Message::AcceptSuggestion,
                                ));
                            }
                            keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                                return Some(text_editor::Binding::Custom(Message::NextSuggestion));
                            }
                            keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                                return Some(text_editor::Binding::Custom(Message::PrevSuggestion));
                            }
                            keyboard::Key::Named(keyboard::key::Named::Escape) => {
                                return Some(text_editor::Binding::Custom(
                                    Message::DismissSuggestions,
                                ));
                            }
                            _ => {}
                        }
                    } else if matches!(
                        key_press.key.as_ref(),
                        keyboard::Key::Named(keyboard::key::Named::Tab)
                    ) {
                        if has_selection {
                            return Some(text_editor::Binding::Custom(Message::IndentSelection));
                        }
                        return Some(text_editor::Binding::Insert('\t'));
                    }
                }
            }
            text_editor::Binding::from_key_press(key_press)
        });

    if needs_horizontal_scroll {
        editor_widget = editor_widget.width(needed_text_w);
    }

    let scroll_direction = if needs_horizontal_scroll {
        scrollable::Direction::Both {
            vertical: thin_scrollbar(),
            horizontal: thin_scrollbar(),
        }
    } else {
        scrollable::Direction::Vertical(thin_scrollbar())
    };

    // Keep `editor_widget` directly inside `editor_scroll` (NEVER inside a fixed-height `stack!`)
    // so `scrollable` passes infinite height limits to `TextEditor`, allowing mouse wheel
    // events to scroll `editor_scroll` smoothly while keeping the left gutter fixed.
    let editor_scroll = scrollable(editor_widget)
        .id(app.editor_scroll_id.clone())
        .direction(scroll_direction)
        .on_scroll(Message::EditorScrolled)
        .style(theme::custom_scrollable)
        .width(Length::Fill)
        .height(Length::Fill);

    // Render error / reverse-sync line highlight bands BEHIND `editor_scroll` in viewport space
    // so they never interfere with `TextEditor` scroll delegation or focus state.
    let highlight_layer: Element<'_, Message> = {
        let mut vis_highlights: Vec<(usize, bool)> = tab
            .error_lines
            .iter()
            .copied()
            .filter(|&l| l >= first_visible && l < last_visible)
            .map(|l| (l, true))
            .collect();

        if let Some(sync_l) = tab.sync_highlight_line {
            if sync_l >= first_visible
                && sync_l < last_visible
                && !tab.error_lines.contains(&sync_l)
            {
                vis_highlights.push((sync_l, false));
            }
        }
        vis_highlights.sort_unstable_by_key(|&(l, _)| l);

        if vis_highlights.is_empty() {
            container(Space::new(0, 0)).into()
        } else {
            let mut bands = Column::new().spacing(0);
            let mut cur_y = 0.0_f32;
            for (hl_line, is_error) in vis_highlights {
                let raw_top = EDITOR_PADDING + (hl_line as f32) * line_height - scroll_y;
                let raw_bottom = raw_top + line_height;
                if raw_bottom <= 0.0 || raw_top >= viewport_h {
                    continue;
                }
                let clipped_top = raw_top.max(0.0);
                let clipped_h = (raw_bottom.min(viewport_h) - clipped_top).max(0.0);
                if clipped_top > cur_y {
                    bands = bands.push(Space::with_height(Length::Fixed(clipped_top - cur_y)));
                }
                let style_fn = if is_error {
                    theme::editor_error_line_band
                } else {
                    theme::editor_sync_line_band
                };
                bands = bands.push(
                    container(Space::new(Length::Fill, Length::Fixed(clipped_h)))
                        .width(Length::Fill)
                        .height(Length::Fixed(clipped_h))
                        .style(style_fn),
                );
                cur_y = clipped_top + clipped_h;
            }
            container(bands)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        }
    };

    let popup_overlay: Element<'_, Message> = if has_suggestions {
        view_suggestion_popup(app, font_size, line_height, gutter_w)
    } else {
        container(Space::new(0, 0)).into()
    };

    let editor_code_area = container(stack![
        container(Space::new(Length::Fill, Length::Fill)).style(theme::app_background),
        highlight_layer,
        EditorDragScrollArea::new(editor_scroll.into(), app.editor_dragging),
    ])
    .width(Length::Fill)
    .height(Length::Fill)
    .clip(true);

    let editor_split = row![
        fixed_gutter,
        vertical_rule(1).style(theme::subtle_rule),
        editor_code_area,
    ]
    .width(Length::Fill)
    .height(Length::Fill);

    let top_tool_slot: Element<'_, Message> = if app.show_goto_bar {
        view_goto_bar(app)
    } else if app.show_find_bar {
        view_find_replace_bar(app)
    } else {
        Space::new(0, 0).into()
    };

    let editor_body: Element<'_, Message> = container(stack![editor_split, popup_overlay])
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .into();

    column![top_tool_slot, editor_body]
        .width(Length::FillPortion(3))
        .height(Length::Fill)
        .into()
}

fn view_goto_bar(app: &App) -> Element<'_, Message> {
    let pal = theme::palette_for(app.dark_mode);
    let max_lines = app.tab().doc.line_count();

    let bar = container(
        row![
            text("Vai a riga:")
                .font(theme::ui_font_semibold())
                .size(12)
                .color(pal.text_primary),
            text_input("Numero riga...", &app.goto_line_input)
                .id(app.goto_input_id.clone())
                .on_input(Message::GotoLineChanged)
                .on_submit(Message::GotoLineSubmit)
                .font(theme::mono_font())
                .size(12)
                .padding([4, 8])
                .width(Length::Fixed(96.0)),
            text(format!("(1..{max_lines})"))
                .font(theme::mono_font())
                .size(11)
                .color(pal.text_faint),
            button(
                text("Salta")
                    .font(theme::ui_font_semibold())
                    .size(12),
            )
            .padding([4, 10])
            .style(theme::primary_button)
            .on_press(Message::GotoLineSubmit),
            horizontal_space(),
            close_panel_button(Message::CloseGotoLine),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([6, 12])
    .width(Length::Fill)
    .style(theme::surface_bar);

    column![bar, horizontal_rule(1).style(theme::subtle_rule)].into()
}

fn view_find_replace_bar(app: &App) -> Element<'_, Message> {
    let pal = theme::palette_for(app.dark_mode);

    let count_label = if let Some(err) = &app.find_error {
        err.clone()
    } else if app.find_query.is_empty() {
        String::new()
    } else if app.find_matches.is_empty() {
        "0 ris.".to_string()
    } else {
        let cur = app.find_active_idx.map(|i| i + 1).unwrap_or(1);
        format!("{cur}/{}", app.find_matches.len())
    };

    let count_color = if app.find_error.is_some()
        || (!app.find_query.is_empty() && app.find_matches.is_empty())
    {
        pal.error
    } else {
        pal.text_muted
    };

    let expand_glyph = if app.show_replace_row { "▾" } else { "▸" };

    let find_row = row![
        button(
            text(expand_glyph)
                .font(theme::math_symbol_font())
                .shaping(Shaping::Advanced)
                .size(12),
        )
        .padding([3, 6])
        .style(theme::ghost_button)
        .on_press(Message::ToggleReplaceRow),
        text_input("Trova nel documento...", &app.find_query)
            .id(app.find_input_id.clone())
            .on_input(Message::FindQueryChanged)
            .on_submit(Message::FindNext)
            .size(12)
            .padding([5, 8])
            .width(Length::Fill),
        text(count_label)
            .font(theme::mono_font())
            .size(11)
            .color(count_color),
        button(text("▲").font(theme::math_symbol_font()).shaping(Shaping::Advanced).size(11))
            .padding([4, 7])
            .style(theme::ghost_button)
            .on_press(Message::FindPrev),
        button(text("▼").font(theme::math_symbol_font()).shaping(Shaping::Advanced).size(11))
            .padding([4, 7])
            .style(theme::ghost_button)
            .on_press(Message::FindNext),
        button(text("Aa").font(theme::mono_font_bold()).size(11))
            .padding([4, 7])
            .style(theme::toggle_toolbar_button(app.find_case_sensitive))
            .on_press(Message::ToggleFindCase),
        button(text(".*").font(theme::mono_font_bold()).size(11))
            .padding([4, 7])
            .style(theme::toggle_toolbar_button(app.find_use_regex))
            .on_press(Message::ToggleFindRegex),
        button(text("Cartella").size(11))
            .padding([4, 8])
            .style(theme::toggle_toolbar_button(app.find_in_folder))
            .on_press(Message::ToggleFindInFolder),
        close_panel_button(Message::CloseFindBar),
    ]
    .spacing(5)
    .align_y(Alignment::Center);

    let mut content = Column::new().spacing(5).push(find_row);

    if app.show_replace_row {
        let replace_row = row![
            Space::with_width(22),
            text_input(
                if app.find_use_regex {
                    "Sostituisci con... (usa $1, $2 per gruppi)"
                } else {
                    "Sostituisci con..."
                },
                &app.replace_query,
            )
            .id(app.replace_input_id.clone())
            .on_input(Message::ReplaceQueryChanged)
            .on_submit(Message::ReplaceCurrent)
            .size(12)
            .padding([5, 8])
            .width(Length::Fill),
            button(text("Sostituisci").size(11))
                .padding([4, 9])
                .style(theme::symbol_button)
                .on_press(Message::ReplaceCurrent),
            button(text("Tutto").size(11))
                .padding([4, 9])
                .style(theme::symbol_button)
                .on_press(Message::ReplaceAll),
        ]
        .spacing(5)
        .align_y(Alignment::Center);
        content = content.push(replace_row);
    }

    if app.find_in_folder && !app.find_query.is_empty() && app.find_error.is_none() {
        let mut folder_col = Column::new().spacing(2);
        if app.folder_search_hits.is_empty() {
            folder_col = folder_col.push(
                text(if app.tab().doc.path.is_none() && app.master_tex_path.is_none() {
                    "Salva il file per cercare nei .tex/.bib della cartella."
                } else {
                    "Nessun risultato nei file .tex/.bib della cartella."
                })
                .size(11)
                .color(pal.text_faint),
            );
        } else {
            folder_col = folder_col.push(
                text(format!(
                    "{} righe trovate nella cartella progetto:",
                    app.folder_search_hits.len()
                ))
                .font(theme::ui_font_semibold())
                .size(10)
                .color(pal.text_faint),
            );
            for hit in &app.folder_search_hits {
                let row_btn = button(
                    row![
                        text(format!("{}:{}", hit.file_name, hit.line_1based))
                            .font(theme::mono_font_bold())
                            .size(11)
                            .color(pal.accent),
                        text(&hit.preview)
                            .font(theme::mono_font())
                            .size(11)
                            .color(pal.text_muted),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([3, 7])
                .width(Length::Fill)
                .style(theme::context_menu_item)
                .on_press(Message::OutlineJump {
                    file_path: Some(hit.file_path.clone()),
                    line_1based: hit.line_1based,
                });
                folder_col = folder_col.push(row_btn);
            }
        }

        content = content.push(
            container(
                scrollable(folder_col)
                    .direction(scrollable::Direction::Vertical(thin_scrollbar()))
                    .style(theme::custom_scrollable)
                    .width(Length::Fill)
                    .height(Length::Shrink),
            )
            .max_height(156.0)
            .padding([5, 7])
            .width(Length::Fill)
            .style(theme::log_inset),
        );
    }

    let wrapped = container(content)
        .padding([6, 10])
        .width(Length::Fill)
        .style(theme::surface_bar);

    column![wrapped, horizontal_rule(1).style(theme::subtle_rule)].into()
}

fn view_outline_panel(app: &App) -> Element<'_, Message> {
    let pal = theme::palette_for(app.dark_mode);

    let filter_btn = |label: &'static str, f: OutlineFilter| -> Element<'static, Message> {
        button(
            text(label)
                .font(if app.outline_filter == f {
                    theme::ui_font_semibold()
                } else {
                    theme::ui_font()
                })
                .size(11),
        )
        .padding([4, 7])
        .style(theme::tab_button(app.outline_filter == f))
        .on_press(Message::SetOutlineFilter(f))
        .into()
    };

    let filters = row![
        filter_btn("Tutto", OutlineFilter::All),
        filter_btn("Sez.", OutlineFilter::Sections),
        filter_btn("Teor.", OutlineFilter::Theorems),
        filter_btn("Label", OutlineFilter::Labels),
        filter_btn("File", OutlineFilter::Files),
    ]
    .spacing(3);

    let master_box: Element<'_, Message> = if let Some(master) = app.effective_master_path() {
        let name = master
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("main.tex")
            .to_string();
        let is_pinned = app.master_tex_path.is_some();
        let mut r = row![
            text(format!("Root: {name}"))
                .font(theme::ui_font_semibold())
                .size(11)
                .color(pal.accent),
            horizontal_space(),
        ]
        .align_y(Alignment::Center);
        if is_pinned {
            r = r.push(
                button(text("Sblocca").size(10))
                    .padding([2, 6])
                    .style(theme::ghost_button)
                    .on_press(Message::ClearMaster),
            );
        } else {
            r = r.push(
                text("!TeX root")
                    .font(theme::mono_font())
                    .size(10)
                    .color(pal.text_faint),
            );
        }
        container(r)
            .padding([5, 8])
            .width(Length::Fill)
            .style(theme::log_inset)
            .into()
    } else {
        let pin_label = if app.tab().doc.path.is_some() {
            "Fissa questo file come Master"
        } else {
            "Salva il file per fissare il Master"
        };
        button(text(pin_label).size(11).color(pal.text_muted))
            .padding([5, 8])
            .width(Length::Fill)
            .style(theme::symbol_button)
            .on_press(Message::TogglePinMaster)
            .into()
    };

    let cur_file_path = app.tab().doc.path.as_deref();
    let cur_line_1based = app.tab().doc.content.cursor_position().0 + 1;

    let filtered: Vec<_> = app
        .outline_items
        .iter()
        .filter(|item| app.outline_filter.matches(item.kind))
        .collect();

    let body: Element<'_, Message> = if filtered.is_empty() {
        container(
            text("Nessun elemento trovato.\nUsa \\section{...}, \\begin{theorem}, \\label{...} o \\input{...}.")
                .size(12)
                .align_x(Alignment::Center)
                .color(pal.text_faint),
        )
        .center(Length::Fill)
        .padding(14)
        .into()
    } else {
        let mut list = Column::new().spacing(3).padding(Padding {
            top: 0.0,
            right: 6.0,
            bottom: 8.0,
            left: 0.0,
        });

        for item in filtered {
            let indent_px = (item.kind.indent_level() as f32) * 12.0
                + if item.from_included_file && item.kind != OutlineKind::Include {
                    8.0
                } else {
                    0.0
                };

            let same_file = match (cur_file_path, item.file_path.as_deref()) {
                (Some(a), Some(b)) => a == b,
                (None, None) => true,
                _ => false,
            };
            let is_near_cursor = same_file
                && cur_line_1based >= item.line_1based
                && cur_line_1based <= item.line_1based + 12;

            let badge_color = match item.kind {
                OutlineKind::Include => pal.success,
                OutlineKind::Theorem => pal.accent,
                OutlineKind::Label => pal.text_muted,
                _ => pal.accent,
            };

            let subtitle = if item.from_included_file && item.kind != OutlineKind::Include {
                format!("{}:{}", item.file_name, item.line_1based)
            } else {
                format!(":{}", item.line_1based)
            };

            let row_content = row![
                Space::with_width(Length::Fixed(indent_px)),
                container(
                    text(item.kind.badge())
                        .font(theme::mono_font_bold())
                        .size(9)
                        .color(badge_color),
                )
                .padding([1, 5])
                .style(theme::shortcut_pill),
                text(&item.title)
                    .font(if matches!(
                        item.kind,
                        OutlineKind::Part | OutlineKind::Chapter | OutlineKind::Section | OutlineKind::Include
                    ) {
                        theme::ui_font_semibold()
                    } else if item.kind == OutlineKind::Label {
                        theme::mono_font()
                    } else {
                        theme::ui_font()
                    })
                    .size(12)
                    .color(if is_near_cursor {
                        pal.accent
                    } else {
                        pal.text_primary
                    })
                    .width(Length::Fill),
                text(subtitle)
                    .font(theme::mono_font())
                    .size(10)
                    .color(pal.text_faint),
            ]
            .spacing(6)
            .align_y(Alignment::Center);

            let jump_line = if item.kind == OutlineKind::Include {
                1
            } else {
                item.line_1based
            };

            list = list.push(
                button(row_content)
                    .padding([5, 6])
                    .width(Length::Fill)
                    .style(theme::suggestion_row(is_near_cursor))
                    .on_press(Message::OutlineJump {
                        file_path: item.file_path.clone(),
                        line_1based: jump_line,
                    }),
            );
        }

        scrollable(list)
            .direction(scrollable::Direction::Vertical(thin_scrollbar()))
            .style(theme::custom_scrollable)
            .height(Length::Fill)
            .into()
    };

    container(
        column![
            row![
                text("Struttura Progetto")
                    .font(theme::ui_font_semibold())
                    .size(13)
                    .color(pal.text_primary),
                horizontal_space(),
                close_panel_button(Message::ToggleOutline),
            ]
            .align_y(Alignment::Center),
            master_box,
            filters,
            horizontal_rule(1).style(theme::subtle_rule),
            body,
        ]
        .spacing(8)
        .padding(12),
    )
    .width(Length::Fixed(symbols_panel_width(app.window_size.width)))
    .height(Length::Fill)
    .clip(true)
    .style(theme::side_panel)
    .into()
}

fn view_pdf_panel(app: &App) -> Element<'_, Message> {
    let pal = theme::palette_for(app.dark_mode);
    let tab = app.tab();
    let compact = app.window_size.width < 1050.0;
    let panel_pad = pdf_panel_padding(app.window_size.width) as u16;
    let hdr_btn_pad: [u16; 2] = if compact { [2, 6] } else { [3, 8] };
    let header_title = if tab.pdf_total_pages == 0 {
        "Anteprima PDF".to_string()
    } else {
        format!("PDF · {} pag.", tab.pdf_total_pages)
    };

    let zoom_pct = (app.pdf_zoom * 100.0).round() as i32;

    let mut header_row = row![
        text(header_title)
            .font(theme::ui_font_semibold())
            .size(13)
            .color(pal.text_primary),
        horizontal_space(),
        button(text("-").size(12))
            .padding(if compact { [2, 6] } else { [2, 8] })
            .style(theme::ghost_button)
            .on_press(Message::ZoomOutPdf),
        button(
            text(format!("{zoom_pct}%"))
                .font(theme::mono_font())
                .size(11)
        )
        .padding([2, 6])
        .style(theme::ghost_button)
        .on_press(Message::ResetZoomPdf),
        button(text("+").size(12))
            .padding(if compact { [2, 6] } else { [2, 8] })
            .style(theme::ghost_button)
            .on_press(Message::ZoomInPdf),
    ]
    .spacing(3)
    .align_y(Alignment::Center);

    if tab.last_pdf.is_some() {
        header_row = header_row
            .push(
                button(text("Sync").size(11))
                    .padding(hdr_btn_pad)
                    .style(theme::ghost_button)
                    .on_press(Message::SyncPdfToLastEdit),
            )
            .push(
                button(text("Esterno").size(11))
                    .padding(hdr_btn_pad)
                    .style(theme::ghost_button)
                    .on_press(Message::OpenPdfExternal),
            );
    }

    header_row = header_row.push(close_panel_button(Message::TogglePdf));

    let body: Element<'_, Message> = if tab.pdf_total_pages > 0 {
        let fit_base_w = pdf_fit_base_width(app);
        let (page_width, page_height) =
            pdf_page_dimensions(fit_base_w, app.pdf_zoom, tab.pdf_aspect_ratio);
        let stride = pdf_page_stride(fit_base_w, app.pdf_zoom, tab.pdf_aspect_ratio);
        let (first_vis, last_vis) = app.visible_pdf_page_range();
        let skip_before = first_vis.saturating_sub(1);
        let skip_after = tab.pdf_total_pages.saturating_sub(last_vis);
        let needs_h_scroll = app.pdf_zoom > 1.001;
        let filter = if app.pdf_cache_width == app.target_pdf_render_width() {
            image::FilterMethod::Nearest
        } else {
            image::FilterMethod::Linear
        };

        let mut pages_col = Column::new()
            .spacing(PDF_PAGE_SPACING)
            .padding(PDF_LIST_PADDING)
            .align_x(Alignment::Center);

        if !needs_h_scroll {
            pages_col = pages_col.width(Length::Fill);
        }

        if skip_before > 0 {
            let top_h = (skip_before as f32) * stride - PDF_PAGE_SPACING;
            pages_col = pages_col.push(Space::new(
                Length::Fixed(page_width + PDF_CARD_PADDING * 2.0),
                Length::Fixed(top_h.max(0.0)),
            ));
        }

        for page_num in first_vis..=last_vis {
            let inner: Element<'_, Message> =
                if let Some(handle) = app.pdf_page_cache.get(&page_num) {
                    // Use `Nearest` on exact 1:1 physical-pixel textures so `iced_wgpu`'s
                    // `+0.5 / (width - 1.0)` atlas UV inset maps every pixel bit-identically;
                    // use `Linear` briefly during resize drags before the crisp re-render arrives.
                    image(handle.clone())
                        .width(Length::Fixed(page_width))
                        .height(Length::Fixed(page_height))
                        .filter_method(filter)
                        .into()
                } else {
                    container(
                        text(format!("Pagina {page_num}..."))
                            .size(13)
                            .color(theme::LIGHT.text_faint),
                    )
                    .width(Length::Fixed(page_width))
                    .height(Length::Fixed(page_height))
                    .center_x(Length::Fixed(page_width))
                    .center_y(Length::Fixed(page_height))
                    .into()
                };

            let interactive_page: Element<'_, Message> = PdfPageArea::new(
                inner,
                page_num,
                tab.pdf_scroll_offset,
                |page, x_ratio, y_ratio| Message::PdfLeftClicked {
                    page,
                    x_ratio,
                    y_ratio,
                },
                |page, x_ratio, y_ratio, anchor| Message::PdfRightClicked {
                    page,
                    x_ratio,
                    y_ratio,
                    anchor,
                },
            )
            .into();

            let page_card = container(interactive_page)
                .padding(PDF_CARD_PADDING)
                .style(theme::pdf_page_frame);
            pages_col = pages_col.push(page_card);
        }

        if skip_after > 0 {
            let bot_h = (skip_after as f32) * stride - PDF_PAGE_SPACING;
            pages_col = pages_col.push(Space::new(
                Length::Fixed(page_width + PDF_CARD_PADDING * 2.0),
                Length::Fixed(bot_h.max(0.0)),
            ));
        }

        let pdf_scroll_dir = if needs_h_scroll {
            scrollable::Direction::Both {
                vertical: thin_scrollbar(),
                horizontal: thin_scrollbar(),
            }
        } else {
            scrollable::Direction::Vertical(thin_scrollbar())
        };

        container(
            scrollable(pages_col)
                .id(app.pdf_scroll_id.clone())
                .direction(pdf_scroll_dir)
                .on_scroll(Message::PdfScrolled)
                .style(theme::custom_scrollable)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::pdf_desk)
        .into()
    } else if let Some(note) = &tab.preview_note {
        container(text(note).size(13).color(pal.text_muted))
            .center(Length::Fill)
            .padding(20)
            .style(theme::pdf_desk)
            .into()
    } else {
        container(
            column![
                text("Nessun PDF generato")
                    .font(theme::ui_font_semibold())
                    .size(14)
                    .color(pal.text_primary),
                text("Premi Compila (Ctrl+B) per generare e visualizzare l'anteprima.")
                    .size(12)
                    .align_x(Alignment::Center)
                    .color(pal.text_muted),
            ]
            .spacing(6)
            .align_x(Alignment::Center),
        )
        .center(Length::Fill)
        .padding(24)
        .style(theme::pdf_desk)
        .into()
    };

    container(
        column![
            header_row,
            horizontal_rule(1).style(theme::subtle_rule),
            body,
        ]
        .spacing(if compact { 8 } else { 10 })
        .padding(panel_pad),
    )
    .width(Length::FillPortion(3))
    .height(Length::Fill)
    .clip(true)
    .style(theme::side_panel)
    .into()
}

/// Thin widget wrapper around a rendered PDF page that captures click coordinates:
/// - Left-click teleports straight to the corresponding LaTeX source line via SyncTeX.
/// - Right-click opens the context menu ("Visualizza codice sorgente") at true window coordinates.
struct PdfPageArea<'a, Message> {
    content: Element<'a, Message>,
    page_number: usize,
    scroll_offset: scrollable::AbsoluteOffset,
    on_left_click: Box<dyn Fn(usize, f32, f32) -> Message + 'a>,
    on_right_click: Box<dyn Fn(usize, f32, f32, Point) -> Message + 'a>,
}

impl<'a, Message> PdfPageArea<'a, Message> {
    fn new(
        content: Element<'a, Message>,
        page_number: usize,
        scroll_offset: scrollable::AbsoluteOffset,
        on_left_click: impl Fn(usize, f32, f32) -> Message + 'a,
        on_right_click: impl Fn(usize, f32, f32, Point) -> Message + 'a,
    ) -> Self {
        Self {
            content,
            page_number,
            scroll_offset,
            on_left_click: Box::new(on_left_click),
            on_right_click: Box::new(on_right_click),
        }
    }
}

impl<'a, Message> Widget<Message, Theme, iced::Renderer> for PdfPageArea<'a, Message> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn on_event(
        &mut self,
        tree: &mut Tree,
        event: Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) -> event::Status {
        let bounds = layout.bounds();
        match &event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(local) = cursor.position_in(bounds) {
                    let x_ratio = (local.x / bounds.width.max(1.0)).clamp(0.0, 1.0);
                    let y_ratio = (local.y / bounds.height.max(1.0)).clamp(0.0, 1.0);
                    shell.publish((self.on_left_click)(self.page_number, x_ratio, y_ratio));
                    return event::Status::Captured;
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                if let (Some(local), Some(scrolled_pos)) =
                    (cursor.position_in(bounds), cursor.position())
                {
                    let x_ratio = (local.x / bounds.width.max(1.0)).clamp(0.0, 1.0);
                    let y_ratio = (local.y / bounds.height.max(1.0)).clamp(0.0, 1.0);
                    // `Scrollable` translates `cursor` into content space by `+scroll_offset`;
                    // subtract it to recover exact window coordinates for the popup menu.
                    let window_pos = Point::new(
                        scrolled_pos.x - self.scroll_offset.x,
                        scrolled_pos.y - self.scroll_offset.y,
                    );
                    shell.publish((self.on_right_click)(
                        self.page_number,
                        x_ratio,
                        y_ratio,
                        window_pos,
                    ));
                    return event::Status::Captured;
                }
            }
            _ => {}
        }

        self.content.as_widget_mut().on_event(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        )
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
}

impl<'a, Message: 'a> From<PdfPageArea<'a, Message>> for Element<'a, Message> {
    fn from(area: PdfPageArea<'a, Message>) -> Self {
        Element::new(area)
    }
}

/// Positions its child vertically inside a clipped viewport, keeping the fixed left gutter
/// aligned line-for-line with the scrolled code editor without participating in horizontal scroll.
struct VerticalOffset<'a, Message> {
    content: Element<'a, Message>,
    offset_y: f32,
}

impl<'a, Message> VerticalOffset<'a, Message> {
    fn new(content: Element<'a, Message>, offset_y: f32) -> Self {
        Self { content, offset_y }
    }
}

impl<'a, Message> Widget<Message, Theme, iced::Renderer> for VerticalOffset<'a, Message> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = limits.resolve(Length::Fill, Length::Fill, Size::ZERO);
        let child_limits = layout::Limits::new(Size::ZERO, Size::new(size.width, f32::INFINITY));
        let child_node = self
            .content
            .as_widget()
            .layout(&mut tree.children[0], renderer, &child_limits)
            .move_to(Point::new(0.0, self.offset_y));
        layout::Node::with_children(size, vec![child_node])
    }

    fn operate(
        &self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        if let Some(child_layout) = layout.children().next() {
            self.content
                .as_widget()
                .operate(&mut tree.children[0], child_layout, renderer, operation);
        }
    }

    fn on_event(
        &mut self,
        _tree: &mut Tree,
        _event: Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        _shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) -> event::Status {
        event::Status::Ignored
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let Some(clipped) = viewport.intersection(&bounds) else {
            return;
        };
        let Some(child_layout) = layout.children().next() else {
            return;
        };
        renderer.with_layer(bounds, |renderer| {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                child_layout,
                cursor,
                &clipped,
            );
        });
    }
}

impl<'a, Message: 'a> From<VerticalOffset<'a, Message>> for Element<'a, Message> {
    fn from(offset: VerticalOffset<'a, Message>) -> Self {
        Element::new(offset)
    }
}

/// Non-intrusive wrapper around the editor's `Scrollable` that tracks pointer position
/// globally while drag-selecting text, enabling 4-edge auto-scrolling and continuous
/// selection extension even when the cursor moves over scrollbars, gutter, or adjacent panes.
struct EditorDragScrollArea<'a> {
    content: Element<'a, Message>,
    is_dragging: bool,
}

impl<'a> EditorDragScrollArea<'a> {
    fn new(content: Element<'a, Message>, is_dragging: bool) -> Self {
        Self {
            content,
            is_dragging,
        }
    }
}

impl<'a> Widget<Message, Theme, iced::Renderer> for EditorDragScrollArea<'a> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn on_event(
        &mut self,
        tree: &mut Tree,
        event: Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) -> event::Status {
        let bounds = layout.bounds();
        if self.is_dragging {
            match &event {
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    shell.publish(Message::EditorDragMoved {
                        local: Point::new(position.x - bounds.x, position.y - bounds.y),
                        viewport: Size::new(bounds.width, bounds.height),
                    });
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    shell.publish(Message::EditorDragEnded);
                }
                _ => {}
            }
        }

        self.content.as_widget_mut().on_event(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        )
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
}

impl<'a> From<EditorDragScrollArea<'a>> for Element<'a, Message> {
    fn from(area: EditorDragScrollArea<'a>) -> Self {
        Element::new(area)
    }
}

fn view_suggestion_popup(
    app: &App,
    font_size: f32,
    line_height: f32,
    gutter_w: f32,
) -> Element<'_, Message> {
    let pal = theme::palette_for(app.dark_mode);
    let tab = app.tab();
    let (line, _) = tab.doc.content.cursor_position();
    let visual_col = tab.doc.cursor_visual_col();
    let prefix_len = app
        .active_prefix
        .as_ref()
        .map_or(0, |p| p.chars().count());
    let start_col = visual_col.saturating_sub(prefix_len);
    let char_width = editor_char_width(font_size);

    let raw_top =
        EDITOR_PADDING + ((line + 1) as f32) * line_height + 4.0 - tab.editor_scroll_offset.y;
    let raw_left = gutter_w
        + 1.0
        + EDITOR_PADDING
        + (start_col as f32) * char_width
        - tab.editor_scroll_offset.x;

    let top_offset = raw_top.max(8.0).round();
    let left_offset = raw_left.max(gutter_w + 8.0).round();

    let has_details = app.suggestions.iter().any(|s| s.detail.is_some());
    let popup_w = if has_details { 384.0 } else { 264.0 };

    let mut rows = Column::new().spacing(2);
    for (idx, suggestion) in app.suggestions.iter().enumerate() {
        let is_selected = idx == app.selected_suggestion;
        let row_body: Element<'_, Message> = if let Some(detail) = &suggestion.detail {
            column![
                text(&suggestion.display_label)
                    .font(theme::mono_font_bold())
                    .size(12)
                    .color(if is_selected {
                        pal.accent
                    } else {
                        pal.text_primary
                    }),
                text(detail).size(11).color(pal.text_muted),
            ]
            .spacing(1)
            .into()
        } else {
            text(&suggestion.display_label)
                .font(theme::mono_font())
                .size(13)
                .into()
        };

        let item = button(row_body)
            .padding([5, 10])
            .width(Length::Fill)
            .style(theme::suggestion_row(is_selected))
            .on_press(Message::ApplySuggestion(idx));

        rows = rows.push(item);
    }

    rows = rows
        .push(horizontal_rule(1).style(theme::subtle_rule))
        .push(
            container(
                text("↑↓ naviga · Tab/Invio accetta · Esc chiudi")
                    .font(theme::math_symbol_font())
                    .shaping(Shaping::Advanced)
                    .size(10)
                    .color(pal.text_faint),
            )
            .padding([3, 8]),
        );

    let popup = container(rows)
        .padding(5)
        .width(Length::Fixed(popup_w))
        .style(theme::popup_box);

    container(popup)
        .padding(Padding {
            top: top_offset,
            left: left_offset,
            right: 0.0,
            bottom: 0.0,
        })
        .into()
}

fn view_symbol_palette(app: &App) -> Element<'_, Message> {
    let pal = theme::palette_for(app.dark_mode);

    let tabs = row![
        tab_btn("Logica", PaletteTab::Logic, app.palette_tab),
        tab_btn("Greco", PaletteTab::Greek, app.palette_tab),
        tab_btn("Math", PaletteTab::Math, app.palette_tab),
        tab_btn("Formule", PaletteTab::Formulas, app.palette_tab),
        tab_btn("Librerie", PaletteTab::Libraries, app.palette_tab),
    ]
    .spacing(3);

    let body: Element<'_, Message> = match app.palette_tab {
        PaletteTab::Logic => symbol_grid(LOGIC_SYMBOLS, 2, app.dark_mode),
        PaletteTab::Greek => symbol_grid(GREEK_SYMBOLS, 3, app.dark_mode),
        PaletteTab::Math => symbol_grid(MATH_SYMBOLS, 2, app.dark_mode),
        PaletteTab::Formulas => formula_list(FORMULAS, app.dark_mode),
        PaletteTab::Libraries => view_libraries_tab(app),
    };

    container(
        column![
            row![
                text("Simboli e Librerie")
                    .font(theme::ui_font_semibold())
                    .size(13)
                    .color(pal.text_primary),
                horizontal_space(),
                close_panel_button(Message::ToggleSymbols),
            ]
            .align_y(Alignment::Center),
            tabs,
            horizontal_rule(1).style(theme::subtle_rule),
            body,
        ]
        .spacing(10)
        .padding(12),
    )
    .width(Length::Fixed(symbols_panel_width(app.window_size.width)))
    .height(Length::Fill)
    .clip(true)
    .style(theme::side_panel)
    .into()
}

fn view_libraries_tab(app: &App) -> Element<'_, Message> {
    let pal = theme::palette_for(app.dark_mode);
    let mut list = Column::new().spacing(10).padding(Padding {
        top: 0.0,
        right: 6.0,
        bottom: 10.0,
        left: 0.0,
    });

    list = list.push(
        text("PACK RAPIDI")
            .font(theme::ui_font_semibold())
            .size(10)
            .color(pal.text_faint),
    );

    let mut presets_col = Column::new().spacing(6);
    for chunk in LIBRARY_PRESETS.chunks(2) {
        let mut r = Row::new().spacing(6);
        for preset in chunk {
            let btn = button(
                column![
                    text(preset.name).font(theme::ui_font_semibold()).size(12),
                    text(format!("{} pacchetti", preset.packages.len())).size(10),
                ]
                .spacing(1)
                .align_x(Alignment::Center),
            )
            .padding([6, 8])
            .width(Length::Fill)
            .style(theme::preset_button)
            .on_press(Message::AddLibraryPreset(preset.name, preset.packages));
            r = r.push(btn);
        }
        presets_col = presets_col.push(r);
    }
    list = list.push(presets_col);

    for &category in LibraryCategory::ALL {
        list = list.push(Space::with_height(4));
        list = list.push(
            text(category.title().to_uppercase())
                .font(theme::ui_font_semibold())
                .size(10)
                .color(pal.text_faint),
        );

        for lib in MATH_LIBRARIES.iter().filter(|l| l.category == category) {
            let in_doc = app.tab().doc.has_package(lib.package);
            let tex_available = app
                .installed_tex_packages
                .as_ref()
                .map(|set| set.contains(lib.package))
                .unwrap_or(true);

            let badge: Element<'_, Message> = match (in_doc, tex_available) {
                (true, true) => container(
                    text("Inclusa ✓")
                        .font(theme::math_symbol_font())
                        .shaping(Shaping::Advanced)
                        .size(10)
                        .color(pal.success),
                )
                .padding([2, 6])
                .style(theme::installed_badge)
                .into(),
                (true, false) => container(text("Manca TeX").size(10).color(pal.error))
                    .padding([2, 6])
                    .style(theme::missing_badge)
                    .into(),
                (false, true) => container(text("+ Aggiungi").size(10).color(pal.accent))
                    .padding([2, 6])
                    .style(theme::add_badge)
                    .into(),
                (false, false) => container(text("Non installata").size(10).color(pal.error))
                    .padding([2, 6])
                    .style(theme::missing_badge)
                    .into(),
            };

            let desc_text = if tex_available {
                lib.description.to_string()
            } else {
                format!("{} · Richiede {}", lib.description, lib.texlive_group)
            };

            let item = button(
                column![
                    row![
                        text(format!("\\usepackage{{{}}}", lib.package))
                            .font(theme::mono_font_bold())
                            .size(12)
                            .color(if !tex_available {
                                pal.error
                            } else if in_doc {
                                pal.success
                            } else {
                                pal.accent
                            }),
                        horizontal_space(),
                        badge,
                    ]
                    .align_y(Alignment::Center),
                    text(desc_text).size(11).color(pal.text_muted),
                ]
                .spacing(4)
                .padding([6, 8]),
            )
            .width(Length::Fill)
            .style(theme::symbol_button)
            .on_press(Message::AddLibrary(lib.package));

            list = list.push(item);
        }
    }

    scrollable(list)
        .direction(scrollable::Direction::Vertical(thin_scrollbar()))
        .style(theme::custom_scrollable)
        .height(Length::Fill)
        .into()
}

fn tab_btn(label: &'static str, tab: PaletteTab, current: PaletteTab) -> Element<'static, Message> {
    button(
        text(label)
            .font(if tab == current {
                theme::ui_font_semibold()
            } else {
                theme::ui_font()
            })
            .size(11),
    )
    .padding([5, 8])
    .style(theme::tab_button(tab == current))
    .on_press(Message::SelectPaletteTab(tab))
    .into()
}

fn symbol_grid(
    items: &'static [SymbolItem],
    cols: usize,
    dark_mode: bool,
) -> Element<'static, Message> {
    let pal = theme::palette_for(dark_mode);
    let mut list = Column::new().spacing(6).padding(Padding {
        top: 0.0,
        right: 6.0,
        bottom: 8.0,
        left: 0.0,
    });

    for chunk in items.chunks(cols) {
        let mut r = Row::new().spacing(6);
        for item in chunk {
            let content: Element<'static, Message> = if cols >= 3 {
                column![
                    text(item.glyph)
                        .font(theme::math_symbol_font())
                        .shaping(Shaping::Advanced)
                        .size(17)
                        .color(pal.text_primary),
                    text(item.label)
                        .font(theme::mono_font())
                        .size(10)
                        .color(pal.text_muted),
                ]
                .spacing(2)
                .align_x(Alignment::Center)
                .width(Length::Fill)
                .into()
            } else {
                row![
                    text(item.glyph)
                        .font(theme::math_symbol_font())
                        .shaping(Shaping::Advanced)
                        .size(16)
                        .color(pal.accent),
                    text(item.label)
                        .font(theme::mono_font())
                        .size(11)
                        .color(pal.text_primary),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            };

            let btn = button(content)
                .padding([7, 8])
                .width(Length::Fill)
                .style(theme::symbol_button)
                .on_press(Message::InsertSnippet(item.insert));
            r = r.push(btn);
        }
        for _ in chunk.len()..cols {
            r = r.push(Space::with_width(Length::Fill));
        }
        list = list.push(r);
    }

    scrollable(list)
        .direction(scrollable::Direction::Vertical(thin_scrollbar()))
        .style(theme::custom_scrollable)
        .height(Length::Fill)
        .into()
}

fn formula_list(items: &'static [FormulaItem], dark_mode: bool) -> Element<'static, Message> {
    let pal = theme::palette_for(dark_mode);
    let mut list = Column::new().spacing(6).padding(Padding {
        top: 0.0,
        right: 6.0,
        bottom: 8.0,
        left: 0.0,
    });

    for item in items {
        let glyph_chip = container(
            text(item.glyph)
                .font(theme::math_symbol_font())
                .shaping(Shaping::Advanced)
                .size(12)
                .color(pal.accent),
        )
        .padding([3, 7])
        .style(theme::icon_chip);

        let btn = button(
            row![
                glyph_chip,
                text(item.label).size(12).color(pal.text_primary),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .padding([3, 4]),
        )
        .padding([6, 8])
        .width(Length::Fill)
        .style(theme::symbol_button)
        .on_press(Message::InsertFormula(item));

        list = list.push(btn);
    }

    scrollable(list)
        .direction(scrollable::Direction::Vertical(thin_scrollbar()))
        .style(theme::custom_scrollable)
        .height(Length::Fill)
        .into()
}

fn view_match_panel(app: &App) -> Element<'_, Message> {
    let pal = theme::palette_for(app.dark_mode);

    let mut header = row![
        text("Match")
            .font(theme::ui_font_semibold())
            .size(13)
            .color(pal.text_primary),
        horizontal_space(),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    if !app.match_strokes.is_empty() {
        header = header
            .push(
                button(text("Annulla").size(11))
                    .padding([3, 8])
                    .style(theme::ghost_button)
                    .on_press(Message::MatchUndo),
            )
            .push(
                button(text("Pulisci").size(11))
                    .padding([3, 8])
                    .style(theme::ghost_button)
                    .on_press(Message::MatchClear),
            );
    }
    header = header.push(close_panel_button(Message::ToggleMatch));

    let pad = matcher::DrawingPad::new(
        &app.match_strokes,
        app.dark_mode,
        Message::MatchStrokeCompleted,
    );

    let results_area: Element<'_, Message> = if app.match_results.is_empty() {
        container(
            column![
                text("Disegna un simbolo nel riquadro")
                    .font(theme::ui_font_semibold())
                    .size(13)
                    .color(pal.text_primary),
                text("Es. ∀, ∃, ∑, ∫, ∈, ⊆, Δ, ∞, →, ≠, ℝ")
                    .font(theme::math_symbol_font())
                    .shaping(Shaping::Advanced)
                    .size(12)
                    .color(pal.text_muted),
                text("Puoi usare più tratti: ogni tratto aggiorna i suggerimenti in tempo reale.")
                    .size(11)
                    .align_x(Alignment::Center)
                    .color(pal.text_faint),
            ]
            .spacing(6)
            .align_x(Alignment::Center),
        )
        .center(Length::Fill)
        .padding(14)
        .into()
    } else {
        let mut list = Column::new().spacing(6).padding(Padding {
            top: 2.0,
            right: 6.0,
            bottom: 8.0,
            left: 0.0,
        });

        for (idx, cand) in app.match_results.iter().enumerate() {
            let glyph_chip = container(
                text(cand.glyph)
                    .font(theme::math_symbol_font())
                    .shaping(Shaping::Advanced)
                    .size(20)
                    .color(pal.accent),
            )
            .width(Length::Fixed(42.0))
            .height(Length::Fixed(38.0))
            .center_x(Length::Fixed(42.0))
            .center_y(Length::Fixed(38.0))
            .style(theme::icon_chip);

            let subtitle = match cand.package {
                Some(pkg) => format!("{} · {}", cand.description, pkg),
                None => cand.description.to_string(),
            };

            let pct_pill = container(
                text(format!("{}%", cand.confidence))
                    .font(theme::mono_font())
                    .size(10)
                    .color(pal.text_muted),
            )
            .padding([2, 6])
            .style(theme::shortcut_pill);

            let card = button(
                row![
                    glyph_chip,
                    column![
                        text(cand.display_cmd)
                            .font(theme::mono_font_bold())
                            .size(13)
                            .color(pal.text_primary),
                        text(subtitle).size(11).color(pal.text_muted),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    pct_pill,
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .padding([4, 6]),
            )
            .width(Length::Fill)
            .style(theme::symbol_button)
            .on_press(Message::MatchInsert(idx));

            list = list.push(card);
        }

        scrollable(list)
            .direction(scrollable::Direction::Vertical(thin_scrollbar()))
            .style(theme::custom_scrollable)
            .height(Length::Fill)
            .into()
    };

    let learned = matcher::learned_count();
    let strokes_label = match app.match_strokes.len() {
        0 if learned > 0 => format!(
            "Disegna con il mouse · {learned} esempi personali appresi"
        ),
        0 => "Disegna con il mouse · le tue scelte migliorano il riconoscimento".to_string(),
        1 => "1 tratto · clicca un simbolo per inserirlo".to_string(),
        n => format!("{n} tratti · clicca un simbolo per inserirlo"),
    };

    container(
        column![
            header,
            text(strokes_label).size(11).color(pal.text_muted),
            pad,
            horizontal_rule(1).style(theme::subtle_rule),
            results_area,
        ]
        .spacing(10)
        .padding(12),
    )
    .width(Length::Fixed(symbols_panel_width(app.window_size.width)))
    .height(Length::Fill)
    .clip(true)
    .style(theme::side_panel)
    .into()
}

fn view_assets_panel(app: &App) -> Element<'_, Message> {
    let pal = theme::palette_for(app.dark_mode);

    let width_chip = |label: &'static str, ratio: f32| -> Element<'static, Message> {
        let active = (app.figure_width_ratio - ratio).abs() < 0.05;
        button(text(label).font(theme::mono_font()).size(11))
            .padding([4, 8])
            .width(Length::Fill)
            .style(theme::tab_button(active))
            .on_press(Message::SetFigureWidth(ratio))
            .into()
    };

    let figure_card = container(
        column![
            text("FIGURA & DRAG-AND-DROP")
                .font(theme::ui_font_semibold())
                .size(10)
                .color(pal.text_faint),
            text("Trascina un file .pdf, .png, .jpg o .svg nella finestra oppure sceglilo dal disco:")
                .size(11)
                .color(pal.text_muted),
            row![
                width_chip("0.5\\tw", 0.5),
                width_chip("0.8\\tw", 0.8),
                width_chip("1.0\\tw", 1.0),
            ]
            .spacing(4),
            button(
                text("+ Inserisci figura dal disco...")
                    .font(theme::ui_font_semibold())
                    .size(12),
            )
            .padding([6, 10])
            .width(Length::Fill)
            .style(theme::primary_button)
            .on_press(Message::PickFigureImage),
        ]
        .spacing(7),
    )
    .padding(10)
    .width(Length::Fill)
    .style(theme::log_inset);

    let align_chip = |label: &'static str, ch: char| -> Element<'static, Message> {
        button(text(label).font(theme::mono_font()).size(11))
            .padding([3, 8])
            .style(theme::tab_button(app.table_align == ch))
            .on_press(Message::SetTableAlign(ch))
            .into()
    };

    let table_card = container(
        column![
            text("GENERATORE TABELLE")
                .font(theme::ui_font_semibold())
                .size(10)
                .color(pal.text_faint),
            row![
                text("Righe:").size(12).color(pal.text_muted),
                button(text("-").font(theme::mono_font_bold()).size(12))
                    .padding([2, 8])
                    .style(theme::symbol_button)
                    .on_press(Message::SetTableRows(app.table_rows.saturating_sub(1))),
                text(format!("{}", app.table_rows))
                    .font(theme::mono_font_bold())
                    .size(12)
                    .color(pal.text_primary),
                button(text("+").font(theme::mono_font_bold()).size(12))
                    .padding([2, 8])
                    .style(theme::symbol_button)
                    .on_press(Message::SetTableRows(app.table_rows + 1)),
                horizontal_space(),
                text("Col:").size(12).color(pal.text_muted),
                button(text("-").font(theme::mono_font_bold()).size(12))
                    .padding([2, 8])
                    .style(theme::symbol_button)
                    .on_press(Message::SetTableCols(app.table_cols.saturating_sub(1))),
                text(format!("{}", app.table_cols))
                    .font(theme::mono_font_bold())
                    .size(12)
                    .color(pal.text_primary),
                button(text("+").font(theme::mono_font_bold()).size(12))
                    .padding([2, 8])
                    .style(theme::symbol_button)
                    .on_press(Message::SetTableCols(app.table_cols + 1)),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
            row![
                text("Allinea:").size(11).color(pal.text_muted),
                align_chip("l", 'l'),
                align_chip("c", 'c'),
                align_chip("r", 'r'),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
            row![
                button(text("booktabs").size(11))
                    .padding([4, 7])
                    .style(theme::toggle_toolbar_button(app.table_booktabs))
                    .on_press(Message::ToggleTableBooktabs),
                button(text("Intestaz.").size(11))
                    .padding([4, 7])
                    .style(theme::toggle_toolbar_button(app.table_header))
                    .on_press(Message::ToggleTableHeader),
                button(text("caption").size(11))
                    .padding([4, 7])
                    .style(theme::toggle_toolbar_button(app.table_caption_env))
                    .on_press(Message::ToggleTableCaptionEnv),
            ]
            .spacing(4),
            button(
                text(format!("+ Inserisci tabella {}×{}", app.table_rows, app.table_cols))
                    .font(theme::ui_font_semibold())
                    .size(12),
            )
            .padding([6, 10])
            .width(Length::Fill)
            .style(theme::preset_button)
            .on_press(Message::InsertGeneratedTable),
        ]
        .spacing(7),
    )
    .padding(10)
    .width(Length::Fill)
    .style(theme::log_inset);

    let mut bib_col = column![
        text("IMPORTA BIBTEX DA DOI / ARXIV")
            .font(theme::ui_font_semibold())
            .size(10)
            .color(pal.text_faint),
        text_input("DOI (10.1145/...) o arXiv (1706.03762)", &app.bib_import_input)
            .on_input(Message::BibImportInputChanged)
            .on_submit(Message::FetchBibFromIdentifier)
            .size(12)
            .padding([6, 8]),
        button(
            text(if app.is_fetching_bib {
                "Scaricamento BibTeX..."
            } else {
                "Importa voce in .bib"
            })
            .font(theme::ui_font_semibold())
            .size(12),
        )
        .padding([6, 10])
        .width(Length::Fill)
        .style(theme::preset_button)
        .on_press_maybe((!app.is_fetching_bib).then_some(Message::FetchBibFromIdentifier)),
    ]
    .spacing(7);

    if let Some(entry) = &app.last_imported_bib {
        bib_col = bib_col.push(
            button(
                column![
                    text(format!("+ Inserisci \\cite{{{}}}", entry.key))
                        .font(theme::mono_font_bold())
                        .size(11)
                        .color(pal.accent),
                    text(entry.summary()).size(10).color(pal.text_muted),
                ]
                .spacing(2),
            )
            .padding([5, 8])
            .width(Length::Fill)
            .style(theme::symbol_button)
            .on_press(Message::InsertCitationKey(entry.key.clone())),
        );
    }

    let bib_card = container(bib_col)
        .padding(10)
        .width(Length::Fill)
        .style(theme::log_inset);

    let diff_card = container(
        column![
            text("REVISIONI CO-AUTORI (LATEXDIFF)")
                .font(theme::ui_font_semibold())
                .size(10)
                .color(pal.text_faint),
            text("Genera il PDF con aggiunte in blu e cancellazioni barrate in rosso:")
                .size(11)
                .color(pal.text_muted),
            button(text("Evidenzia modifiche rispetto a Git HEAD").size(12))
                .padding([6, 8])
                .width(Length::Fill)
                .style(theme::symbol_button)
                .on_press(Message::RunLatexdiffGit),
            button(text("Confronta con un file .tex precedente...").size(12))
                .padding([6, 8])
                .width(Length::Fill)
                .style(theme::symbol_button)
                .on_press(Message::RunLatexdiffFile),
        ]
        .spacing(6),
    )
    .padding(10)
    .width(Length::Fill)
    .style(theme::log_inset);

    let list = Column::new()
        .spacing(10)
        .padding(Padding {
            top: 0.0,
            right: 6.0,
            bottom: 10.0,
            left: 0.0,
        })
        .push(figure_card)
        .push(table_card)
        .push(bib_card)
        .push(diff_card);

    container(
        column![
            row![
                text("Asset, Bibliografia e Diff")
                    .font(theme::ui_font_semibold())
                    .size(13)
                    .color(pal.text_primary),
                horizontal_space(),
                close_panel_button(Message::ToggleAssets),
            ]
            .align_y(Alignment::Center),
            horizontal_rule(1).style(theme::subtle_rule),
            scrollable(list)
                .direction(scrollable::Direction::Vertical(thin_scrollbar()))
                .style(theme::custom_scrollable)
                .height(Length::Fill),
        ]
        .spacing(10)
        .padding(12),
    )
    .width(Length::Fixed(symbols_panel_width(app.window_size.width)))
    .height(Length::Fill)
    .clip(true)
    .style(theme::side_panel)
    .into()
}

fn view_log_panel(app: &App) -> Element<'_, Message> {
    let pal = theme::palette_for(app.dark_mode);
    let tab = app.tab();

    let lint_label = if tab.lint_warnings.is_empty() {
        "Linter (chktex)".to_string()
    } else {
        format!("Linter ({})", tab.lint_warnings.len())
    };

    let mode_tabs = row![
        button(
            text("Compilazione")
                .font(if app.log_tab == LogTab::Compilation {
                    theme::ui_font_semibold()
                } else {
                    theme::ui_font()
                })
                .size(11),
        )
        .padding([5, 9])
        .style(theme::tab_button(app.log_tab == LogTab::Compilation))
        .on_press(Message::SelectLogTab(LogTab::Compilation)),
        button(
            text(lint_label)
                .font(if app.log_tab == LogTab::Linter {
                    theme::ui_font_semibold()
                } else {
                    theme::ui_font()
                })
                .size(11),
        )
        .padding([5, 9])
        .style(theme::tab_button(app.log_tab == LogTab::Linter))
        .on_press(Message::SelectLogTab(LogTab::Linter)),
        button(
            text("Ortografia")
                .font(if app.log_tab == LogTab::Spellcheck {
                    theme::ui_font_semibold()
                } else {
                    theme::ui_font()
                })
                .size(11),
        )
        .padding([5, 9])
        .style(theme::tab_button(app.log_tab == LogTab::Spellcheck))
        .on_press(Message::SelectLogTab(LogTab::Spellcheck)),
    ]
    .spacing(4);

    let body: Element<'_, Message> = match app.log_tab {
        LogTab::Compilation => {
            let (banner_icon, banner_text, banner_fg, banner_bg) = if app.is_compiling {
                (
                    "●",
                    "Compilazione in corso...".to_string(),
                    pal.accent,
                    pal.accent_soft,
                )
            } else {
                match tab.last_compile_ok {
                    Some(true) => (
                        "✓",
                        match &tab.last_pdf {
                            Some(path) => format!(
                                "Compilazione riuscita · {}",
                                path.file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("PDF pronto")
                            ),
                            None => "Compilazione riuscita".to_string(),
                        },
                        pal.success,
                        pal.success_soft,
                    ),
                    Some(false) => {
                        let msg = match tab.diagnostics.len() {
                            0 => "Compilazione fallita — controlla il log tecnico".to_string(),
                            1 => "1 errore rilevato · clicca per andare alla riga".to_string(),
                            n => format!("{n} errori rilevati · clicca per andare alla riga"),
                        };
                        ("!", msg, pal.error, pal.error_soft)
                    }
                    None => (
                        "●",
                        "Premi Compila (Ctrl+B) per verificare il documento".to_string(),
                        pal.text_muted,
                        pal.bg_elevated,
                    ),
                }
            };

            let summary_banner = container(
                row![
                    text(banner_icon)
                        .font(theme::math_symbol_font())
                        .shaping(Shaping::Advanced)
                        .size(12)
                        .color(banner_fg),
                    text(banner_text)
                        .font(theme::ui_font_semibold())
                        .size(12)
                        .color(banner_fg),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([8, 12])
            .width(Length::Fill)
            .style(theme::status_pill(banner_fg, banner_bg));

            let mut content_col = Column::new().spacing(10).padding(Padding {
                top: 2.0,
                right: 6.0,
                bottom: 12.0,
                left: 0.0,
            });

            if !tab.diagnostics.is_empty() {
                for diag in &tab.diagnostics {
                    let line_pill_label = match diag.line {
                        Some(line) => format!("Riga {line}"),
                        None => "Errore".to_string(),
                    };

                    let mut card_top = row![
                        container(
                            text(line_pill_label)
                                .font(theme::mono_font_bold())
                                .size(11)
                                .color(pal.error),
                        )
                        .padding([2, 7])
                        .style(theme::missing_badge),
                        horizontal_space(),
                    ]
                    .align_y(Alignment::Center);

                    if diag.line.is_some() {
                        card_top = card_top.push(
                            text("Vai alla riga ↗")
                                .font(theme::math_symbol_font())
                                .shaping(Shaping::Advanced)
                                .size(11)
                                .color(pal.accent),
                        );
                    }

                    let card_body = column![
                        card_top,
                        text(&diag.title)
                            .font(theme::ui_font_semibold())
                            .size(13)
                            .color(pal.text_primary),
                        text(&diag.explanation).size(12).color(pal.text_muted),
                        container(
                            text(&diag.raw_excerpt)
                                .font(theme::mono_font())
                                .size(11)
                                .color(pal.text_faint),
                        )
                        .padding([6, 8])
                        .width(Length::Fill)
                        .style(theme::log_inset),
                    ]
                    .spacing(6)
                    .padding([8, 10]);

                    let card_btn = button(card_body)
                        .width(Length::Fill)
                        .style(theme::error_card_button);

                    let card: Element<'_, Message> = if let Some(line) = diag.line {
                        card_btn.on_press(Message::JumpToLine(line)).into()
                    } else {
                        card_btn.into()
                    };

                    content_col = content_col.push(card);
                }
            }

            let raw_toggle_label = if tab.show_raw_log {
                "Nascondi log tecnico"
            } else {
                "Mostra log tecnico"
            };

            content_col = content_col.push(
                button(
                    text(raw_toggle_label)
                        .font(theme::ui_font_semibold())
                        .size(11)
                        .color(pal.text_muted),
                )
                .padding([5, 10])
                .style(theme::ghost_button)
                .on_press(Message::ToggleRawLog),
            );

            if tab.show_raw_log || tab.diagnostics.is_empty() {
                let log_box = container(
                    text(&tab.compile_log)
                        .font(theme::mono_font())
                        .size(11)
                        .color(pal.text_primary),
                )
                .padding(12)
                .width(Length::Fill)
                .style(theme::log_inset);

                content_col = content_col.push(log_box);
            }

            column![
                summary_banner,
                scrollable(content_col)
                    .direction(scrollable::Direction::Vertical(thin_scrollbar()))
                    .style(theme::custom_scrollable)
                    .width(Length::Fill)
                    .height(Length::Fill),
            ]
            .spacing(10)
            .height(Length::Fill)
            .into()
        }
        LogTab::Linter => {
            let mut lint_col = Column::new().spacing(8).padding(Padding {
                top: 2.0,
                right: 6.0,
                bottom: 12.0,
                left: 0.0,
            });

            let toolbar = row![
                text(if app.is_linting {
                    "Analisi tipografica chktex in corso..."
                } else if tab.lint_warnings.is_empty() {
                    "Nessun avviso tipografico rilevato da chktex."
                } else {
                    "Suggerimenti tipografici non bloccanti per la pubblicazione:"
                })
                .size(12)
                .color(pal.text_muted),
                horizontal_space(),
                button(text("Rianalizza").size(11))
                    .padding([4, 9])
                    .style(theme::symbol_button)
                    .on_press(Message::RunChktex),
            ]
            .align_y(Alignment::Center);

            lint_col = lint_col.push(toolbar);

            for w in &tab.lint_warnings {
                let card = button(
                    column![
                        row![
                            container(
                                text(format!("Riga {} · #{}", w.line, w.code))
                                    .font(theme::mono_font_bold())
                                    .size(10)
                                    .color(pal.accent),
                            )
                            .padding([2, 6])
                            .style(theme::add_badge),
                            horizontal_space(),
                            text("Salta ↗")
                                .font(theme::math_symbol_font())
                                .shaping(Shaping::Advanced)
                                .size(11)
                                .color(pal.accent),
                        ]
                        .align_y(Alignment::Center),
                        text(&w.title)
                            .font(theme::ui_font_semibold())
                            .size(12)
                            .color(pal.text_primary),
                        text(&w.detail).size(11).color(pal.text_muted),
                    ]
                    .spacing(4)
                    .padding([6, 8]),
                )
                .width(Length::Fill)
                .style(theme::symbol_button)
                .on_press(Message::JumpToLine(w.line));

                lint_col = lint_col.push(card);
            }

            scrollable(lint_col)
                .direction(scrollable::Direction::Vertical(thin_scrollbar()))
                .style(theme::custom_scrollable)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        }
        LogTab::Spellcheck => {
            let lang_chip = |label: &'static str, code: &'static str| -> Element<'static, Message> {
                let active = app.spell_lang == code;
                button(text(label).font(theme::ui_font_semibold()).size(11))
                    .padding([4, 8])
                    .style(theme::tab_button(active))
                    .on_press(Message::SetSpellLang(code.to_string()))
                    .into()
            };

            let top_controls = row![
                text("Lingua:").size(12).color(pal.text_muted),
                lang_chip("IT", "it_IT"),
                lang_chip("EN", "en_US"),
                lang_chip("IT+EN", "it_IT,en_US"),
                horizontal_space(),
                button(
                    text(if app.is_spellchecking {
                        "Controllo..."
                    } else {
                        "Avvia controllo"
                    })
                    .font(theme::ui_font_semibold())
                    .size(11),
                )
                .padding([5, 10])
                .style(theme::primary_button)
                .on_press_maybe((!app.is_spellchecking).then_some(Message::RunSpellcheck)),
            ]
            .spacing(5)
            .align_y(Alignment::Center);

            let mut spell_col = Column::new().spacing(8).padding(Padding {
                top: 4.0,
                right: 6.0,
                bottom: 12.0,
                left: 0.0,
            });

            match &tab.spell_outcome {
                None => {
                    spell_col = spell_col.push(
                        text("Premi 'Avvia controllo' per verificare il testo ignorando comandi LaTeX e blocchi matematici.")
                            .size(12)
                            .color(pal.text_muted),
                    );
                }
                Some(SpellCheckOutcome::MissingDictionary { lang, hint }) => {
                    spell_col = spell_col.push(
                        container(
                            column![
                                text(format!("Dizionario hunspell '{lang}' non installato"))
                                    .font(theme::ui_font_semibold())
                                    .size(13)
                                    .color(pal.error),
                                text("Per abilitare il correttore ortografico su Arch Linux esegui:")
                                    .size(12)
                                    .color(pal.text_muted),
                                text(hint)
                                    .font(theme::mono_font_bold())
                                    .size(12)
                                    .color(pal.accent),
                            ]
                            .spacing(5),
                        )
                        .padding(10)
                        .width(Length::Fill)
                        .style(theme::log_inset),
                    );
                }
                Some(SpellCheckOutcome::Error(err)) => {
                    spell_col = spell_col.push(text(err).size(12).color(pal.error));
                }
                Some(SpellCheckOutcome::Ok(issues)) if issues.is_empty() => {
                    spell_col = spell_col.push(
                        text("✓ Nessun errore ortografico trovato nel testo.")
                            .font(theme::math_symbol_font())
                            .shaping(Shaping::Advanced)
                            .size(12)
                            .color(pal.success),
                    );
                }
                Some(SpellCheckOutcome::Ok(issues)) => {
                    for issue in issues {
                        let sug_str = if issue.suggestions.is_empty() {
                            "Nessun suggerimento".to_string()
                        } else {
                            format!("Suggerimenti: {}", issue.suggestions.join(", "))
                        };
                        let card = button(
                            column![
                                row![
                                    container(
                                        text(format!("Riga {}", issue.line))
                                            .font(theme::mono_font_bold())
                                            .size(10)
                                            .color(pal.accent),
                                    )
                                    .padding([2, 6])
                                    .style(theme::add_badge),
                                    text(&issue.word)
                                        .font(theme::mono_font_bold())
                                        .size(13)
                                        .color(pal.text_primary),
                                    horizontal_space(),
                                    text("Salta ↗")
                                        .font(theme::math_symbol_font())
                                        .shaping(Shaping::Advanced)
                                        .size(11)
                                        .color(pal.accent),
                                ]
                                .spacing(8)
                                .align_y(Alignment::Center),
                                text(sug_str).size(11).color(pal.text_muted),
                            ]
                            .spacing(4)
                            .padding([6, 8]),
                        )
                        .width(Length::Fill)
                        .style(theme::symbol_button)
                        .on_press(Message::JumpToLine(issue.line));

                        spell_col = spell_col.push(card);
                    }
                }
            }

            column![
                top_controls,
                scrollable(spell_col)
                    .direction(scrollable::Direction::Vertical(thin_scrollbar()))
                    .style(theme::custom_scrollable)
                    .width(Length::Fill)
                    .height(Length::Fill),
            ]
            .spacing(8)
            .height(Length::Fill)
            .into()
        }
    };

    container(
        column![
            row![
                text("Log e Controllo Qualità")
                    .font(theme::ui_font_semibold())
                    .size(13)
                    .color(pal.text_primary),
                horizontal_space(),
                button(text("Pulisci .build").size(11))
                    .padding([3, 8])
                    .style(theme::ghost_button)
                    .on_press(Message::CleanBuild),
                close_panel_button(Message::ToggleLog),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
            mode_tabs,
            horizontal_rule(1).style(theme::subtle_rule),
            body,
        ]
        .spacing(10)
        .padding(14),
    )
    .width(Length::FillPortion(2))
    .height(Length::Fill)
    .clip(true)
    .style(theme::side_panel)
    .into()
}
