use iced::widget::{button, container, rule, scrollable, text_editor};
use iced::{font, Background, Border, Color, Font, Shadow, Theme, Vector};
use std::path::Path;
use std::sync::OnceLock;

/// Detect a crisp TrueType monospace font so Linux fontdb does not fall back to
/// thin PostScript CFF fonts like Nimbus Mono PS.
fn detect_mono_family() -> font::Family {
    static MONO_FAMILY: OnceLock<font::Family> = OnceLock::new();
    *MONO_FAMILY.get_or_init(|| {
        const CANDIDATES: &[(&str, &str)] = &[
            ("/usr/share/fonts/TTF/Hack-Regular.ttf", "Hack"),
            ("/usr/share/fonts/noto/NotoSansMono-Regular.ttf", "Noto Sans Mono"),
            ("/usr/share/fonts/TTF/DejaVuSansMono.ttf", "DejaVu Sans Mono"),
            ("/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf", "Adwaita Mono"),
            ("/usr/share/fonts/liberation/LiberationMono-Regular.ttf", "Liberation Mono"),
        ];
        for &(path, name) in CANDIDATES {
            if Path::new(path).exists() {
                return font::Family::Name(name);
            }
        }
        font::Family::Monospace
    })
}

/// Detect a crisp TrueType sans-serif UI font on Linux.
fn detect_ui_family() -> font::Family {
    static UI_FAMILY: OnceLock<font::Family> = OnceLock::new();
    *UI_FAMILY.get_or_init(|| {
        const CANDIDATES: &[(&str, &str)] = &[
            ("/usr/share/fonts/noto/NotoSans-Regular.ttf", "Noto Sans"),
            ("/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf", "Adwaita Sans"),
            ("/usr/share/fonts/TTF/DejaVuSans.ttf", "DejaVu Sans"),
        ];
        for &(path, name) in CANDIDATES {
            if Path::new(path).exists() {
                return font::Family::Name(name);
            }
        }
        font::Family::SansSerif
    })
}

/// Detect a crisp TrueType font with wide mathematical, logic, and UI symbol coverage.
fn detect_symbol_family() -> font::Family {
    static SYMBOL_FAMILY: OnceLock<font::Family> = OnceLock::new();
    *SYMBOL_FAMILY.get_or_init(|| {
        const CANDIDATES: &[(&str, &str)] = &[
            ("/usr/share/fonts/TTF/DejaVuSans.ttf", "DejaVu Sans"),
            ("/usr/share/fonts/noto/NotoSansMath-Regular.ttf", "Noto Sans Math"),
            ("/usr/share/fonts/TTF/DejaVuMathTeXGyre.ttf", "DejaVu Math TeX Gyre"),
        ];
        for &(path, name) in CANDIDATES {
            if Path::new(path).exists() {
                return font::Family::Name(name);
            }
        }
        font::Family::SansSerif
    })
}

pub fn math_symbol_font() -> Font {
    Font {
        family: detect_symbol_family(),
        weight: font::Weight::Normal,
        ..Font::DEFAULT
    }
}

pub fn mono_font() -> Font {
    Font {
        family: detect_mono_family(),
        weight: font::Weight::Normal,
        ..Font::MONOSPACE
    }
}

pub fn mono_font_bold() -> Font {
    Font {
        family: detect_mono_family(),
        weight: font::Weight::Bold,
        ..Font::MONOSPACE
    }
}

pub fn ui_font() -> Font {
    Font {
        family: detect_ui_family(),
        weight: font::Weight::Normal,
        ..Font::DEFAULT
    }
}

pub fn ui_font_semibold() -> Font {
    Font {
        family: detect_ui_family(),
        weight: font::Weight::Semibold,
        ..Font::DEFAULT
    }
}

