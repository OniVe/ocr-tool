//! The whole visible surface of the GUI.
//!
//! Pixel/brutalist skin from the “Pixel Poetry” reference: cream paper, 2 px
//! black frames, zero corner radii, Press Start 2P for short labels and IBM Plex
//! Mono for body text. Backend state lives in `main.rs`; this module only reads
//! and mutates it through the `OcrApp` fields.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{
    Align, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame,
    Label, Layout, Margin, Pos2, Rect, RichText, Sense, Shadow, Stroke, StrokeKind, Vec2,
};

use ocr_app::runtime::runtime_download_mb;
use ocr_app::setup::ModelStatus;
use ocr_core::{BlockExtraction, Device, OutputFormat};
use ocr_engine::{DEFAULT_IMAGE_MAX_TOKENS, FAST_IMAGE_MAX_TOKENS};

use crate::{gui_log, prefs, status_text, FailureKind, OcrApp, StatusPhase};

// ---------------------------------------------------------------------------
// Pixel Poetry tokens
// ---------------------------------------------------------------------------

const CREAM: Color32 = Color32::from_rgb(0xF2, 0xEF, 0xE7);
const PANEL: Color32 = Color32::from_rgb(0xFA, 0xF8, 0xF2);
const INK: Color32 = Color32::from_rgb(0x11, 0x11, 0x11);
const MUTED: Color32 = Color32::from_rgb(0x5A, 0x5A, 0x56);
const GREY: Color32 = Color32::from_rgb(0xA8, 0xA8, 0xA3);
const DISABLED: Color32 = Color32::from_rgb(0xD6, 0xD3, 0xCB);
const WHITE: Color32 = Color32::WHITE;

const BORDER: f32 = 2.0;
const RAIL_WIDTH: f32 = 252.0;
const RESULT_WIDTH: f32 = 352.0;

/// The custom title bar replaces the native Windows decoration. Kept at the
/// previous header height so the brand block is not clipped.
const HEADER_HEIGHT: f32 = 78.0;
const WINDOW_CONTROL: f32 = 30.0;
/// Grab width of the borderless resize frame at every edge and corner.
const RESIZE_EDGE: f32 = 7.0;

// Embedded OFL fonts, so a packaged exe never depends on a system font.
const PRESS_START: &[u8] = include_bytes!("../../../assets/fonts/PressStart2P-Regular.ttf");
const PLEX_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/IBMPlexMono-Regular.ttf");
const PLEX_BOLD: &[u8] = include_bytes!("../../../assets/fonts/IBMPlexMono-Bold.ttf");

fn pixel_family() -> FontFamily {
    FontFamily::Name("pixel".into())
}

fn bold_family() -> FontFamily {
    FontFamily::Name("mono-bold".into())
}

fn pixel_text(text: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(text)
        .font(FontId::new(size, pixel_family()))
        .color(color)
}

fn mono_text(text: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(text)
        .font(FontId::new(size, FontFamily::Monospace))
        .color(color)
}

fn mono_bold_text(text: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(text)
        .font(FontId::new(size, bold_family()))
        .color(color)
}

/// Register the three OFL faces once. `pixel` is a named family; the mono files
/// replace the default proportional and monospace families, with egui's Latin
/// fallbacks kept after them for rare glyphs.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "press-start-2p".to_owned(),
        Arc::new(FontData::from_static(PRESS_START)),
    );
    fonts.font_data.insert(
        "ibm-plex-mono".to_owned(),
        Arc::new(FontData::from_static(PLEX_REGULAR)),
    );
    fonts.font_data.insert(
        "ibm-plex-mono-bold".to_owned(),
        Arc::new(FontData::from_static(PLEX_BOLD)),
    );

    fonts
        .families
        .insert(pixel_family(), vec!["press-start-2p".to_owned()]);
    fonts.families.insert(
        bold_family(),
        vec!["ibm-plex-mono-bold".to_owned(), "ibm-plex-mono".to_owned()],
    );
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "ibm-plex-mono".to_owned());
    }
    ctx.set_fonts(fonts);
}

/// One flat, light, monochrome visual language. Applied to both egui themes so
/// a dark desktop preference cannot reintroduce blue or rounded corners.
pub fn apply_style(ctx: &egui::Context) {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        ctx.style_mut_of(theme, |style| {
            style.spacing.item_spacing = Vec2::new(8.0, 8.0);
            style.spacing.button_padding = Vec2::new(12.0, 8.0);
            style.spacing.interact_size = Vec2::new(0.0, 38.0);
            style.spacing.slider_width = 130.0;
            style.spacing.text_edit_width = 260.0;
            style.spacing.icon_width = 18.0;
            style.spacing.indent = 14.0;
            style.spacing.scroll.foreground_color = false;
            style.spacing.scroll.floating = false;
            style.spacing.scroll.bar_width = 8.0;

            style
                .text_styles
                .insert(egui::TextStyle::Heading, FontId::new(16.0, pixel_family()));
            style.text_styles.insert(
                egui::TextStyle::Body,
                FontId::new(13.0, FontFamily::Monospace),
            );
            style.text_styles.insert(
                egui::TextStyle::Button,
                FontId::new(11.0, FontFamily::Monospace),
            );
            style.text_styles.insert(
                egui::TextStyle::Small,
                FontId::new(10.0, FontFamily::Monospace),
            );
            style.text_styles.insert(
                egui::TextStyle::Monospace,
                FontId::new(11.0, FontFamily::Monospace),
            );

            let visuals = &mut style.visuals;
            visuals.dark_mode = false;
            visuals.panel_fill = CREAM;
            visuals.window_fill = PANEL;
            visuals.extreme_bg_color = PANEL;
            visuals.faint_bg_color = CREAM;
            visuals.code_bg_color = CREAM;
            visuals.window_corner_radius = CornerRadius::ZERO;
            visuals.menu_corner_radius = CornerRadius::ZERO;
            visuals.window_stroke = Stroke::new(BORDER, INK);
            visuals.window_shadow = Shadow::NONE;
            visuals.popup_shadow = Shadow::NONE;
            visuals.selection.bg_fill = GREY;
            visuals.selection.stroke = Stroke::new(1.0_f32, INK);
            visuals.text_cursor.stroke = Stroke::new(2.0_f32, INK);
            visuals.hyperlink_color = INK;
            visuals.warn_fg_color = INK;
            visuals.error_fg_color = INK;

            for widget in [
                &mut visuals.widgets.inactive,
                &mut visuals.widgets.hovered,
                &mut visuals.widgets.active,
                &mut visuals.widgets.noninteractive,
                &mut visuals.widgets.open,
            ] {
                widget.corner_radius = CornerRadius::ZERO;
                widget.expansion = 0.0;
            }

            visuals.widgets.noninteractive.bg_fill = PANEL;
            visuals.widgets.noninteractive.weak_bg_fill = PANEL;
            visuals.widgets.noninteractive.bg_stroke = Stroke::new(BORDER, INK);
            visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, INK);
            visuals.widgets.inactive.bg_fill = GREY;
            visuals.widgets.inactive.weak_bg_fill = PANEL;
            visuals.widgets.inactive.bg_stroke = Stroke::new(BORDER, INK);
            visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, INK);
            visuals.widgets.hovered.bg_fill = INK;
            visuals.widgets.hovered.weak_bg_fill = INK;
            visuals.widgets.hovered.bg_stroke = Stroke::new(BORDER, INK);
            visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, WHITE);
            visuals.widgets.active.bg_fill = GREY;
            visuals.widgets.active.weak_bg_fill = GREY;
            visuals.widgets.active.bg_stroke = Stroke::new(BORDER, INK);
            visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, INK);
            visuals.widgets.open.bg_fill = PANEL;
            visuals.widgets.open.weak_bg_fill = PANEL;
            visuals.widgets.open.bg_stroke = Stroke::new(BORDER, INK);
            visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, INK);
        });
    }
}

// ---------------------------------------------------------------------------
// Reusable pixel widgets
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum ButtonKind {
    Primary,
    Secondary,
}

fn button(
    ui: &mut egui::Ui,
    label: &str,
    kind: ButtonKind,
    enabled: bool,
    min_width: f32,
    height: f32,
    size: f32,
    pixel: bool,
    full_width: bool,
) -> egui::Response {
    let enabled = enabled && ui.is_enabled();
    let font = if pixel {
        FontId::new(size, pixel_family())
    } else {
        FontId::new(size, FontFamily::Monospace)
    };
    let base_color = Color32::PLACEHOLDER;
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, base_color);
    let pad = if height < 34.0 { 7.0 } else { 12.0 };
    let width = if full_width {
        ui.available_width().max(min_width)
    } else {
        (galley.size().x + pad * 2.0).max(min_width)
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let hovered = enabled && response.hovered();
    let pressed = enabled && response.is_pointer_button_down_on();
    // Short state transitions without moving the layout. Disabled controls keep
    // their flat look: no animation, no cursor.
    let hover_t = if enabled {
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), hovered, 0.10)
    } else {
        0.0
    };
    let press_t = if enabled {
        ui.ctx()
            .animate_bool_with_time(response.id.with("press"), pressed, 0.04)
    } else {
        0.0
    };
    let (normal_fill, normal_fg, hover_fill, hover_fg) = match kind {
        ButtonKind::Primary => (INK, WHITE, PANEL, INK),
        ButtonKind::Secondary => (PANEL, INK, INK, WHITE),
    };
    let (fill, fg) = if !enabled {
        (DISABLED, MUTED)
    } else {
        let fill = normal_fill
            .lerp_to_gamma(hover_fill, hover_t)
            .lerp_to_gamma(GREY, press_t);
        let hover_fg = if hover_t >= 0.5 { hover_fg } else { normal_fg };
        let fg = if press_t >= 0.5 { INK } else { hover_fg };
        (fill, fg)
    };
    let border_color = if enabled { INK } else { GREY };
    let face = rect.shrink(1.0 * press_t);
    ui.painter().rect(
        rect,
        CornerRadius::ZERO,
        fill,
        Stroke::new(BORDER, border_color),
        StrokeKind::Inside,
    );
    let pos = face.center() - galley.size() * 0.5;
    ui.painter().galley(pos, galley, fg);
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

fn pixel_button(ui: &mut egui::Ui, label: &str, kind: ButtonKind, enabled: bool) -> egui::Response {
    button(ui, label, kind, enabled, 0.0, 40.0, 9.0, true, false)
}

fn pixel_button_full(
    ui: &mut egui::Ui,
    label: &str,
    kind: ButtonKind,
    enabled: bool,
) -> egui::Response {
    button(ui, label, kind, enabled, 0.0, 46.0, 10.0, true, true)
}

fn mono_button(ui: &mut egui::Ui, label: &str, kind: ButtonKind, enabled: bool) -> egui::Response {
    button(ui, label, kind, enabled, 0.0, 36.0, 11.0, false, false)
}

fn small_pixel_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    button(
        ui,
        label,
        ButtonKind::Secondary,
        enabled,
        30.0,
        30.0,
        10.0,
        true,
        false,
    )
}

fn segment(
    ui: &mut egui::Ui,
    label: &str,
    selected: bool,
    enabled: bool,
    width: f32,
) -> egui::Response {
    button(
        ui,
        label,
        if selected {
            ButtonKind::Primary
        } else {
            ButtonKind::Secondary
        },
        enabled,
        width,
        34.0,
        8.0,
        true,
        false,
    )
}

fn badge(ui: &mut egui::Ui, text: &str, solid: bool) -> egui::Response {
    let fill = if solid { INK } else { PANEL };
    let fg = if solid { WHITE } else { INK };
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), FontId::new(8.0, pixel_family()), fg);
    let size = Vec2::new(galley.size().x + 14.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect(
        rect,
        CornerRadius::ZERO,
        fill,
        Stroke::new(BORDER, INK),
        StrokeKind::Inside,
    );
    ui.painter()
        .galley(rect.center() - galley.size() * 0.5, galley, fg);
    response
}

fn note(ui: &mut egui::Ui, text: &str, important: bool) {
    let fill = if important { CREAM } else { PANEL };
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(BORDER, INK))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(mono_text(text, 10.0, INK));
        });
}

fn divider(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, BORDER), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, INK);
}

fn progress_bar(ui: &mut egui::Ui, fraction: f32, height: f32) {
    let fraction = fraction.clamp(0.0, 1.0);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, PANEL);
    let inner = rect.shrink(4.0);
    if fraction > 0.0 {
        let filled = Rect::from_min_size(
            inner.min,
            Vec2::new(inner.width() * fraction, inner.height()),
        );
        ui.painter().rect_filled(filled, CornerRadius::ZERO, INK);
    }
    ui.painter().rect(
        rect,
        CornerRadius::ZERO,
        Color32::TRANSPARENT,
        Stroke::new(BORDER, INK),
        StrokeKind::Inside,
    );
}