pub fn ui_font_bold() -> Font {
    Font {
        family: detect_ui_family(),
        weight: font::Weight::Bold,
        ..Font::DEFAULT
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub bg_base: Color,
    pub bg_surface: Color,
    pub bg_elevated: Color,
    pub bg_gutter: Color,
    pub bg_gutter_active: Color,
    pub bg_pdf_desk: Color,
    pub bg_inset: Color,
    pub border: Color,
    pub border_subtle: Color,
    pub accent: Color,
    pub accent_hover: Color,
    pub accent_soft: Color,
    pub text_primary: Color,
    pub text_muted: Color,
    pub text_faint: Color,
    pub success: Color,
    pub success_soft: Color,
    pub error: Color,
    pub error_soft: Color,
    pub selection: Color,
    pub shadow_color: Color,
}

pub const LIGHT: Palette = Palette {
    bg_base: Color::from_rgb(0.992, 0.984, 0.961),         // #FDFBF5 Avorio luminoso
    bg_surface: Color::from_rgb(0.957, 0.937, 0.894),      // #F4EFE4 Pergamena morbida
    bg_elevated: Color::from_rgb(0.914, 0.882, 0.816),     // #E9E1D0 Sabbia calda
    bg_gutter: Color::from_rgb(0.969, 0.953, 0.914),       // #F7F3E9 Margine carta
    bg_gutter_active: Color::from_rgb(0.929, 0.894, 0.816),// #EDE4D0 Evidenziazione riga
    bg_pdf_desk: Color::from_rgb(0.898, 0.871, 0.812),     // #E5DECF Scrivania anteprima
    bg_inset: Color::from_rgb(0.980, 0.969, 0.941),        // #FAF7F0 Riquadro log
    border: Color::from_rgb(0.835, 0.788, 0.694),          // #D5C9B1 Bordo caldo
    border_subtle: Color::from_rgb(0.886, 0.847, 0.769),   // #E2D8C4 Bordo leggero
    accent: Color::from_rgb(0.706, 0.412, 0.055),          // #B4690E Ambra ricca
    accent_hover: Color::from_rgb(0.561, 0.318, 0.031),    // #8F5108 Ambra profonda
    accent_soft: Color::from_rgba(0.706, 0.412, 0.055, 0.13),
    text_primary: Color::from_rgb(0.102, 0.094, 0.082),    // #1A1815 Inchiostro scuro
    text_muted: Color::from_rgb(0.380, 0.341, 0.286),      // #615749 Seppia leggibile
    text_faint: Color::from_rgb(0.545, 0.494, 0.420),      // #8B7E6B Numeri riga inattivi
    success: Color::from_rgb(0.165, 0.490, 0.302),         // #2A7D4D Verde bosco
    success_soft: Color::from_rgba(0.165, 0.490, 0.302, 0.14),
    error: Color::from_rgb(0.725, 0.220, 0.173),           // #B9382C Rosso mattone
    error_soft: Color::from_rgba(0.725, 0.220, 0.173, 0.14),
    selection: Color::from_rgba(0.847, 0.659, 0.235, 0.32),
    shadow_color: Color::from_rgba(0.20, 0.15, 0.07, 0.14),
};

pub const DARK: Palette = Palette {
    bg_base: Color::from_rgb(0.106, 0.102, 0.090),         // #1B1A17 Inchiostro notte
    bg_surface: Color::from_rgb(0.137, 0.133, 0.118),      // #23221E Superficie calda
    bg_elevated: Color::from_rgb(0.192, 0.184, 0.161),     // #312F29 Elevato
    bg_gutter: Color::from_rgb(0.122, 0.118, 0.102),       // #1F1E1A Margine scuro
    bg_gutter_active: Color::from_rgb(0.204, 0.184, 0.145),// #342F25 Riga attiva
    bg_pdf_desk: Color::from_rgb(0.082, 0.078, 0.071),     // #151412 Sfondo PDF
    bg_inset: Color::from_rgb(0.094, 0.090, 0.078),        // #181714 Riquadro log
    border: Color::from_rgb(0.243, 0.227, 0.192),          // #3E3A31 Bordo scuro
    border_subtle: Color::from_rgb(0.188, 0.176, 0.149),   // #302D26 Bordo leggero
    accent: Color::from_rgb(0.875, 0.627, 0.239),          // #DFA03D Oro caldo
    accent_hover: Color::from_rgb(0.784, 0.545, 0.169),    // #C88B2B Oro scuro
    accent_soft: Color::from_rgba(0.875, 0.627, 0.239, 0.16),
    text_primary: Color::from_rgb(0.941, 0.922, 0.882),    // #F0EBE1 Carta chiara
    text_muted: Color::from_rgb(0.659, 0.624, 0.561),      // #A89F8F Grigio caldo
    text_faint: Color::from_rgb(0.451, 0.424, 0.376),      // #736C60 Numeri riga inattivi
    success: Color::from_rgb(0.345, 0.745, 0.506),         // #58BE81 Verde menta
    success_soft: Color::from_rgba(0.345, 0.745, 0.506, 0.16),
    error: Color::from_rgb(0.910, 0.435, 0.388),           // #E86F63 Corallo
    error_soft: Color::from_rgba(0.910, 0.435, 0.388, 0.16),
    selection: Color::from_rgba(0.875, 0.627, 0.239, 0.28),
    shadow_color: Color::from_rgba(0.0, 0.0, 0.0, 0.42),
};

pub fn palette(theme: &Theme) -> Palette {
    if theme.extended_palette().is_dark {
        DARK
    } else {
        LIGHT
    }
}

pub fn palette_for(dark: bool) -> Palette {
    if dark {
        DARK
    } else {
        LIGHT
    }
}

pub fn app_background(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.bg_base)),
        text_color: Some(p.text_primary),
        ..Default::default()
    }
}

pub fn surface_bar(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.bg_surface)),
        text_color: Some(p.text_primary),
        ..Default::default()
    }
}

pub fn gutter_background(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.bg_gutter)),
        text_color: Some(p.text_faint),
        ..Default::default()
    }
}

pub fn gutter_active_line(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.bg_gutter_active)),
        text_color: Some(p.accent),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

pub fn gutter_error_line(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.error_soft)),
        text_color: Some(p.error),
        border: Border {
            color: p.error,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

pub fn editor_error_line_band(theme: &Theme) -> container::Style {
    let is_dark = theme.extended_palette().is_dark;
    let bg = if is_dark {
        Color::from_rgba(0.910, 0.365, 0.325, 0.20)
    } else {
        Color::from_rgba(0.820, 0.210, 0.165, 0.14)
    };
    container::Style {
        background: Some(Background::Color(bg)),
        ..Default::default()
    }
}

pub fn editor_sync_line_band(theme: &Theme) -> container::Style {
    let is_dark = theme.extended_palette().is_dark;
    let bg = if is_dark {
        Color::from_rgba(0.875, 0.560, 0.310, 0.22)
    } else {
        Color::from_rgba(0.745, 0.355, 0.155, 0.16)
    };
    container::Style {
        background: Some(Background::Color(bg)),
        ..Default::default()
    }
}

pub fn file_tab_strip(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.bg_gutter)),
        text_color: Some(p.text_primary),
        ..Default::default()
    }
}

pub fn file_tab_card(active: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = palette(theme);
        if active {
            container::Style {
                background: Some(Background::Color(p.bg_base)),
                text_color: Some(p.text_primary),
                border: Border {
                    color: p.border,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                shadow: Shadow {
                    color: p.shadow_color,
                    offset: Vector::new(0.0, 1.0),
                    blur_radius: 4.0,
                },
            }
        } else {
            container::Style {
                background: Some(Background::Color(Color::TRANSPARENT)),
                text_color: Some(p.text_muted),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            }
        }
    }
}

pub fn file_tab_select_button(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        let (bg, text_color) = if active {
            (None, p.text_primary)
        } else {
            match status {
                button::Status::Hovered => (Some(Background::Color(p.bg_elevated)), p.text_primary),
                _ => (None, p.text_muted),
            }
        };
        button::Style {
            background: bg,
            text_color,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 6.0.into(),
            },
            shadow: Shadow::default(),
        }
    }
}

pub fn context_menu_item(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let (bg, text_color) = match status {
        button::Status::Hovered => (Some(Background::Color(p.accent_soft)), p.accent),
        button::Status::Pressed => (Some(Background::Color(p.bg_elevated)), p.accent),
        _ => (None, p.text_primary),
    };
    button::Style {
        background: bg,
        text_color,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 6.0.into(),
        },
        shadow: Shadow::default(),
    }
}

pub fn error_card_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let (bg, border_color) = match status {
        button::Status::Hovered => (p.error_soft, p.error),
        button::Status::Pressed => (p.bg_base, p.error),
        _ => (p.bg_base, p.border_subtle),
    };

    button::Style {
        background: Some(Background::Color(bg)),
        text_color: p.text_primary,
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 8.0.into(),
        },
        shadow: Shadow::default(),
    }
}

pub fn welcome_card(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.bg_surface)),
        text_color: Some(p.text_primary),
        border: Border {
            color: p.border,
            width: 1.0,
            radius: 18.0.into(),
        },
        shadow: Shadow {
            color: p.shadow_color,
            offset: Vector::new(0.0, 12.0),
            blur_radius: 32.0,
        },
    }
}