fn check_row(ui: &mut egui::Ui, checked: &mut bool, label: &str, hint: &str) -> bool {
    let mut toggled = false;
    ui.horizontal(|ui| {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
        let label_response = ui.add(Label::new(mono_text(label, 11.0, INK)).sense(Sense::click()));
        if response.clicked() || label_response.clicked() {
            *checked = !*checked;
            toggled = true;
        }
        let hovered = response.hovered() || label_response.hovered();
        let pressed =
            response.is_pointer_button_down_on() || label_response.is_pointer_button_down_on();
        let hover_t = ui
            .ctx()
            .animate_bool_with_time(response.id.with("hover"), hovered, 0.10);
        let press_t = ui
            .ctx()
            .animate_bool_with_time(response.id.with("press"), pressed, 0.04);
        let normal = if *checked { INK } else { PANEL };
        let hover = if *checked { MUTED } else { DISABLED };
        let fill = normal.lerp_to_gamma(hover, hover_t);
        let fg = if *checked { WHITE } else { INK };
        let face = rect.shrink(1.0 * press_t);
        ui.painter().rect(
            rect,
            CornerRadius::ZERO,
            fill,
            Stroke::new(BORDER, INK),
            StrokeKind::Inside,
        );
        if *checked {
            ui.painter().text(
                face.center(),
                Align2::CENTER_CENTER,
                "X",
                FontId::new(9.0, pixel_family()),
                fg,
            );
        }
        response.on_hover_text(hint);
        label_response.on_hover_text(hint);
    });
    toggled
}

fn toggle_button(ui: &mut egui::Ui, open: &mut bool, label: &str) -> bool {
    let text = if *open {
        format!("{label}  [-]")
    } else {
        format!("{label}  [+]")
    };
    let response = mono_button(ui, &text, ButtonKind::Secondary, true);
    if response.clicked() {
        *open = !*open;
        true
    } else {
        false
    }
}

fn window_title_bar(ui: &mut egui::Ui, title: &str, closable: bool) -> bool {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 34.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, PANEL);
    ui.painter().text(
        Pos2::new(rect.left() + 12.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        FontId::new(10.0, pixel_family()),
        INK,
    );
    let dots_start = rect.right() - if closable { 92.0 } else { 54.0 };
    for index in 0..3 {
        ui.painter().circle_filled(
            Pos2::new(dots_start + index as f32 * 12.0, rect.center().y),
            3.5,
            INK,
        );
    }
    let mut close = false;
    if closable {
        let close_rect = Rect::from_center_size(
            Pos2::new(rect.right() - 18.0, rect.center().y),
            Vec2::splat(24.0),
        );
        let response = ui.interact(
            close_rect,
            ui.id().with(("window-close", title)),
            Sense::click(),
        );
        let hovered = response.hovered();
        let pressed = response.is_pointer_button_down_on();
        let hover_t = ui
            .ctx()
            .animate_bool_with_time(response.id.with("hover"), hovered, 0.10);
        let press_t = ui
            .ctx()
            .animate_bool_with_time(response.id.with("press"), pressed, 0.04);
        let fill = PANEL
            .lerp_to_gamma(INK, hover_t)
            .lerp_to_gamma(GREY, press_t);
        let fg = if press_t >= 0.5 || hover_t < 0.5 {
            INK
        } else {
            WHITE
        };
        let face = close_rect.shrink(1.0 * press_t);
        ui.painter().rect(
            close_rect,
            CornerRadius::ZERO,
            fill,
            Stroke::new(BORDER, INK),
            StrokeKind::Inside,
        );
        ui.painter().text(
            face.center(),
            Align2::CENTER_CENTER,
            "×",
            FontId::new(10.0, pixel_family()),
            fg,
        );
        close = response.clicked();
    }
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 1.0,
        Stroke::new(BORDER, INK),
    );
    close
}

/// A framed window with the reference title bar. The body gets the standard
/// 14/16 px inner margin.
fn pixel_window<R>(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(BORDER, INK))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(0)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            window_title_bar(ui, title, false);
            Frame::new()
                .inner_margin(Margin::symmetric(16, 14))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    add(ui)
                })
                .inner
        })
        .inner
}

fn window_frame() -> Frame {
    Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(BORDER, INK))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(0)
        .shadow(Shadow::NONE)
}

// ---------------------------------------------------------------------------
// Screen skeleton
// ---------------------------------------------------------------------------

/// Entry point called from `eframe::App::update`.
pub fn draw(app: &mut OcrApp, ctx: &egui::Context) {
    if !app.fonts_installed {
        install_fonts(ctx);
        app.fonts_installed = true;
        // `set_fonts` is applied at the start of the next pass; drawing a pixel
        // label now would ask for a family that is not bound yet.
        ctx.request_repaint();
        return;
    }
    ctx.set_theme(egui::Theme::Light);
    apply_style(ctx);

    header(&mut app.show_help, ctx);
    footer(app, ctx);
    left_rail(app, ctx);
    result_panel(app, ctx);

    egui::CentralPanel::default()
        .frame(
            Frame::new()
                .fill(CREAM)
                .inner_margin(Margin::symmetric(16, 14)),
        )
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("main-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if !app.setup_complete() {
                        setup_window(app, ui);
                        ui.add_space(12.0);
                    }
                    documents_window(app, ui);
                    ui.add_space(12.0);
                    output_window(app, ui);
                    ui.add_space(10.0);
                });
        });

    if app.show_advanced {
        advanced_window(app, ctx);
    }
    if app.show_help {
        help_window(app, ctx);
    }
    // Last, so the thin edge strips sit above every panel.
    frameless_resize(ctx);
}

fn header(show_help: &mut bool, ctx: &egui::Context) {
    let maximized = ctx
        .input(|input| input.viewport().maximized)
        .unwrap_or(false);
    egui::TopBottomPanel::top("header")
        .exact_height(HEADER_HEIGHT)
        .frame(Frame::new().fill(CREAM).inner_margin(0))
        .show(ctx, |ui| {
            // The native decoration is off, so this row is the title bar. The
            // full-width response is registered first: controls allocated later
            // win egui's hit test, so their clicks are never eaten by the drag.
            let bar = ui.interact(
                ui.max_rect(),
                ui.id().with("window-title-bar"),
                Sense::click_and_drag(),
            );
            let mut control_rects: Vec<Rect> = Vec::new();
            let mut action = None;
            Frame::new()
                .inner_margin(Margin::symmetric(20, 10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(pixel_text("OCR TOOL", 22.0, INK));
                            ui.label(mono_text(
                                "ЛОКАЛЬНОЕ РАСПОЗНАВАНИЕ ДОКУМЕНТОВ · БЕЗ ОБЛАКА",
                                9.0,
                                MUTED,
                            ));
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.scope(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                let close = window_control(ui, WindowGlyph::Close, "Закрыть");
                                if close.clicked() {
                                    action = Some(WindowAction::Close);
                                }
                                let (glyph, tooltip) = if maximized {
                                    (WindowGlyph::Restore, "Восстановить")
                                } else {
                                    (WindowGlyph::Maximize, "Развернуть")
                                };
                                let maximize = window_control(ui, glyph, tooltip);
                                if maximize.clicked() {
                                    action = Some(WindowAction::MaximizeToggle);
                                }
                                let minimize =
                                    window_control(ui, WindowGlyph::Minimize, "Свернуть");
                                if minimize.clicked() {
                                    action = Some(WindowAction::Minimize);
                                }
                                control_rects.extend([close.rect, maximize.rect, minimize.rect]);
                            });
                            ui.add_space(10.0);
                            let help =
                                small_pixel_button(ui, "?", true).on_hover_text("Как это работает");
                            if help.clicked() {
                                *show_help = !*show_help;
                            }
                            control_rects.push(help.rect);
                            ui.add_space(8.0);
                            ui.label(mono_text(
                                format!("v{} / ЛОКАЛЬНО", env!("CARGO_PKG_VERSION")),
                                9.0,
                                MUTED,
                            ));
                            badge(ui, "ЛОКАЛЬНО", true);
                        });
                    });
                });
            // A press that began on a control must never turn into a window drag.
            let press_origin = ctx.input(|input| input.pointer.press_origin());
            let on_control =
                press_origin.is_some_and(|pos| control_rects.iter().any(|rect| rect.contains(pos)));
            if !on_control {
                if bar.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if bar.double_clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }
            }
            match action {
                Some(WindowAction::Close) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Some(WindowAction::Minimize) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
                Some(WindowAction::MaximizeToggle) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }
                None => {}
            }
            let rect = ui.max_rect();
            ui.painter().hline(
                rect.x_range(),
                rect.bottom() - 1.0,
                Stroke::new(BORDER, INK),
            );
        });
}

#[derive(Clone, Copy)]
enum WindowGlyph {
    Minimize,
    Maximize,
    Restore,
    Close,
}

#[derive(Clone, Copy)]
enum WindowAction {
    Minimize,
    MaximizeToggle,
    Close,
}

/// A 30×30 pixel button: square, 2 px ink border, no rounding. Close (and every
/// other control on hover) inverts to a black fill with a white glyph.
fn window_control(ui: &mut egui::Ui, glyph: WindowGlyph, tooltip: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(WINDOW_CONTROL), Sense::click());
    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();
    let hover_t = ui
        .ctx()
        .animate_bool_with_time(response.id.with("hover"), hovered, 0.10);
    let press_t = ui
        .ctx()
        .animate_bool_with_time(response.id.with("press"), pressed, 0.04);
    let fill = PANEL
        .lerp_to_gamma(INK, hover_t)
        .lerp_to_gamma(GREY, press_t);
    let fg = if press_t >= 0.5 || hover_t < 0.5 {
        INK
    } else {
        WHITE
    };
    let face = rect.shrink(1.0 * press_t);
    ui.painter().rect(
        rect,
        CornerRadius::ZERO,
        fill,
        Stroke::new(BORDER, INK),
        StrokeKind::Inside,
    );
    let center = face.center();
    match glyph {
        WindowGlyph::Minimize => {
            ui.painter().rect_filled(
                Rect::from_center_size(center + Vec2::new(0.0, 4.0), Vec2::new(10.0, 2.0)),
                CornerRadius::ZERO,
                fg,
            );
        }
        WindowGlyph::Maximize => {
            ui.painter().rect_stroke(
                Rect::from_center_size(center, Vec2::splat(10.0)),
                CornerRadius::ZERO,
                Stroke::new(2.0_f32, fg),
                StrokeKind::Inside,
            );
        }
        WindowGlyph::Restore => {
            let back = Rect::from_min_size(center + Vec2::new(-3.0, -5.0), Vec2::splat(8.0));
            let front = Rect::from_min_size(center + Vec2::new(-5.0, -3.0), Vec2::splat(8.0));
            ui.painter().rect_stroke(
                back,
                CornerRadius::ZERO,
                Stroke::new(2.0_f32, fg),
                StrokeKind::Inside,
            );
            ui.painter().rect(
                front,
                CornerRadius::ZERO,
                fill,
                Stroke::new(2.0_f32, fg),
                StrokeKind::Inside,
            );
        }
        WindowGlyph::Close => {
            // A 2 px pixel X: five square steps per diagonal, no anti-aliasing.
            for step in -2..=2 {
                let offset = step as f32 * 2.0;
                for (x, y) in [(offset, offset), (offset, -offset)] {
                    ui.painter().rect_filled(
                        Rect::from_min_size(center + Vec2::new(x - 1.0, y - 1.0), Vec2::splat(2.0)),
                        CornerRadius::ZERO,
                        fg,
                    );
                }
            }
        }
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tooltip)
}

/// Borderless windows have no native resize frame. Eight thin foreground strips
/// restore the edges/corners: the cursor changes there and a primary press asks
/// winit to start an OS resize. Skipped while maximized.
fn frameless_resize(ctx: &egui::Context) {
    if ctx
        .input(|input| input.viewport().maximized)
        .unwrap_or(false)
    {
        return;
    }
    let screen = ctx.screen_rect();
    let edge = RESIZE_EDGE;
    let inner = screen.shrink(edge);
    let (l, r, t, b) = (screen.left(), screen.right(), screen.top(), screen.bottom());
    let directions: [(egui::ResizeDirection, Rect, egui::CursorIcon); 8] = [
        (
            egui::ResizeDirection::North,
            Rect::from_min_max(
                egui::pos2(inner.left(), t),
                egui::pos2(inner.right(), t + edge),
            ),
            egui::CursorIcon::ResizeNorth,
        ),
        (
            egui::ResizeDirection::South,
            Rect::from_min_max(
                egui::pos2(inner.left(), b - edge),
                egui::pos2(inner.right(), b),
            ),
            egui::CursorIcon::ResizeSouth,
        ),
        (
            egui::ResizeDirection::West,
            Rect::from_min_max(
                egui::pos2(l, inner.top()),
                egui::pos2(l + edge, inner.bottom()),
            ),
            egui::CursorIcon::ResizeWest,
        ),
        (
            egui::ResizeDirection::East,
            Rect::from_min_max(
                egui::pos2(r - edge, inner.top()),
                egui::pos2(r, inner.bottom()),
            ),
            egui::CursorIcon::ResizeEast,
        ),
        (
            egui::ResizeDirection::NorthWest,
            Rect::from_min_max(egui::pos2(l, t), egui::pos2(l + edge, t + edge)),
            egui::CursorIcon::ResizeNorthWest,
        ),
        (
            egui::ResizeDirection::NorthEast,
            Rect::from_min_max(egui::pos2(r - edge, t), egui::pos2(r, t + edge)),
            egui::CursorIcon::ResizeNorthEast,
        ),
        (
            egui::ResizeDirection::SouthWest,
            Rect::from_min_max(egui::pos2(l, b - edge), egui::pos2(l + edge, b)),
            egui::CursorIcon::ResizeSouthWest,
        ),
        (
            egui::ResizeDirection::SouthEast,
            Rect::from_min_max(egui::pos2(r - edge, b - edge), egui::pos2(r, b)),
            egui::CursorIcon::ResizeSouthEast,
        ),
    ];
    egui::Area::new(egui::Id::new("frameless-resize"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .interactable(true)
        // Avoid the default first-frame 600×400 area background: start at zero
        // so the only foreground widgets are the eight edge strips.
        .default_size(Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_clip_rect(screen);
            // Do NOT allocate the full screen here. An interactive `Area`
            // registers its own background widget over `state.rect()`; a
            // full-screen one sits in a higher layer and egui 0.31's hit test
            // (`hidden` pass in hit_test.rs) then discards every panel, header
            // and button underneath it as "hidden". Only the eight thin edge
            // widgets exist; the centre stays transparent to input.
            for (index, (direction, rect, cursor)) in directions.into_iter().enumerate() {
                let response = ui.interact(
                    rect,
                    egui::Id::new(("frameless-resize-edge", index)),
                    Sense::click_and_drag(),
                );
                if response.is_pointer_button_down_on()
                    && ui.ctx().input(|input| input.pointer.primary_pressed())
                {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
                }
                let _ = response.on_hover_cursor(cursor);
            }
        });
}

fn footer(app: &mut OcrApp, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("footer")
        .exact_height(32.0)
        .frame(Frame::new().fill(INK).inner_margin(0))
        .show(ctx, |ui| {
            Frame::new()
                .inner_margin(Margin::symmetric(16, 0))
                .show(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.label(mono_bold_text("OCR TOOL", 10.0, WHITE));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(mono_text(
                                "CTRL+O ДОБАВИТЬ · CTRL+ENTER ЗАПУСК · ESC ОТМЕНА",
                                9.0,
                                WHITE,
                            ));
                            ui.add_space(12.0);
                            let phase = app.status_phase();
                            let status = status_text(phase, &app.status);
                            let prefix = match phase {
                                StatusPhase::Error | StatusPhase::Notice => "! ",
                                _ => "",
                            };
                            ui.add(
                                Label::new(mono_text(format!("{prefix}{status}"), 9.0, WHITE))
                                    .truncate(),
                            );
                        });
                    });
                });
        });
}

fn left_rail(app: &mut OcrApp, ctx: &egui::Context) {
    egui::SidePanel::left("left-rail")
        .resizable(false)
        .exact_width(RAIL_WIDTH)
        .frame(Frame::new().fill(CREAM).inner_margin(0))
        .show(ctx, |ui| {
            let full = ui.max_rect();
            ui.painter()
                .vline(full.right() - 1.0, full.y_range(), Stroke::new(BORDER, INK));
            egui::ScrollArea::vertical()
                .id_salt("rail-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    Frame::new()
                        .inner_margin(Margin::symmetric(16, 12))
                        .show(ui, |ui| {
                            ui.label(pixel_text("ЭТАПЫ", 9.0, MUTED));
                            ui.add_space(4.0);
                            let active_step = if app.running
                                || (app.setup_complete()
                                    && !app.inputs.is_empty()
                                    && !app.output_dir.trim().is_empty())
                            {
                                3
                            } else if !app.inputs.is_empty() {
                                2
                            } else {
                                1
                            };
                            let mut previous_bottom = None;
                            step_block(
                                ui,
                                &mut previous_bottom,
                                1,
                                "ДОКУМЕНТЫ",
                                "Добавьте PDF или изображения.",
                                active_step == 1,
                                |ui| {
                                    if pixel_button(
                                        ui,
                                        "ДОБАВИТЬ ФАЙЛЫ",
                                        ButtonKind::Secondary,
                                        !app.running,
                                    )
                                    .clicked()
                                    {
                                        app.pick_files();
                                    }
                                    let count = app.inputs.len();
                                    ui.label(mono_text(
                                        if count == 0 {
                                            "список пуст".to_string()
                                        } else {
                                            format!(
                                                "{count} {}",
                                                plural_ru(count, "файл", "файла", "файлов")
                                            )
                                        },
                                        9.0,
                                        if count == 0 { MUTED } else { INK },
                                    ));
                                },
                            );
                            step_block(
                                ui,
                                &mut previous_bottom,
                                2,
                                "СОХРАНЕНИЕ",
                                "Папка, формат, скорость.",
                                active_step == 2,
                                |ui| {
                                    if pixel_button(
                                        ui,
                                        "ПАПКА СОХРАНЕНИЯ",
                                        ButtonKind::Secondary,
                                        !app.running,
                                    )
                                    .clicked()
                                    {
                                        app.pick_output_dir();
                                    }
                                    let folder = if app.output_dir.trim().is_empty() {
                                        "не выбрана".to_string()
                                    } else {
                                        app.output_dir.clone()
                                    };
                                    ui.add(Label::new(mono_text(folder, 9.0, MUTED)).truncate());
                                },
                            );
                            step_block(
                                ui,
                                &mut previous_bottom,
                                3,
                                "РАСПОЗНАТЬ",
                                "Проверьте настройки и запускайте.",
                                active_step == 3,
                                |ui| {
                                    if app.running {
                                        if pixel_button_full(
                                            ui,
                                            "ОТМЕНА",
                                            ButtonKind::Primary,
                                            !app.cancelling,
                                        )
                                        .clicked()
                                        {
                                            app.cancel_job();
                                        }
                                        ui.label(mono_text(
                                            if app.cancelling {
                                                "Останавливаем…"
                                            } else {
                                                "Идёт распознавание."
                                            },
                                            9.0,
                                            MUTED,
                                        ));
                                    } else {
                                        let enabled = app.can_start();
                                        let response = pixel_button_full(
                                            ui,
                                            "РАСПОЗНАТЬ",
                                            ButtonKind::Primary,
                                            enabled,
                                        );
                                        let response = if let Some(blocker) = app.start_blocker() {
                                            response.on_hover_text(blocker)
                                        } else {
                                            response.on_hover_text("Ctrl+Enter")
                                        };
                                        if response.clicked() {
                                            app.start();
                                        }
                                    }
                                },
                            );

                            ui.add_space(4.0);
                            divider(ui);
                            ui.add_space(6.0);

                            if app.setup_complete() {
                                ui.horizontal(|ui| {
                                    badge(ui, "ГОТОВО", true);
                                    ui.label(mono_text("модель и движок готовы", 9.0, MUTED));
                                });
                            } else {
                                ui.horizontal(|ui| {
                                    badge(ui, "ПОДГОТОВКА", false);
                                    ui.label(mono_text("нужна подготовка", 9.0, MUTED));
                                });
                            }
                            if app.preloading {
                                ui.add_space(4.0);
                                badge(ui, "ЗАГРУЖАЕМ МОДЕЛЬ…", false);
                            } else if app
                                .preloaded
                                .as_ref()
                                .map(|server| server.is_alive())
                                .unwrap_or(false)
                            {
                                ui.add_space(4.0);
                                badge(ui, "МОДЕЛЬ В ПАМЯТИ", true);
                            } else if app.preload_error.is_some() {
                                ui.add_space(4.0);
                                badge(ui, "ПРЕДЗАГРУЗКА НЕ УДАЛАСЬ", false);
                            }

                            ui.add_space(10.0);
                            ui.label(pixel_text("СКОРОСТЬ", 8.0, MUTED));
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                let quality = app.image_max_tokens >= DEFAULT_IMAGE_MAX_TOKENS;
                                if segment(ui, "КАЧЕСТВО", quality, !app.running, 82.0).clicked()
                                    && !quality
                                {
                                    app.image_max_tokens = DEFAULT_IMAGE_MAX_TOKENS;
                                    app.save_prefs();
                                    app.maybe_start_preload();
                                }
                                if segment(ui, "БЫСТРО", !quality, !app.running, 82.0).clicked()
                                    && quality
                                {
                                    app.image_max_tokens = FAST_IMAGE_MAX_TOKENS;
                                    app.save_prefs();
                                    app.maybe_start_preload();
                                }
                            });
                            ui.add_space(6.0);
                            let mut preload = app.preload_model;
                            if check_row(
                                ui,
                                &mut preload,
                                "ПРЕДЗАГРУЗКА МОДЕЛИ",
                                "Держать модель в памяти между запусками.",
                            ) {
                                app.preload_model = preload;
                                app.save_prefs();
                                app.maybe_start_preload();
                                if preload {
                                    app.log("preload: enabled", false);
                                }
                            }
                            ui.label(mono_text(
                                "Держать модель в памяти: первый документ быстрее, но занимает \
                                 ~5 ГБ VRAM/RAM. Отключите, если память нужна другим приложениям.",
                                9.0,
                                MUTED,
                            ));
                            ui.add_space(8.0);
                            if pixel_button(
                                ui,
                                "ДОПОЛНИТЕЛЬНО",
                                ButtonKind::Secondary,
                                !app.running,
                            )
                            .clicked()
                            {
                                app.show_advanced = true;
                            }
                            ui.label(mono_text(
                                "Модель, движок, PDFium, DPI, лимиты и журнал.",
                                9.0,
                                MUTED,
                            ));
                        });
                });
        });
}

fn step_block(
    ui: &mut egui::Ui,
    previous_bottom: &mut Option<f32>,
    number: u8,
    title: &str,
    description: &str,
    active: bool,
    add: impl FnOnce(&mut egui::Ui),
) {
    let width = ui.available_width();
    let (row, _) = ui.allocate_exact_size(Vec2::new(width, 26.0), Sense::hover());
    let center = Pos2::new(row.left() + 13.0, row.center().y);
    if let Some(previous) = *previous_bottom {
        ui.painter()
            .vline(center.x, previous..=center.y, Stroke::new(BORDER, INK));
    }
    ui.painter().circle(
        center,
        12.0,
        if active { INK } else { PANEL },
        Stroke::new(BORDER, INK),
    );
    ui.painter().text(
        center,
        Align2::CENTER_CENTER,
        number.to_string(),
        FontId::new(9.0, pixel_family()),
        if active { WHITE } else { INK },
    );
    ui.painter().text(
        Pos2::new(center.x + 22.0, center.y),
        Align2::LEFT_CENTER,
        title,
        FontId::new(9.0, pixel_family()),
        if active { INK } else { MUTED },
    );
    *previous_bottom = Some(center.y + 12.0);

    ui.horizontal(|ui| {
        ui.add_space(36.0);
        ui.vertical(|ui| {
            ui.label(mono_text(description, 9.5, MUTED));
            ui.add_space(2.0);
            add(ui);
        });
    });
    ui.add_space(6.0);
}

// ---------------------------------------------------------------------------
// Center: setup, documents, output
// ---------------------------------------------------------------------------

fn setup_window(app: &mut OcrApp, ui: &mut egui::Ui) {
    pixel_window(ui, "ПОДГОТОВКА", |ui| {
        ui.label(pixel_text("ПЕРВАЯ ПОДГОТОВКА", 10.0, INK));
        ui.label(mono_text(
            "Это разовая загрузка. Документы не отправляются в интернет.",
            10.0,
            MUTED,
        ));
        ui.add_space(8.0);
        divider(ui);
        ui.add_space(10.0);
        setup_model_row(app, ui);
        ui.add_space(10.0);
        divider(ui);
        ui.add_space(10.0);
        setup_runtime_row(app, ui);
    });
}