pub fn brand_badge(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.accent)),
        text_color: Some(Color::WHITE),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 14.0.into(),
        },
        shadow: Shadow {
            color: p.shadow_color,
            offset: Vector::new(0.0, 4.0),
            blur_radius: 10.0,
        },
    }
}

pub fn small_brand_badge(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.accent)),
        text_color: Some(Color::WHITE),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

pub fn icon_chip(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.accent_soft)),
        text_color: Some(p.accent),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    }
}

pub fn shortcut_pill(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.bg_elevated)),
        text_color: Some(p.text_muted),
        border: Border {
            color: p.border_subtle,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

pub fn installed_badge(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.success_soft)),
        text_color: Some(p.success),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 5.0.into(),
        },
        ..Default::default()
    }
}

pub fn missing_badge(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.error_soft)),
        text_color: Some(p.error),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 5.0.into(),
        },
        ..Default::default()
    }
}

pub fn add_badge(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.accent_soft)),
        text_color: Some(p.accent),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 5.0.into(),
        },
        ..Default::default()
    }
}

pub fn status_pill(color: Color, bg: Color) -> impl Fn(&Theme) -> container::Style {
    move |_theme| container::Style {
        background: Some(Background::Color(bg)),
        text_color: Some(color),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    }
}

pub fn side_panel(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.bg_surface)),
        text_color: Some(p.text_primary),
        ..Default::default()
    }
}

pub fn pdf_desk(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.bg_pdf_desk)),
        text_color: Some(p.text_primary),
        border: Border {
            color: p.border_subtle,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    }
}

pub fn log_inset(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.bg_inset)),
        text_color: Some(p.text_primary),
        border: Border {
            color: p.border_subtle,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    }
}

pub fn pdf_page_frame(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(Color::WHITE)),
        text_color: Some(p.text_primary),
        border: Border {
            color: p.border,
            width: 1.0,
            radius: 4.0.into(),
        },
        shadow: Shadow {
            color: p.shadow_color,
            offset: Vector::new(0.0, 5.0),
            blur_radius: 14.0,
        },
    }
}

pub fn popup_box(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.bg_surface)),
        text_color: Some(p.text_primary),
        border: Border {
            color: p.border,
            width: 1.0,
            radius: 10.0.into(),
        },
        shadow: Shadow {
            color: p.shadow_color,
            offset: Vector::new(0.0, 8.0),
            blur_radius: 20.0,
        },
    }
}

pub fn suggestion_row(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        let (bg, text_color) = if selected {
            (Some(Background::Color(p.accent_soft)), p.accent)
        } else {
            match status {
                button::Status::Hovered => (Some(Background::Color(p.bg_elevated)), p.text_primary),
                _ => (None, p.text_primary),
            }
        };

        button::Style {
            background: bg,
            text_color,
            border: Border {
                color: if selected {
                    p.accent
                } else {
                    Color::TRANSPARENT
                },
                width: if selected { 1.0 } else { 0.0 },
                radius: 6.0.into(),
            },
            shadow: Shadow::default(),
        }
    }
}

pub fn action_card_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let (bg, border_color, shadow) = match status {
        button::Status::Hovered => (
            p.bg_elevated,
            p.accent,
            Shadow {
                color: p.shadow_color,
                offset: Vector::new(0.0, 4.0),
                blur_radius: 10.0,
            },
        ),
        button::Status::Pressed => (p.bg_base, p.accent, Shadow::default()),
        _ => (p.bg_base, p.border, Shadow::default()),
    };

    button::Style {
        background: Some(Background::Color(bg)),
        text_color: p.text_primary,
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow,
    }
}

pub fn symbol_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let (bg, border_color) = match status {
        button::Status::Hovered => (p.bg_elevated, p.accent),
        button::Status::Pressed => (p.bg_base, p.accent),
        _ => (p.bg_base, p.border_subtle),
    };

    button::Style {
        background: Some(Background::Color(bg)),
        text_color: p.text_primary,
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 8.0.into(),
        },
        shadow: Shadow::default(),
    }
}

pub fn preset_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let (bg, border_color, text_color) = match status {
        button::Status::Hovered => (p.accent, p.accent, Color::WHITE),
        button::Status::Pressed => (p.accent_hover, p.accent_hover, Color::WHITE),
        _ => (p.accent_soft, p.border_subtle, p.accent),
    };

    button::Style {
        background: Some(Background::Color(bg)),
        text_color,
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 7.0.into(),
        },
        shadow: Shadow::default(),
    }
}