fn setup_model_row(app: &mut OcrApp, ui: &mut egui::Ui) {
    let ready = app.model_status.is_ready();
    let incomplete = matches!(&app.model_status, ModelStatus::Incomplete { .. });
    let busy = app.downloading || app.runtime_downloading || app.running;

    ui.horizontal(|ui| {
        badge(ui, "МОДЕЛЬ", true);
        ui.label(mono_bold_text("Модель распознавания", 12.0, INK));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ready {
                badge(ui, "ГОТОВО", true);
            } else if incomplete {
                badge(ui, "НЕПОЛНАЯ", false);
            } else {
                badge(ui, "НЕТ", false);
            }
        });
    });
    ui.label(mono_text(
        "~3,4 ГБ · скачивается один раз · локальная модель",
        9.5,
        MUTED,
    ));
    let status = if ready {
        "Файлы модели на месте."
    } else if incomplete {
        "Файлы повреждены или неполные — нажмите «Заменить»."
    } else {
        "Ещё не скачана. Это самая большая часть подготовки."
    };
    ui.label(mono_text(status, 10.0, if ready { INK } else { MUTED }));
    ui.add_space(4.0);

    ui.horizontal_wrapped(|ui| {
        if !ready {
            if mono_button(ui, "СКАЧАТЬ МОДЕЛЬ (~3,4 ГБ)", ButtonKind::Primary, !busy).clicked()
            {
                app.start_download();
            }
        }
        if mono_button(ui, "ПАПКА МОДЕЛИ", ButtonKind::Secondary, !busy).clicked() {
            app.pick_model_dir();
        }
        if (ready || incomplete)
            && mono_button(ui, "ЗАМЕНИТЬ", ButtonKind::Secondary, !busy).clicked()
        {
            app.confirm_replace = true;
        }
    });

    if app.confirm_replace {
        note(
            ui,
            "Замена удалит существующие файлы модели (включая недокачанные) и скачает \
             закреплённую версию заново.",
            true,
        );
        ui.horizontal(|ui| {
            if mono_button(ui, "ДА, ЗАМЕНИТЬ", ButtonKind::Primary, !busy).clicked() {
                app.start_download_force();
            }
            if mono_button(ui, "ОТМЕНА", ButtonKind::Secondary, true).clicked() {
                app.confirm_replace = false;
            }
        });
    }

    if app.downloading {
        progress_bar(ui, app.progress, 16.0);
        ui.label(mono_text(
            if app.download_text.is_empty() {
                "Готовим загрузку…"
            } else {
                app.download_text.as_str()
            },
            9.5,
            MUTED,
        ));
        ui.horizontal(|ui| {
            if mono_button(ui, "ОТМЕНИТЬ ЗАГРУЗКУ", ButtonKind::Secondary, true).clicked()
            {
                app.download_cancel
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                app.status = "Останавливаем загрузку модели…".to_string();
            }
        });
    }
}

fn setup_runtime_row(app: &mut OcrApp, ui: &mut egui::Ui) {
    let ready = app.runtime.is_ready();
    let llama_ready = app.runtime.llama_ready();
    let busy = app.downloading || app.runtime_downloading || app.running;
    let size = runtime_download_mb()
        .map(|mb| format!("~{mb} МБ"))
        .unwrap_or_else(|| "~69 МБ".to_string());

    ui.horizontal(|ui| {
        badge(ui, "ДВИЖОК", true);
        ui.label(mono_bold_text("Движок и просмотр PDF", 12.0, INK));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ready {
                badge(ui, "ГОТОВО", true);
            } else if llama_ready {
                badge(ui, "НЕТ PDFIUM", false);
            } else {
                badge(ui, "НЕТ", false);
            }
        });
    });
    ui.label(mono_text(
        format!("{size} · движок распознавания + просмотр PDF · один раз"),
        9.5,
        MUTED,
    ));
    let status = if ready {
        "Движок и просмотр PDF готовы."
    } else if llama_ready {
        "Движок есть, но не хватает просмотрщика PDF."
    } else {
        "Нужен для распознавания и для PDF."
    };
    ui.label(mono_text(status, 10.0, if ready { INK } else { MUTED }));
    ui.add_space(4.0);

    ui.horizontal_wrapped(|ui| {
        if !ready {
            if mono_button(ui, "СКАЧАТЬ ДВИЖОК", ButtonKind::Primary, !busy).clicked()
            {
                app.start_runtime_download(false);
            }
        }
        if mono_button(ui, "ПАПКА ДВИЖКА", ButtonKind::Secondary, !busy).clicked() {
            app.pick_runtime_dir();
        }
        if (ready || llama_ready)
            && mono_button(ui, "ПЕРЕУСТАНОВИТЬ", ButtonKind::Secondary, !busy).clicked()
        {
            app.confirm_runtime_replace = true;
        }
    });

    if app.confirm_runtime_replace {
        note(
            ui,
            "Переустановка заменит движок и просмотр PDF в папке движка закреплённой \
             версией.",
            true,
        );
        ui.horizontal(|ui| {
            if mono_button(ui, "ДА, ПЕРЕУСТАНОВИТЬ", ButtonKind::Primary, !busy).clicked()
            {
                app.start_runtime_download(true);
            }
            if mono_button(ui, "ОТМЕНА", ButtonKind::Secondary, true).clicked() {
                app.confirm_runtime_replace = false;
            }
        });
    }

    if app.runtime_downloading {
        progress_bar(ui, app.progress, 16.0);
        ui.label(mono_text(
            if app.runtime_text.is_empty() {
                "Готовим загрузку…"
            } else {
                app.runtime_text.as_str()
            },
            9.5,
            MUTED,
        ));
        ui.horizontal(|ui| {
            if mono_button(ui, "ОТМЕНИТЬ ЗАГРУЗКУ", ButtonKind::Secondary, true).clicked()
            {
                app.runtime_cancel
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                app.status = "Останавливаем загрузку движка…".to_string();
            }
        });
    }
}

fn documents_window(app: &mut OcrApp, ui: &mut egui::Ui) {
    pixel_window(ui, "ДОКУМЕНТЫ", |ui| {
        ui.horizontal(|ui| {
            ui.label(mono_text("Добавьте PDF или изображения.", 11.0, INK));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                badge(
                    ui,
                    &format!(
                        "{} {}",
                        app.inputs.len(),
                        plural_ru(app.inputs.len(), "ФАЙЛ", "ФАЙЛА", "ФАЙЛОВ")
                    ),
                    app.inputs.len() > 0,
                );
            });
        });
        ui.add_space(6.0);
        drop_zone(app, ui);
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let can_edit = !app.running;
            ui.spacing_mut().item_spacing.x = 6.0;
            if pixel_button(ui, "ДОБАВИТЬ ФАЙЛЫ", ButtonKind::Primary, can_edit)
                .on_hover_text("Ctrl+O")
                .clicked()
            {
                app.pick_files();
            }
            if pixel_button(ui, "ДОБАВИТЬ ПАПКУ", ButtonKind::Secondary, can_edit).clicked()
            {
                app.pick_folder();
            }
            if !app.inputs.is_empty()
                && pixel_button(ui, "ОЧИСТИТЬ", ButtonKind::Secondary, can_edit).clicked()
            {
                app.inputs.clear();
                app.status = "Список очищен. Добавьте файлы, чтобы начать.".to_string();
            }
        });
        ui.label(mono_text(
            "PDF · PNG · JPEG · TIFF · WEBP · BMP",
            9.0,
            MUTED,
        ));

        if app.inputs.is_empty() {
            ui.add_space(6.0);
            ui.label(mono_text(
                "Пока пусто. Добавьте счёт, договор или скан — текст распознается на этом \
                 компьютере.",
                10.5,
                MUTED,
            ));
            ui.label(mono_text(
                "Можно перетащить файл в любое место окна.",
                9.0,
                MUTED,
            ));
        } else {
            ui.add_space(6.0);
            let mut remove = None;
            egui::ScrollArea::vertical()
                .id_salt("input-files")
                .max_height(200.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for (index, path) in app.inputs.iter().enumerate() {
                        if file_row(app, ui, path, index) {
                            remove = Some(index);
                        }
                    }
                });
            if let Some(index) = remove {
                app.inputs.remove(index);
                if app.inputs.is_empty() {
                    app.status = "Список очищен. Добавьте файлы, чтобы начать.".to_string();
                }
            }
        }
    });
}

fn file_row(app: &OcrApp, ui: &mut egui::Ui, path: &Path, index: usize) -> bool {
    let mut remove = false;
    Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(BORDER, INK))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let label = if path.is_dir() {
                    "ПАПКА"
                } else if is_pdf(path) {
                    "PDF"
                } else {
                    "ФОТО"
                };
                badge(ui, label, true);
                ui.vertical(|ui| {
                    let name = ui.add(
                        Label::new(mono_bold_text(display_name(path), 11.0, INK))
                            .truncate()
                            .sense(Sense::hover()),
                    );
                    name.on_hover_text(path.display().to_string());
                    ui.label(mono_text(input_detail(path), 9.0, MUTED));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if small_pixel_button(ui, "×", !app.running)
                        .on_hover_text("Убрать из списка")
                        .clicked()
                    {
                        remove = true;
                    }
                });
            });
        });
    ui.add_space(4.0);
    let _ = index;
    remove
}

fn drop_zone(app: &mut OcrApp, ui: &mut egui::Ui) {
    let height = 92.0;
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let hovered = response.hovered() && !app.running;
    let hover_t = ui
        .ctx()
        .animate_bool_with_time(response.id.with("hover"), hovered, 0.10);
    let fill = PANEL.lerp_to_gamma(CREAM, hover_t);
    ui.painter().rect(
        rect,
        CornerRadius::ZERO,
        fill,
        Stroke::new(BORDER, INK),
        StrokeKind::Inside,
    );
    // Corner ticks keep the drop area visibly a target without rounded chrome.
    let tick = 10.0 + 4.0 * hover_t;
    for corner in [
        (rect.left_top(), Vec2::new(tick, 0.0), Vec2::new(0.0, tick)),
        (
            rect.right_top(),
            Vec2::new(-tick, 0.0),
            Vec2::new(0.0, tick),
        ),
        (
            rect.left_bottom(),
            Vec2::new(tick, 0.0),
            Vec2::new(0.0, -tick),
        ),
        (
            rect.right_bottom(),
            Vec2::new(-tick, 0.0),
            Vec2::new(0.0, -tick),
        ),
    ] {
        ui.painter()
            .line_segment([corner.0, corner.0 + corner.1], Stroke::new(3.0_f32, INK));
        ui.painter()
            .line_segment([corner.0, corner.0 + corner.2], Stroke::new(3.0_f32, INK));
    }
    ui.painter().text(
        rect.center() - Vec2::new(0.0, 14.0),
        Align2::CENTER_CENTER,
        "ПЕРЕТАЩИТЕ ФАЙЛЫ СЮДА",
        FontId::new(9.0, pixel_family()),
        if app.running { MUTED } else { INK },
    );
    ui.painter().text(
        rect.center() + Vec2::new(0.0, 12.0),
        Align2::CENTER_CENTER,
        if app.running {
            "Дождитесь окончания распознавания"
        } else {
            "или нажмите, чтобы выбрать файлы"
        },
        FontId::new(10.0, FontFamily::Monospace),
        MUTED,
    );
    if response.clicked() && !app.running {
        app.pick_files();
    }
}

fn output_window(app: &mut OcrApp, ui: &mut egui::Ui) {
    pixel_window(ui, "СОХРАНЕНИЕ", |ui| {
        ui.label(pixel_text("ПАПКА", 8.0, MUTED));
        ui.add_space(2.0);
        let folder = if app.output_dir.trim().is_empty() {
            "не выбрана".to_string()
        } else {
            app.output_dir.clone()
        };
        Frame::new()
            .fill(CREAM)
            .stroke(Stroke::new(BORDER, INK))
            .corner_radius(CornerRadius::ZERO)
            .inner_margin(Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let response = ui.add(
                    Label::new(mono_text(folder, 10.0, INK))
                        .truncate()
                        .sense(Sense::hover()),
                );
                response.on_hover_text(app.output_dir.clone());
            });
        if app.output_dir.trim().is_empty() {
            ui.label(mono_text("Выберите папку для результата.", 9.5, INK));
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if mono_button(ui, "ИЗМЕНИТЬ", ButtonKind::Secondary, !app.running).clicked() {
                app.pick_output_dir();
            }
            if mono_button(ui, "ОТКРЫТЬ", ButtonKind::Secondary, true).clicked() {
                app.open_output_folder();
            }
        });

        ui.add_space(8.0);
        ui.label(pixel_text("ФОРМАТ", 8.0, MUTED));
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (value, label) in [
                (OutputFormat::Md, "MD"),
                (OutputFormat::Html, "HTML"),
                (OutputFormat::Json, "JSON"),
                (OutputFormat::Both, "ОБА"),
                (OutputFormat::All, "ВСЁ"),
            ] {
                if segment(ui, label, app.format == value, !app.running, 58.0).clicked()
                    && app.format != value
                {
                    app.format = value;
                    app.save_prefs();
                }
            }
        });
        ui.label(mono_text(format_hint(app.format), 9.5, MUTED));

        ui.add_space(8.0);
        ui.label(pixel_text("СКОРОСТЬ", 8.0, MUTED));
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let quality = app.image_max_tokens >= DEFAULT_IMAGE_MAX_TOKENS;
            if segment(ui, "КАЧЕСТВО", quality, !app.running, 92.0).clicked() && !quality {
                app.image_max_tokens = DEFAULT_IMAGE_MAX_TOKENS;
                app.save_prefs();
                app.maybe_start_preload();
            }
            if segment(ui, "БЫСТРО", !quality, !app.running, 92.0).clicked() && quality {
                app.image_max_tokens = FAST_IMAGE_MAX_TOKENS;
                app.save_prefs();
                app.maybe_start_preload();
            }
        });
        let hint = if app.image_max_tokens < DEFAULT_IMAGE_MAX_TOKENS {
            "Быстро: 2048 токенов изображения, примерно на 15% меньше времени. Строки \
             таблиц (БИК, Сч. №) могут сдвигаться — проверяйте реквизиты."
        } else {
            "Качество: 4096 токенов изображения. Лучше сохраняет таблицы и мелкий текст."
        };
        ui.label(mono_text(hint, 9.5, MUTED));

        ui.add_space(8.0);
        let mut parallel = app.parallel_pages > 1;
        if check_row(
            ui,
            &mut parallel,
            "2 СТРАНИЦЫ ОДНОВРЕМЕННО",
            "llama-server запускается с -np 2 и контекстом 20000. Ускоряет пакет страниц, \
             но не одну страницу; нужно около 5 ГБ VRAM.",
        ) {
            app.parallel_pages = if parallel { 2 } else { 1 };
            app.save_prefs();
            app.maybe_start_preload();
        }
        if parallel && app.is_cpu_mode() {
            note(
                ui,
                "На CPU две страницы одновременно могут не ускорить распознавание.",
                false,
            );
        }

        ui.add_space(8.0);
        let _ = toggle_button(ui, &mut app.show_password, "PDF-ПАРОЛЬ");
        if app.show_password {
            ui.label(mono_text(
                "Только для защищённых PDF. Пароль не сохраняется и не попадает в журнал.",
                9.5,
                MUTED,
            ));
            ui.add(
                egui::TextEdit::singleline(&mut app.password)
                    .password(true)
                    .hint_text("Пароль (необязательно)")
                    .desired_width(260.0),
            );
        }
    });
}

// ---------------------------------------------------------------------------
// Right: result
// ---------------------------------------------------------------------------

fn result_panel(app: &mut OcrApp, ctx: &egui::Context) {
    egui::SidePanel::right("result-panel")
        .resizable(false)
        .exact_width(RESULT_WIDTH)
        .frame(Frame::new().fill(CREAM).inner_margin(0))
        .show(ctx, |ui| {
            let full = ui.max_rect();
            ui.painter()
                .vline(full.left() + 1.0, full.y_range(), Stroke::new(BORDER, INK));
            Frame::new()
                .inner_margin(Margin::symmetric(10, 12))
                .show(ui, |ui| {
                    Frame::new()
                        .fill(PANEL)
                        .stroke(Stroke::new(BORDER, INK))
                        .corner_radius(CornerRadius::ZERO)
                        .inner_margin(0)
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            window_title_bar(ui, "РЕЗУЛЬТАТ", false);
                            egui::ScrollArea::vertical()
                                .id_salt("result-scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    Frame::new().inner_margin(Margin::symmetric(12, 12)).show(
                                        ui,
                                        |ui| {
                                            ui.set_min_width(ui.available_width());
                                            result_body(app, ui);
                                        },
                                    );
                                });
                        });
                });
        });
}

fn result_body(app: &mut OcrApp, ui: &mut egui::Ui) {
    if app.running {
        result_running(app, ui);
    } else if app.last_error.is_some() {
        result_error(app, ui);
    } else if app.last_report.is_some() {
        result_done(app, ui);
    } else {
        result_idle(app, ui);
    }
}

fn result_idle(app: &mut OcrApp, ui: &mut egui::Ui) {
    ui.label(pixel_text("ЗДЕСЬ ПОЯВИТСЯ", 9.0, INK));
    ui.label(pixel_text("РЕЗУЛЬТАТ", 9.0, INK));
    ui.add_space(6.0);
    ui.label(mono_text(
        "Добавьте файлы, выберите папку и нажмите «Распознать».",
        10.5,
        MUTED,
    ));
    ui.add_space(8.0);
    divider(ui);
    ui.add_space(8.0);
    ui.label(mono_text(
        "Готовые Markdown, HTML и JSON появятся в выбранной папке. Их можно открыть сразу.",
        9.5,
        MUTED,
    ));
    ui.add_space(6.0);
    ui.label(mono_text(
        format!(
            "РЕЖИМ: {} · {}",
            if app.image_max_tokens >= DEFAULT_IMAGE_MAX_TOKENS {
                "КАЧЕСТВО"
            } else {
                "БЫСТРО"
            },
            if app.parallel_pages > 1 {
                "2 СТРАНИЦЫ"
            } else {
                "1 СТРАНИЦА"
            }
        ),
        9.0,
        MUTED,
    ));
    if app.preloading {
        ui.add_space(4.0);
        ui.label(mono_text("ЗАГРУЖАЕМ МОДЕЛЬ В ПАМЯТЬ…", 9.0, INK));
    } else if app
        .preloaded
        .as_ref()
        .map(|server| server.is_alive())
        .unwrap_or(false)
    {
        ui.add_space(4.0);
        ui.label(mono_text(
            "МОДЕЛЬ В ПАМЯТИ — первый документ быстрее.",
            9.0,
            INK,
        ));
    } else if app.preload_error.is_some() {
        ui.add_space(4.0);
        ui.label(mono_text(
            "Предзагрузка не удалась. Распознавание запустит движок само.",
            9.0,
            MUTED,
        ));
    }
    if app.downloading || app.runtime_downloading {
        ui.add_space(8.0);
        ui.label(mono_text(
            "Идёт загрузка. Прогресс — в окне подготовки.",
            9.5,
            INK,
        ));
    }
    if !app.logs.is_empty() {
        ui.add_space(10.0);
        details_block(app, ui, 220.0, "result-log-idle");
    }
}

fn result_running(app: &mut OcrApp, ui: &mut egui::Ui) {
    ui.label(pixel_text(
        if app.cancelling {
            "ОСТАНАВЛИВАЕМ"
        } else {
            "РАСПОЗНАЁМ"
        },
        10.0,
        INK,
    ));
    if let Some(file) = &app.current_file {
        ui.add(Label::new(mono_bold_text(file, 10.5, INK)).truncate());
    } else {
        ui.label(mono_text("Готовим документы и движок…", 10.0, MUTED));
    }
    ui.add_space(6.0);
    progress_bar(ui, app.job_progress, 18.0);
    if let Some(line) = progress_line(app) {
        ui.label(mono_text(line, 10.0, INK));
    }
    if app.is_cpu_mode() {
        note(
            ui,
            "Работаем на CPU — это медленнее: ~10–15 минут на страницу.",
            true,
        );
    }
    ui.add_space(4.0);
    if mono_button(ui, "ОТМЕНА", ButtonKind::Primary, !app.cancelling).clicked() {
        app.cancel_job();
    }
    if app.cancelling {
        ui.label(mono_text("Ждём текущую страницу…", 9.5, MUTED));
    }
    ui.label(mono_text(
        "Можно свернуть окно — распознавание продолжится.",
        9.5,
        MUTED,
    ));
    ui.add_space(8.0);
    details_block(app, ui, 260.0, "result-log-running");
}

fn result_done(app: &mut OcrApp, ui: &mut egui::Ui) {
    let Some(report) = app.last_report.clone() else {
        return;
    };
    let partial = report.documents_failed > 0 || report.pages_failed > 0;
    let html = first_html(&report);
    let md = first_md(&report);
    let json = first_json(&report);

    badge(
        ui,
        if report.cancelled {
            "ОСТАНОВЛЕНО"
        } else if partial {
            "ЧАСТИЧНО"
        } else {
            "ГОТОВО"
        },
        true,
    );
    ui.add_space(4.0);
    if report.cancelled {
        ui.label(pixel_text("ОСТАНОВЛЕНО", 10.0, INK));
    } else if partial {
        ui.label(pixel_text("ГОТОВО С ЗАМЕЧАНИЯМИ", 9.0, INK));
    } else {
        ui.label(pixel_text("ГОТОВО", 12.0, INK));
    }
    ui.label(mono_bold_text(
        format!(
            "{} {} · {} {}",
            report.documents_ok,
            plural_ru(report.documents_ok, "файл", "файла", "файлов"),
            report.pages_ok,
            plural_ru(report.pages_ok, "страница", "страницы", "страниц"),
        ),
        11.0,
        INK,
    ));
    ui.add_space(6.0);
    divider(ui);
    ui.add_space(6.0);
    ui.label(pixel_text("ПАПКА", 7.0, MUTED));
    ui.add(
        Label::new(mono_text(report.output_dir.clone(), 9.5, INK))
            .truncate()
            .sense(Sense::hover()),
    )
    .on_hover_text(report.output_dir.clone());

    if partial {
        let mut failures = Vec::new();
        if report.documents_failed > 0 {
            failures.push(format!(
                "{} {}",
                report.documents_failed,
                plural_ru(report.documents_failed, "файл", "файла", "файлов")
            ));
        }
        if report.pages_failed > 0 {
            failures.push(format!(
                "{} {}",
                report.pages_failed,
                plural_ru(report.pages_failed, "страница", "страницы", "страниц")
            ));
        }
        note(
            ui,
            &format!(
                "{} не удалось, остальные сохранены. Причина — в журнале.",
                failures.join(" и ")
            ),
            true,
        );
    }

    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if pixel_button(ui, "ОТКРЫТЬ ПАПКУ", ButtonKind::Primary, true).clicked() {
            app.open_folder_path(std::path::PathBuf::from(&report.output_dir));
        }
        if let Some(html) = html {
            if pixel_button(ui, "ОТКРЫТЬ HTML", ButtonKind::Secondary, true).clicked() {
                app.open_path(&html);
            }
        }
        if let Some(md) = md {
            if pixel_button(ui, "ОТКРЫТЬ MD", ButtonKind::Secondary, true).clicked() {
                app.open_path(&md);
            }
        }
        if let Some(json) = json {
            if pixel_button(ui, "ОТКРЫТЬ JSON", ButtonKind::Secondary, true).clicked() {
                app.open_path(&json);
            }
        }
        if pixel_button(ui, "ЕЩЁ РАЗ", ButtonKind::Secondary, true).clicked() {
            app.reset_for_new_batch();
        }
    });
    ui.add_space(6.0);
    note(
        ui,
        "Проверьте юридические и финансовые документы: печати, рукописный текст и плохие \
         сканы могут распознаваться с ошибками.",
        false,
    );
    if partial && mono_button(ui, "ОТКРЫТЬ ФАЙЛ ЖУРНАЛА", ButtonKind::Secondary, true).clicked()
    {
        app.open_log_file();
    }
    ui.add_space(8.0);
    details_block(app, ui, 260.0, "result-log-done");
}

fn result_error(app: &mut OcrApp, ui: &mut egui::Ui) {
    let Some(message) = app.last_error.clone() else {
        return;
    };
    let kind = app.last_error_kind;
    badge(ui, "ОШИБКА", true);
    ui.add_space(4.0);
    ui.label(pixel_text("НЕ ПОЛУЧИЛОСЬ", 10.0, INK));
    ui.add_space(4.0);
    ui.label(mono_text(message, 10.5, INK));
    ui.label(mono_text(
        "Документы не изменены. Техническая причина — в журнале.",
        9.0,
        MUTED,
    ));
    ui.add_space(6.0);
    divider(ui);
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let busy = app.running || app.downloading || app.runtime_downloading;
        match kind {
            FailureKind::Model => {
                if mono_button(ui, "СКАЧАТЬ МОДЕЛЬ", ButtonKind::Primary, !busy).clicked()
                {
                    app.start_download();
                }
                if mono_button(ui, "ПАПКА МОДЕЛИ", ButtonKind::Secondary, !busy).clicked()
                {
                    app.pick_model_dir();
                }
            }
            FailureKind::Runtime | FailureKind::Pdfium => {
                if mono_button(ui, "СКАЧАТЬ ДВИЖОК", ButtonKind::Primary, !busy).clicked()
                {
                    app.start_runtime_download(false);
                }
                if mono_button(ui, "ПАПКА ДВИЖКА", ButtonKind::Secondary, !busy).clicked()
                {
                    app.pick_runtime_dir();
                }
            }
            FailureKind::File => {
                if mono_button(ui, "ВЫБРАТЬ ФАЙЛ", ButtonKind::Primary, !busy).clicked()
                {
                    app.pick_files();
                }
            }
            FailureKind::Output => {
                if mono_button(ui, "ВЫБРАТЬ ПАПКУ", ButtonKind::Primary, !busy).clicked()
                {
                    app.pick_output_dir();
                }
            }
            FailureKind::Password => {
                if mono_button(ui, "ВВЕСТИ ПАРОЛЬ", ButtonKind::Primary, !busy).clicked()
                {
                    app.show_password = true;
                }
            }
            FailureKind::Pages => {
                if mono_button(ui, "ИСПРАВИТЬ СТРАНИЦЫ", ButtonKind::Primary, !busy).clicked()
                {
                    app.show_advanced = true;
                }
            }
            FailureKind::Timeout | FailureKind::Unknown => {}
        }
        if matches!(
            kind,
            FailureKind::Timeout | FailureKind::Unknown | FailureKind::Password | FailureKind::File
        ) && mono_button(ui, "ПОВТОРИТЬ", ButtonKind::Secondary, !busy).clicked()
        {
            app.start();
        }
        if mono_button(ui, "СКРЫТЬ", ButtonKind::Secondary, true).clicked() {
            app.last_error = None;
            app.status = "Ошибка скрыта. Можно попробовать снова.".to_string();
        }
        if mono_button(ui, "ЖУРНАЛ", ButtonKind::Secondary, true).clicked() {
            app.show_advanced = true;
            app.scroll_log = true;
        }
    });
    ui.add_space(8.0);
    details_block(app, ui, 260.0, "result-log-error");
}