pub fn tab_button(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        let (bg, text_color, border_color) = if active {
            (Some(Background::Color(p.bg_base)), p.accent, p.border)
        } else {
            match status {
                button::Status::Hovered => (
                    Some(Background::Color(p.bg_elevated)),
                    p.text_primary,
                    Color::TRANSPARENT,
                ),
                _ => (None, p.text_muted, Color::TRANSPARENT),
            }
        };

        button::Style {
            background: bg,
            text_color,
            border: Border {
                color: border_color,
                width: 1.0,
                radius: 7.0.into(),
            },
            shadow: Shadow::default(),
        }
    }
}

pub fn toggle_toolbar_button(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = palette(theme);
        let (bg, text_color, border_color) = if active {
            (Some(Background::Color(p.accent_soft)), p.accent, p.accent)
        } else {
            match status {
                button::Status::Hovered => (
                    Some(Background::Color(p.bg_elevated)),
                    p.text_primary,
                    Color::TRANSPARENT,
                ),
                button::Status::Pressed => (
                    Some(Background::Color(p.bg_base)),
                    p.text_primary,
                    Color::TRANSPARENT,
                ),
                _ => (None, p.text_muted, Color::TRANSPARENT),
            }
        };

        button::Style {
            background: bg,
            text_color,
            border: Border {
                color: border_color,
                width: if active { 1.0 } else { 0.0 },
                radius: 7.0.into(),
            },
            shadow: Shadow::default(),
        }
    }
}

pub fn primary_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let bg = match status {
        button::Status::Hovered => p.accent_hover,
        button::Status::Disabled => p.bg_elevated,
        _ => p.accent,
    };

    let text_color = match status {
        button::Status::Disabled => p.text_muted,
        _ => Color::WHITE,
    };

    button::Style {
        background: Some(Background::Color(bg)),
        text_color,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 8.0.into(),
        },
        shadow: match status {
            button::Status::Disabled => Shadow::default(),
            _ => Shadow {
                color: p.shadow_color,
                offset: Vector::new(0.0, 2.0),
                blur_radius: 6.0,
            },
        },
    }
}

pub fn ghost_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let (bg, text_color) = match status {
        button::Status::Hovered => (Some(Background::Color(p.bg_elevated)), p.text_primary),
        button::Status::Pressed => (Some(Background::Color(p.bg_base)), p.text_primary),
        _ => (None, p.text_muted),
    };

    button::Style {
        background: bg,
        text_color,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 7.0.into(),
        },
        shadow: Shadow::default(),
    }
}

pub fn hairline_rule(theme: &Theme) -> rule::Style {
    let p = palette(theme);
    rule::Style {
        color: p.border,
        width: 1,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
    }
}

pub fn subtle_rule(theme: &Theme) -> rule::Style {
    let p = palette(theme);
    rule::Style {
        color: p.border_subtle,
        width: 1,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
    }
}

pub fn editor_style(theme: &Theme, _status: text_editor::Status) -> text_editor::Style {
    let p = palette(theme);
    text_editor::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        icon: p.text_muted,
        placeholder: p.text_faint,
        value: p.text_primary,
        selection: p.selection,
    }
}

pub fn custom_scrollable(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let p = palette(theme);

    let (v_hovered, h_hovered) = match status {
        scrollable::Status::Active => (false, false),
        scrollable::Status::Hovered {
            is_horizontal_scrollbar_hovered,
            is_vertical_scrollbar_hovered,
        } => (is_vertical_scrollbar_hovered, is_horizontal_scrollbar_hovered),
        scrollable::Status::Dragged {
            is_horizontal_scrollbar_dragged,
            is_vertical_scrollbar_dragged,
        } => (is_vertical_scrollbar_dragged, is_horizontal_scrollbar_dragged),
    };

    let rail = |active: bool| scrollable::Rail {
        background: Some(Background::Color(Color::TRANSPARENT)),
        border: Border::default(),
        scroller: scrollable::Scroller {
            color: if active { p.accent } else { p.border },
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 5.0.into(),
            },
        },
    };

    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail(v_hovered),
        horizontal_rail: rail(h_hovered),
        gap: None,
    }
}