fn details_block(app: &mut OcrApp, ui: &mut egui::Ui, height: f32, id_salt: &str) {
    toggle_button(ui, &mut app.show_details, "ПОДРОБНЕЕ");
    if app.show_details {
        ui.add_space(4.0);
        job_log_header(app, ui);
        job_log(app, ui, height, id_salt);
    }
}

// ---------------------------------------------------------------------------
// Windows: advanced, help
// ---------------------------------------------------------------------------

fn advanced_window(app: &mut OcrApp, ctx: &egui::Context) {
    let max_height = (ctx.screen_rect().height() - 60.0).max(420.0);
    let height = max_height.min(640.0);
    let mut close = false;
    egui::Window::new("advanced-window")
        .id(egui::Id::new("advanced-window"))
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .fixed_size(Vec2::new(660.0, height))
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .frame(window_frame())
        .show(ctx, |ui| {
            ui.set_min_width(ui.available_width());
            if window_title_bar(ui, "ДОПОЛНИТЕЛЬНО", true) {
                close = true;
            }
            egui::ScrollArea::vertical()
                .id_salt("advanced-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    Frame::new()
                        .inner_margin(Margin::symmetric(16, 14))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(mono_text(
                                "Технические пути и параметры. Обычно здесь ничего менять не нужно.",
                                10.0,
                                MUTED,
                            ));
                            ui.add_space(6.0);
                            advanced_grid(app, ui);
                            ui.add_space(8.0);
                            divider(ui);
                            ui.add_space(8.0);
                            let log_header = job_log_header(app, ui);
                            if app.scroll_log {
                                app.scroll_log = false;
                                log_header.scroll_to_me(Some(Align::TOP));
                            }
                            job_log(app, ui, 240.0, "advanced-log");
                        });
                });
        });
    if close {
        app.show_advanced = false;
    }
}

fn advanced_grid(app: &mut OcrApp, ui: &mut egui::Ui) {
    let mut changed = false;
    egui::Grid::new("advanced-grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(mono_text("ПАПКА МОДЕЛИ", 10.0, MUTED));
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut app.model_dir)
                            .hint_text("авто: OCR_MODEL_DIR или app-data")
                            .desired_width(340.0),
                    )
                    .changed()
                {
                    changed = true;
                }
                if small_pixel_button(ui, "…", true).clicked() {
                    app.pick_model_dir();
                }
            });
            ui.end_row();

            ui.label(mono_text("ПАПКА ДВИЖКА", 10.0, MUTED));
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut app.runtime_dir)
                            .hint_text("авто: app-data/runtime")
                            .desired_width(340.0),
                    )
                    .changed()
                {
                    app.apply_runtime_dir_text();
                }
                if small_pixel_button(ui, "…", true).clicked() {
                    app.pick_runtime_dir();
                }
            });
            ui.end_row();

            ui.label(mono_text("llama-server", 10.0, MUTED));
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut app.llama_dir)
                            .hint_text("авто: OCR_LLAMA_SERVER или runtime/")
                            .desired_width(340.0),
                    )
                    .changed()
                {
                    changed = true;
                }
                if small_pixel_button(ui, "…", true).clicked() {
                    app.pick_llama_dir();
                }
            });
            ui.end_row();

            ui.label(mono_text("PDFium", 10.0, MUTED));
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut app.pdfium)
                            .hint_text("авто: OCR_PDFIUM, рядом с exe, runtime/")
                            .desired_width(340.0),
                    )
                    .changed()
                {
                    changed = true;
                }
                if small_pixel_button(ui, "…", true).clicked() {
                    app.pick_pdfium();
                }
            });
            ui.end_row();

            ui.label(mono_text("Устройство", 10.0, MUTED));
            egui::ComboBox::from_id_salt("device")
                .selected_text(mono_text(device_label(app.device), 10.0, INK))
                .show_ui(ui, |ui| {
                    for device in [Device::Auto, Device::Gpu, Device::Cpu] {
                        if ui
                            .selectable_value(
                                &mut app.device,
                                device,
                                mono_text(device_label(device), 10.0, INK),
                            )
                            .changed()
                        {
                            app.save_prefs();
                        }
                    }
                });
            app.maybe_start_preload();
            ui.end_row();

            ui.label(mono_text("Блоки", 10.0, MUTED));
            egui::ComboBox::from_id_salt("extract-blocks")
                .selected_text(mono_text(
                    extract_blocks_label(app.extract_blocks),
                    10.0,
                    INK,
                ))
                .show_ui(ui, |ui| {
                    for (value, label) in [
                        (BlockExtraction::None, "Выкл"),
                        (BlockExtraction::Images, "Только рисунки"),
                        (BlockExtraction::All, "Все блоки"),
                    ] {
                        if ui
                            .selectable_value(
                                &mut app.extract_blocks,
                                value,
                                mono_text(label, 10.0, INK),
                            )
                            .changed()
                        {
                            app.save_prefs();
                        }
                    }
                });
            ui.end_row();

            ui.label(mono_text("DPI", 10.0, MUTED));
            if ui
                .add(
                    egui::DragValue::new(&mut app.dpi)
                        .range(72..=600)
                        .speed(8.0),
                )
                .changed()
            {
                app.save_prefs();
            }
            ui.end_row();

            ui.label(mono_text("Страницы", 10.0, MUTED));
            if ui
                .add(
                    egui::TextEdit::singleline(&mut app.pages)
                        .hint_text("все, например 1-5,7")
                        .desired_width(220.0),
                )
                .changed()
            {
                app.save_prefs();
            }
            ui.end_row();

            ui.label(mono_text("МАКС. ПИКСЕЛЕЙ", 10.0, MUTED));
            if ui
                .add(
                    egui::DragValue::new(&mut app.max_pixels)
                        .range(1_000_000..=400_000_000)
                        .speed(1_000_000.0),
                )
                .changed()
            {
                app.save_prefs();
            }
            ui.end_row();

            ui.label(mono_text("МАКС. ТОКЕНОВ", 10.0, MUTED));
            if ui
                .add(
                    egui::DragValue::new(&mut app.max_tokens)
                        .range(128..=32_768)
                        .speed(64.0),
                )
                .changed()
            {
                app.save_prefs();
            }
            ui.end_row();

            ui.label(mono_text("ТАЙМАУТ, С", 10.0, MUTED));
            if ui
                .add(
                    egui::DragValue::new(&mut app.timeout_secs)
                        .range(30..=7200)
                        .speed(10.0),
                )
                .changed()
            {
                app.save_prefs();
            }
            ui.end_row();

            ui.label(mono_text("Подпапки", 10.0, MUTED));
            let mut recursive = app.recursive;
            if check_row(ui, &mut recursive, "Обходить вложенные папки", "") {
                app.recursive = recursive;
                app.save_prefs();
            }
            ui.end_row();

            ui.label(mono_text("Пути", 10.0, MUTED));
            ui.vertical(|ui| {
                ui.label(mono_text(
                    format!("Модель: {}", app.model_status.dir().display()),
                    9.0,
                    MUTED,
                ));
                ui.label(mono_text(
                    format!(
                        "PDFium: {}",
                        app.runtime
                            .pdfium
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|error| format!("не найден ({error})"))
                    ),
                    9.0,
                    MUTED,
                ));
                ui.label(mono_text(
                    format!(
                        "llama-server: {} вариант(ов), просмотрено {} мест",
                        app.runtime.llama.len(),
                        app.runtime.llama_searched.len()
                    ),
                    9.0,
                    MUTED,
                ));
                if let Some(warning) = app.runtime.llama_warnings.first() {
                    ui.label(mono_text(warning, 9.0, INK));
                }
            });
            ui.end_row();
        });

    if app.extract_blocks == BlockExtraction::All {
        note(
            ui,
            "Все блоки создадут много файлов: PNG, Markdown/HTML/CSV/TeX/TXT для каждого блока.",
            false,
        );
    }
    if changed {
        app.refresh_setup();
        app.save_prefs();
        app.maybe_start_preload();
    }
}

fn help_window(app: &mut OcrApp, ctx: &egui::Context) {
    let mut close = false;
    egui::Window::new("help-window")
        .id(egui::Id::new("help-window"))
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .default_width(520.0)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .frame(window_frame())
        .show(ctx, |ui| {
            ui.set_min_width(ui.available_width());
            if window_title_bar(ui, "СПРАВКА", true) {
                close = true;
            }
            Frame::new()
                .inner_margin(Margin::symmetric(16, 14))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(pixel_text("ТРИ ШАГА", 10.0, INK));
                    ui.add_space(6.0);
                    for (number, text) in [
                        (
                            "1",
                            "Добавьте PDF или изображение — можно перетащить в окно.",
                        ),
                        ("2", "Выберите папку для результата и нажмите «Распознать»."),
                        (
                            "3",
                            "Готовые файлы появятся в этой папке. Их можно открыть сразу.",
                        ),
                    ] {
                        ui.horizontal(|ui| {
                            badge(ui, number, true);
                            ui.label(mono_text(text, 10.5, INK));
                        });
                    }
                    ui.add_space(8.0);
                    divider(ui);
                    ui.add_space(8.0);
                    ui.label(mono_text(
                        "Модель (~3,4 ГБ) и движок (~69 МБ) скачиваются один раз. Документы \
                         остаются на этом компьютере; сеть нужна только для загрузки.",
                        10.0,
                        MUTED,
                    ));
                    ui.add_space(4.0);
                    ui.label(mono_text(
                        "На процессоре (CPU) распознавание медленнее — примерно 10–15 минут на \
                         страницу. На видеокарте заметно быстрее.",
                        10.0,
                        MUTED,
                    ));
                    ui.add_space(10.0);
                    if pixel_button_full(ui, "ПОНЯТНО", ButtonKind::Primary, true).clicked()
                    {
                        close = true;
                    }
                });
        });
    if close {
        app.show_help = false;
    }
}

// ---------------------------------------------------------------------------
// Small shared blocks
// ---------------------------------------------------------------------------

fn job_log_header(app: &mut OcrApp, ui: &mut egui::Ui) -> egui::Response {
    ui.horizontal_wrapped(|ui| {
        ui.label(pixel_text("ЖУРНАЛ", 8.0, INK));
        ui.spacing_mut().item_spacing.x = 4.0;
        if mono_button(ui, "КОПИРОВАТЬ", ButtonKind::Secondary, true).clicked() {
            let text = app
                .logs
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            ui.ctx().copy_text(text);
        }
        if mono_button(ui, "ОЧИСТИТЬ", ButtonKind::Secondary, true).clicked() {
            app.logs.clear();
        }
        if mono_button(ui, "ФАЙЛ", ButtonKind::Secondary, true).clicked() {
            app.open_log_file();
        }
    })
    .response
}

fn job_log(app: &mut OcrApp, ui: &mut egui::Ui, height: f32, id_salt: &str) {
    Frame::new()
        .fill(CREAM)
        .stroke(Stroke::new(BORDER, INK))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::same(8))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(id_salt)
                .max_height(height)
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if app.logs.is_empty() {
                        ui.label(mono_text("События появятся здесь.", 9.5, MUTED));
                    }
                    for line in &app.logs {
                        let color = if line.error { INK } else { MUTED };
                        ui.label(mono_text(&line.text, 9.5, color));
                    }
                });
        });
}

fn device_label(device: Device) -> &'static str {
    match device {
        Device::Auto => "Авто (GPU, иначе CPU)",
        Device::Gpu => "GPU (видеокарта)",
        Device::Cpu => "CPU (медленнее)",
    }
}

fn extract_blocks_label(mode: BlockExtraction) -> &'static str {
    match mode {
        BlockExtraction::None => "Выкл",
        BlockExtraction::Images => "Только рисунки",
        BlockExtraction::All => "Все блоки",
    }
}

fn format_hint(format: OutputFormat) -> &'static str {
    match format {
        OutputFormat::Md => "Markdown — для текста, поиска и правок.",
        OutputFormat::Html => "HTML — для чтения в браузере с форматированием.",
        OutputFormat::Json => {
            "JSON — блоки с bbox и текстом, как в Chandra chunks; для автоматизации."
        }
        OutputFormat::Both => "Markdown — для текста, HTML — для просмотра. Сохраняются оба.",
        OutputFormat::All => "Markdown, HTML и JSON — текст, просмотр в браузере и данные с bbox.",
    }
}

fn progress_line(app: &OcrApp) -> Option<String> {
    let mut parts = Vec::new();
    if app.page_total > 0 {
        if app.page_selected_total > 0 && app.page_selected_total != app.page_total {
            parts.push(format!(
                "Страница {} из {} (выбрано {} из {})",
                app.page, app.page_total, app.page_selected_index, app.page_selected_total
            ));
        } else {
            parts.push(format!("Страница {} из {}", app.page, app.page_total));
        }
    } else if app.current_file.is_some() {
        parts.push("Готовим страницы…".to_string());
    }
    if app.job_parallel_pages > 1 && app.page_selected_total > 1 {
        parts.push("2 страницы одновременно".to_string());
    }
    if let Some(eta) = page_eta(app) {
        parts.push(eta);
    }
    if app.job_files_total > 1 {
        parts.push(format!(
            "файл {} из {}",
            app.job_file_index, app.job_files_total
        ));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" • "))
    }
}

/// Rough planning numbers before the first page completes. Auto is treated as
/// CPU on purpose: a cautious estimate is better than promising GPU speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EtaMode {
    Cpu,
    Gpu,
}

impl EtaMode {
    fn startup_seconds(self) -> f64 {
        match self {
            EtaMode::Cpu => 20.0,
            EtaMode::Gpu => 10.0,
        }
    }

    fn seconds_per_page(self, fast: bool) -> f64 {
        match (self, fast) {
            (EtaMode::Cpu, false) => 12.0 * 60.0,
            // The fast profile cuts prefill; on CPU decode still dominates.
            (EtaMode::Cpu, true) => 10.0 * 60.0,
            (EtaMode::Gpu, false) => 30.0,
            (EtaMode::Gpu, true) => 25.0,
        }
    }
}

fn eta_mode(device: Device, runtime_cpu_only: bool) -> EtaMode {
    match device {
        Device::Gpu => EtaMode::Gpu,
        Device::Cpu => EtaMode::Cpu,
        // Auto follows what the runtime can actually offer. `is_cpu_mode()`
        // is true only when discovery found no GPU candidate.
        Device::Auto if runtime_cpu_only => EtaMode::Cpu,
        Device::Auto => EtaMode::Gpu,
    }
}

fn initial_page_eta(
    mode: EtaMode,
    selected_pages: u32,
    fast: bool,
    parallel_pages: u32,
) -> Option<f64> {
    if selected_pages == 0 {
        return None;
    }
    // Parallel pages shorten the number of waves, not the latency of one page.
    let parallel = parallel_pages.clamp(1, 2) as f64;
    let waves = (selected_pages as f64 / parallel).ceil();
    Some(mode.startup_seconds() + mode.seconds_per_page(fast) * waves)
}

fn refined_page_eta(
    selected_total: u32,
    selected_index: u32,
    pages_done: u32,
    page_seconds_total: f64,
    parallel_pages: u32,
) -> Option<f64> {
    if selected_total == 0 || pages_done == 0 {
        return None;
    }
    let average = page_seconds_total / pages_done as f64;
    if !average.is_finite() || average <= 0.0 {
        return None;
    }
    let remaining = selected_total.saturating_sub(selected_index);
    if remaining == 0 {
        return None;
    }
    let parallel = parallel_pages.clamp(1, 2) as f64;
    let seconds = average * remaining as f64 / parallel;
    if !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    Some(seconds)
}

fn estimate_page_eta(
    mode: EtaMode,
    selected_total: u32,
    selected_index: u32,
    pages_done: u32,
    page_seconds_total: f64,
    fast: bool,
    parallel_pages: u32,
) -> Option<f64> {
    if selected_total == 0 {
        return None;
    }
    if pages_done == 0 {
        initial_page_eta(mode, selected_total, fast, parallel_pages)
    } else {
        refined_page_eta(
            selected_total,
            selected_index,
            pages_done,
            page_seconds_total,
            parallel_pages,
        )
    }
}

fn eta_label(seconds: f64, multi_file: bool) -> String {
    let eta = format_seconds(seconds);
    if multi_file {
        format!("по текущему файлу осталось примерно ~{eta}")
    } else {
        format!("осталось примерно ~{eta}")
    }
}

fn page_eta(app: &OcrApp) -> Option<String> {
    let fast = app.job_image_max_tokens < DEFAULT_IMAGE_MAX_TOKENS;
    let seconds = estimate_page_eta(
        eta_mode(app.device, app.is_cpu_mode()),
        app.page_selected_total,
        app.page_selected_index,
        app.pages_done,
        app.page_seconds_total,
        fast,
        app.job_parallel_pages,
    )?;
    Some(eta_label(seconds, app.job_files_total > 1))
}

pub(crate) fn format_seconds(seconds: f64) -> String {
    if seconds < 60.0 {
        format!("{} с", seconds.ceil().max(1.0) as u64)
    } else if seconds < 3600.0 {
        format!("{} мин", (seconds / 60.0).ceil() as u64)
    } else {
        let hours = (seconds / 3600.0).floor() as u64;
        let minutes = ((seconds - hours as f64 * 3600.0) / 60.0).ceil() as u64;
        format!("{hours} ч {minutes} мин")
    }
}

/// Russian decimal units, friendly in the download card (`1,2 из 3,4 ГБ`).
pub(crate) fn ru_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["Б", "КБ", "МБ", "ГБ", "ТБ"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        return format!("{bytes} Б");
    }
    let mut number = format!("{value:.1}").replace('.', ",");
    if number.ends_with(",0") {
        number.truncate(number.len() - 2);
    }
    format!("{number} {}", UNITS[unit])
}

pub(crate) fn plural_ru(
    count: usize,
    one: &'static str,
    few: &'static str,
    many: &'static str,
) -> &'static str {
    let last_two = count % 100;
    if (11..=14).contains(&last_two) {
        return many;
    }
    match count % 10 {
        1 => one,
        2..=4 => few,
        _ => many,
    }
}

fn is_pdf(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn input_detail(path: &Path) -> String {
    if path.is_dir() {
        return "папка · все поддерживаемые файлы".to_string();
    }
    match std::fs::metadata(path) {
        Ok(metadata) => ru_bytes(metadata.len()),
        Err(_) => "файл недоступен".to_string(),
    }
}

fn first_html(report: &ocr_app::JobReport) -> Option<String> {
    report
        .outputs
        .iter()
        .find(|path| path.to_ascii_lowercase().ends_with(".html"))
        .cloned()
}

fn first_json(report: &ocr_app::JobReport) -> Option<String> {
    report
        .outputs
        .iter()
        .find(|path| path.to_ascii_lowercase().ends_with(".json"))
        .cloned()
}

fn first_md(report: &ocr_app::JobReport) -> Option<String> {
    report
        .outputs
        .iter()
        .find(|path| path.to_ascii_lowercase().ends_with(".md"))
        .cloned()
}

/// ETA helper used by the download cards. `started` is reset per file so the
/// estimate is not polluted by a previous model shard.
pub(crate) fn download_eta(done: u64, total: u64, started: Option<Instant>) -> Option<Duration> {
    let started = started?;
    if done == 0 || total == 0 || done >= total {
        return None;
    }
    let elapsed = started.elapsed().as_secs_f64();
    if elapsed <= 0.0 {
        return None;
    }
    let remaining = elapsed * (total - done) as f64 / done as f64;
    if !remaining.is_finite() || remaining <= 0.0 {
        return None;
    }
    Some(Duration::from_secs_f64(remaining))
}

pub(crate) fn progress_text(done: u64, total: u64, started: Option<Instant>) -> String {
    let mut text = if total > 0 {
        format!("Скачано {} из {}", ru_bytes(done), ru_bytes(total))
    } else {
        format!("Скачано {}", ru_bytes(done))
    };
    if let Some(eta) = download_eta(done, total, started) {
        text.push_str(&format!(
            " • осталось ~{}",
            format_seconds(eta.as_secs_f64())
        ));
    }
    text
}

/// Opening helpers keep every filesystem/dialog call out of the drawing code.
impl OcrApp {
    pub(crate) fn pick_files(&mut self) {
        if let Some(files) = rfd::FileDialog::new()
            .add_filter(
                "Документы и изображения",
                &["pdf", "png", "jpg", "jpeg", "tif", "tiff", "webp", "bmp"],
            )
            .pick_files()
        {
            self.add_paths(files);
            self.status = format!(
                "Добавлено файлов: {}. Выберите папку и нажмите «Распознать».",
                self.inputs.len()
            );
        }
    }

    pub(crate) fn pick_folder(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            self.add_paths([folder]);
            self.recursive = true;
            self.save_prefs();
            self.status = format!("Добавлена папка. Файлов в списке: {}.", self.inputs.len());
        }
    }

    pub(crate) fn pick_output_dir(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            self.output_dir = folder.display().to_string();
            self.save_prefs();
            if self.last_error_kind == FailureKind::Output {
                self.last_error = None;
                self.last_error_kind = FailureKind::Unknown;
            }
            self.status = format!("Результаты будут сохранены в {}", self.output_dir);
        }
    }

    pub(crate) fn pick_model_dir(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            self.model_dir = folder.display().to_string();
            self.refresh_setup();
            self.save_prefs();
            self.clear_setup_error_if_fixed();
            if self.model_status.is_ready() {
                self.download_notice = false;
                self.status = "Модель найдена. Можно продолжать.".to_string();
            } else {
                self.status =
                    "В этой папке не хватает файлов модели. Выберите другую или скачайте модель."
                        .to_string();
            }
        }
    }

    pub(crate) fn pick_runtime_dir(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            self.apply_runtime_dir(folder);
        }
    }

    pub(crate) fn pick_llama_dir(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            self.llama_dir = folder.display().to_string();
            self.refresh_setup();
            self.save_prefs();
            self.clear_setup_error_if_fixed();
            if self.runtime.is_ready() {
                self.download_notice = false;
            }
        }
    }

    pub(crate) fn pick_pdfium(&mut self) {
        if let Some(file) = rfd::FileDialog::new()
            .add_filter("PDFium", &["dll", "so", "dylib"])
            .pick_file()
        {
            self.pdfium = file.display().to_string();
            self.refresh_setup();
            self.save_prefs();
            self.clear_setup_error_if_fixed();
            if self.runtime.is_ready() {
                self.download_notice = false;
            }
        }
    }

    pub(crate) fn cancel_job(&mut self) {
        if self.running && !self.cancelling {
            self.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
            self.cancelling = true;
            self.status = "Останавливаем… Уже готовые страницы сохранятся.".to_string();
        }
    }

    pub(crate) fn open_output_folder(&mut self) {
        let path = std::path::PathBuf::from(self.output_dir.trim());
        self.open_folder_path(path);
    }

    pub(crate) fn open_folder_path(&mut self, path: std::path::PathBuf) {
        if path.as_os_str().is_empty() {
            self.status = "Сначала выберите папку для результата.".to_string();
            return;
        }
        if let Err(error) = std::fs::create_dir_all(&path) {
            self.log(
                format!("cannot create output folder {}: {error}", path.display()),
                true,
            );
            self.status = "Не могу открыть эту папку. Выберите другую.".to_string();
            return;
        }
        match open::that(&path) {
            Ok(()) => {}
            Err(error) => {
                self.log(
                    format!("cannot open folder {}: {error}", path.display()),
                    true,
                );
                self.status = "Не удалось открыть папку. Проверьте права доступа.".to_string();
            }
        }
    }

    pub(crate) fn open_path(&mut self, path: &str) {
        if let Err(error) = open::that(path) {
            self.log(format!("cannot open {path}: {error}"), true);
            self.status = "Не удалось открыть файл. Он мог быть перемещён.".to_string();
        }
    }

    pub(crate) fn open_log_file(&mut self) {
        let path = gui_log::log_path();
        if let Err(error) = open::that(path) {
            self.log(
                format!("cannot open log file {}: {error}", path.display()),
                true,
            );
            self.status = "Не удалось открыть файл журнала.".to_string();
        }
    }

    pub(crate) fn reset_for_new_batch(&mut self) {
        self.inputs.clear();
        self.password.clear();
        self.last_report = None;
        self.last_error = None;
        self.last_error_kind = FailureKind::Unknown;
        self.download_notice = false;
        self.progress = 0.0;
        self.job_progress = 0.0;
        self.current_file = None;
        self.page = 0;
        self.page_total = 0;
        self.page_selected_index = 0;
        self.page_selected_total = 0;
        self.pages_done = 0;
        self.page_seconds_total = 0.0;
        self.job_image_max_tokens = self.image_max_tokens;
        self.job_parallel_pages = self.parallel_pages.clamp(1, 2);
        self.status = "Добавьте файлы — начнём с шага 1.".to_string();
    }

    pub(crate) fn save_prefs(&mut self) {
        prefs::save(&prefs::GuiPrefs {
            output_dir: Some(self.output_dir.clone()),
            format: Some(self.format.as_str().to_string()),
            device: Some(self.device.as_str().to_string()),
            dpi: Some(self.dpi),
            extract_blocks: Some(self.extract_blocks.as_str().to_string()),
            extract_images: Some(self.extract_blocks == BlockExtraction::Images),
            fast: Some(self.image_max_tokens < DEFAULT_IMAGE_MAX_TOKENS),
            image_max_tokens: Some(self.image_max_tokens),
            parallel_pages: Some(self.parallel_pages),
            preload_model: self.preload_model,
            model_dir: self.model_dir.clone(),
            runtime_dir: self.runtime_dir.clone(),
            llama_dir: self.llama_dir.clone(),
            pdfium: self.pdfium.clone(),
            pages: self.pages.clone(),
            recursive: self.recursive,
            max_pixels: self.max_pixels,
            max_tokens: self.max_tokens,
            timeout_secs: self.timeout_secs,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn russian_plurals_cover_the_awkward_cases() {
        assert_eq!(plural_ru(1, "файл", "файла", "файлов"), "файл");
        assert_eq!(plural_ru(2, "файл", "файла", "файлов"), "файла");
        assert_eq!(plural_ru(4, "файл", "файла", "файлов"), "файла");
        assert_eq!(plural_ru(5, "файл", "файла", "файлов"), "файлов");
        assert_eq!(plural_ru(11, "файл", "файла", "файлов"), "файлов");
        assert_eq!(plural_ru(21, "страница", "страницы", "страниц"), "страница");
        assert_eq!(
            plural_ru(102, "страница", "страницы", "страниц"),
            "страницы"
        );
    }

    #[test]
    fn byte_sizes_are_russian_and_decimal() {
        assert_eq!(ru_bytes(3_433_279_808), "3,4 ГБ");
        assert_eq!(ru_bytes(69_000_000), "69 МБ");
        assert_eq!(ru_bytes(1_200_000), "1,2 МБ");
        assert_eq!(ru_bytes(512), "512 Б");
    }

    #[test]
    fn eta_formats_seconds_minutes_and_hours() {
        assert_eq!(format_seconds(12.4), "13 с");
        assert_eq!(format_seconds(59.1), "60 с");
        assert_eq!(format_seconds(61.0), "2 мин");
        assert_eq!(format_seconds(3700.0), "1 ч 2 мин");
        assert_eq!(format_seconds(3660.0), "1 ч 1 мин");
        assert_eq!(format_seconds(7200.0), "2 ч 0 мин");
    }

    #[test]
    fn eta_heuristic_covers_cpu_gpu_auto_and_zero_pages() {
        assert_eq!(eta_mode(Device::Cpu, false), EtaMode::Cpu);
        assert_eq!(eta_mode(Device::Cpu, true), EtaMode::Cpu);
        assert_eq!(eta_mode(Device::Gpu, true), EtaMode::Gpu);
        assert_eq!(eta_mode(Device::Gpu, false), EtaMode::Gpu);
        // Auto on a machine with a GPU candidate uses the GPU estimate;
        // Auto without one falls back to the cautious CPU estimate.
        assert_eq!(eta_mode(Device::Auto, false), EtaMode::Gpu);
        assert_eq!(eta_mode(Device::Auto, true), EtaMode::Cpu);

        // 20 s startup + 12 min for one page; 10 s + 30 s for GPU.
        assert_eq!(initial_page_eta(EtaMode::Cpu, 1, false, 1), Some(740.0));
        assert_eq!(initial_page_eta(EtaMode::Gpu, 1, false, 1), Some(40.0));
        assert_eq!(initial_page_eta(EtaMode::Cpu, 0, false, 1), None);

        // Fast CPU is ~10 min/page, fast GPU ~25 s/page.
        assert_eq!(initial_page_eta(EtaMode::Cpu, 1, true, 1), Some(620.0));
        assert_eq!(initial_page_eta(EtaMode::Gpu, 1, true, 1), Some(35.0));

        // Two pages in parallel are two waves, not three sequential latencies.
        assert_eq!(initial_page_eta(EtaMode::Gpu, 3, false, 2), Some(70.0));

        // Three selected CPU pages: 20 + 3 * 720 = 2180 s.
        let cpu = estimate_page_eta(eta_mode(Device::Cpu, true), 3, 0, 0, 0.0, false, 1).unwrap();
        assert_eq!(cpu, 2180.0);
        assert_eq!(format_seconds(cpu), "37 мин");

        // Auto + GPU: 10 + 3 * 30 = 100 s ≈ 2 min, not 12 min/page.
        let auto_gpu =
            estimate_page_eta(eta_mode(Device::Auto, false), 3, 0, 0, 0.0, false, 1).unwrap();
        assert_eq!(auto_gpu, 100.0);
        assert_eq!(format_seconds(auto_gpu), "2 мин");

        let gpu = estimate_page_eta(eta_mode(Device::Gpu, false), 3, 0, 0, 0.0, false, 1).unwrap();
        assert_eq!(gpu, 100.0);
        assert_eq!(format_seconds(gpu), "2 мин");

        assert_eq!(
            estimate_page_eta(EtaMode::Cpu, 0, 0, 0, 0.0, false, 1),
            None
        );
    }

    #[test]
    fn eta_refines_from_the_actual_average_after_the_first_page() {
        // One page finished in 16 s, four selected pages remain.
        let first = estimate_page_eta(EtaMode::Gpu, 5, 1, 1, 16.0, false, 1).unwrap();
        assert_eq!(first, 64.0);

        // Three pages finished, 45 s total, two pages remain.
        let later = estimate_page_eta(EtaMode::Gpu, 5, 3, 3, 45.0, false, 1).unwrap();
        assert_eq!(later, 30.0);
        assert_eq!(format_seconds(later), "30 с");

        // With two pages in flight the same work is one wave, not two.
        // Two pages took 13 s each (26 s total), three pages still start.
        let parallel = estimate_page_eta(EtaMode::Gpu, 5, 2, 2, 26.0, false, 2).unwrap();
        assert_eq!(parallel, 19.5);

        // Fast changes only the pre-first-page planning number.
        assert_eq!(refined_page_eta(5, 3, 3, 45.0, 1), Some(30.0));

        // No remaining pages, no ETA.
        assert_eq!(
            estimate_page_eta(EtaMode::Cpu, 5, 5, 5, 75.0, false, 1),
            None
        );
        assert_eq!(refined_page_eta(0, 0, 0, 0.0, 1), None);
        assert_eq!(refined_page_eta(5, 1, 0, 0.0, 1), None);
    }

    #[test]
    fn eta_wording_marks_the_current_file_for_multi_file_jobs() {
        assert_eq!(eta_label(90.0, false), "осталось примерно ~2 мин");
        assert_eq!(
            eta_label(90.0, true),
            "по текущему файлу осталось примерно ~2 мин"
        );
        assert_eq!(eta_label(45.0, false), "осталось примерно ~45 с");
        assert_eq!(
            eta_label(3700.0, true),
            "по текущему файлу осталось примерно ~1 ч 2 мин"
        );
    }

    #[test]
    fn download_eta_needs_progress_and_a_clock() {
        assert_eq!(download_eta(0, 100, Some(Instant::now())), None);
        assert_eq!(download_eta(10, 0, Some(Instant::now())), None);
        assert_eq!(download_eta(10, 100, None), None);
    }

    fn report_with_outputs(outputs: &[&str]) -> ocr_app::JobReport {
        ocr_app::JobReport {
            output_dir: "/out".to_string(),
            documents_ok: 1,
            documents_failed: 0,
            pages_ok: 1,
            pages_failed: 0,
            cancelled: false,
            elapsed_ms: 0,
            device: "cpu".to_string(),
            model: "local".to_string(),
            llama_server: "llama-server".to_string(),
            outputs: outputs.iter().map(|path| path.to_string()).collect(),
            documents: Vec::new(),
        }
    }

    #[test]
    fn first_md_finds_the_first_markdown_output_case_insensitively() {
        let report = report_with_outputs(&["/out/a.html", "/out/a.MD", "/out/b.md"]);
        assert_eq!(first_md(&report).as_deref(), Some("/out/a.MD"));

        let report = report_with_outputs(&["/out/a.html", "/out/a.json"]);
        assert_eq!(first_md(&report), None);
    }

    #[test]
    fn format_hints_stay_human() {
        assert!(format_hint(OutputFormat::Both).contains("Markdown"));
        assert!(format_hint(OutputFormat::Html).contains("браузере"));
        assert!(!format_hint(OutputFormat::Md).contains("HTML"));
        assert!(format_hint(OutputFormat::Json).contains("JSON"));
        assert!(format_hint(OutputFormat::All).contains("HTML"));
    }

    #[test]
    fn extraction_modes_have_human_labels() {
        assert_eq!(extract_blocks_label(BlockExtraction::None), "Выкл");
        assert_eq!(
            extract_blocks_label(BlockExtraction::Images),
            "Только рисунки"
        );
        assert_eq!(extract_blocks_label(BlockExtraction::All), "Все блоки");
    }

    // ------------------------------------------------------------------
    // Input routing for the frameless window
    // ------------------------------------------------------------------

    const TEST_SCREEN: Vec2 = Vec2::new(1180.0, 840.0);

    fn test_raw(time: f64, events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, TEST_SCREEN)),
            time: Some(time),
            predicted_dt: 1.0 / 60.0,
            events,
            ..Default::default()
        }
    }

    fn pointer_moved(pos: Pos2) -> egui::Event {
        egui::Event::PointerMoved(pos)
    }

    fn pointer_button(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        }
    }

    fn viewport_commands(output: &egui::FullOutput) -> Vec<egui::ViewportCommand> {
        output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|viewport| viewport.commands.clone())
            .unwrap_or_default()
    }

    fn pixel_test_context() -> egui::Context {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        // `set_fonts` lands at the start of the next pass; an empty warm-up
        // frame binds the pixel family before the header asks for it.
        let _ = ctx.run(test_raw(0.0, Vec::new()), |_| {});
        ctx
    }

    /// The old full-screen `allocate_space` inside the foreground Area made
    /// egui's hidden-widget pass drop every lower-layer widget, so this click
    /// never arrived. It must keep arriving.
    #[test]
    fn frameless_resize_overlay_does_not_swallow_buttons() {
        let ctx = egui::Context::default();
        let button_rect = Rect::from_min_size(Pos2::new(200.0, 200.0), Vec2::new(160.0, 48.0));
        let center = button_rect.center();
        let mut clicked = false;
        let mut frame = |time: f64, events: Vec<egui::Event>| {
            ctx.run(test_raw(time, events), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    clicked |= ui.put(button_rect, egui::Button::new("test")).clicked();
                });
                frameless_resize(ctx);
            });
        };
        frame(0.0, vec![pointer_moved(center)]);
        frame(0.05, vec![pointer_moved(center)]);
        frame(0.10, vec![pointer_button(center, true)]);
        frame(0.15, vec![pointer_button(center, false)]);
        assert!(clicked, "the foreground resize strips must not hide the UI");
    }

    #[test]
    fn frameless_edge_requests_resize_and_center_does_not() {
        let ctx = egui::Context::default();
        let mut frame = |time: f64, events: Vec<egui::Event>| {
            ctx.run(test_raw(time, events), |ctx| {
                egui::CentralPanel::default().show(ctx, |_| {});
                frameless_resize(ctx);
            })
        };

        let edge = Pos2::new(2.0, TEST_SCREEN.y / 2.0);
        frame(0.0, vec![pointer_moved(edge)]);
        frame(0.05, vec![pointer_moved(edge)]);
        let pressed = frame(0.10, vec![pointer_button(edge, true)]);
        assert!(
            viewport_commands(&pressed).contains(&egui::ViewportCommand::BeginResize(
                egui::ResizeDirection::West
            )),
            "pressing the left edge must start a west resize"
        );

        frame(0.15, vec![pointer_button(edge, false)]);
        let center = Pos2::new(TEST_SCREEN.x / 2.0, TEST_SCREEN.y / 2.0);
        frame(0.20, vec![pointer_moved(center)]);
        let center_press = frame(0.25, vec![pointer_button(center, true)]);
        assert!(
            !viewport_commands(&center_press)
                .iter()
                .any(|command| matches!(command, egui::ViewportCommand::BeginResize(_))),
            "the centre of the window must stay free for the UI"
        );
    }

    #[test]
    fn title_bar_drag_from_empty_space_requests_start_drag() {
        let ctx = pixel_test_context();
        let mut show_help = false;
        let mut frame = |time: f64, events: Vec<egui::Event>| {
            ctx.run(test_raw(time, events), |ctx| header(&mut show_help, ctx))
        };

        let empty = Pos2::new(500.0, 40.0);
        frame(0.0, vec![pointer_moved(empty)]);
        frame(0.05, vec![pointer_button(empty, true)]);
        let dragged = frame(0.10, vec![pointer_moved(Pos2::new(560.0, 40.0))]);
        assert!(
            viewport_commands(&dragged).contains(&egui::ViewportCommand::StartDrag),
            "dragging the empty title-bar space must move the window"
        );
    }

    #[test]
    fn title_bar_close_is_clickable_under_frameless_overlay() {
        let ctx = pixel_test_context();
        let mut show_help = false;
        let mut frame = |time: f64, events: Vec<egui::Event>| {
            ctx.run(test_raw(time, events), |ctx| {
                header(&mut show_help, ctx);
                frameless_resize(ctx);
            })
        };

        // Right margin 20, 30 px controls; the row is vertically centred in
        // the 78 px title bar with a 10 px inner margin.
        let close = Pos2::new(TEST_SCREEN.x - 20.0 - WINDOW_CONTROL / 2.0, 10.0 + 29.0);
        frame(0.0, vec![pointer_moved(close)]);
        frame(0.05, vec![pointer_button(close, true)]);
        let released = frame(0.10, vec![pointer_button(close, false)]);
        assert!(
            viewport_commands(&released).contains(&egui::ViewportCommand::Close),
            "the window close control must receive its click"
        );
    }
}
