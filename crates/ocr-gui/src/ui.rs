//! The whole visible surface of the GUI.
//!
//! The layout follows the "empathy first" plan: one main path (files → folder →
//! Recognize), a preparation card for the first run, honest progress, calm
//! errors and a collapsed advanced section. Backend state lives in `main.rs`;
//! this module only reads and mutates it through the `OcrApp` fields.

use std::path::Path;
use std::time::{Duration, Instant};

use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Layout, Margin, RichText, Stroke, StrokeKind,
    Vec2,
};

use ocr_app::runtime::runtime_download_mb;
use ocr_app::setup::ModelStatus;
use ocr_core::{Device, OutputFormat};

use crate::{gui_log, prefs, status_text, FailureKind, OcrApp, StatusPhase};

const ACCENT_LIGHT: Color32 = Color32::from_rgb(37, 99, 235);
const ACCENT_DARK: Color32 = Color32::from_rgb(120, 162, 255);
const OK_COLOR: Color32 = Color32::from_rgb(52, 150, 92);
const WARN_LIGHT: Color32 = Color32::from_rgb(154, 103, 8);
const WARN_DARK: Color32 = Color32::from_rgb(232, 184, 82);
const ERROR_LIGHT: Color32 = Color32::from_rgb(184, 51, 51);
const ERROR_DARK: Color32 = Color32::from_rgb(239, 122, 122);

fn accent(dark: bool) -> Color32 {
    if dark {
        ACCENT_DARK
    } else {
        ACCENT_LIGHT
    }
}

/// Filled buttons use a deeper blue than links/accent text: white labels keep
/// enough contrast in both themes.
fn accent_fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(47, 95, 201)
    } else {
        ACCENT_LIGHT
    }
}

fn ok_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(107, 203, 142)
    } else {
        OK_COLOR
    }
}

fn warn_color(dark: bool) -> Color32 {
    if dark {
        WARN_DARK
    } else {
        WARN_LIGHT
    }
}

fn error_color(dark: bool) -> Color32 {
    if dark {
        ERROR_DARK
    } else {
        ERROR_LIGHT
    }
}

fn muted(ui: &egui::Ui) -> Color32 {
    ui.visuals().weak_text_color()
}

fn panel_fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(22, 24, 27)
    } else {
        Color32::from_rgb(244, 246, 249)
    }
}

fn card_fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(31, 34, 39)
    } else {
        Color32::from_rgb(255, 255, 255)
    }
}

fn soft_fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(38, 42, 48)
    } else {
        Color32::from_rgb(246, 248, 251)
    }
}

fn border(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(54, 59, 66)
    } else {
        Color32::from_rgb(218, 223, 230)
    }
}

/// Calm spacing, larger controls and one accent color. Applied to both themes
/// because egui follows the system theme by default.
pub fn apply_style(ctx: &egui::Context) {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let dark = theme == egui::Theme::Dark;
        ctx.style_mut_of(theme, |style| {
            style.spacing.item_spacing = Vec2::new(10.0, 10.0);
            style.spacing.button_padding = Vec2::new(14.0, 9.0);
            // Large hit targets everywhere; the main buttons are even larger.
            style.spacing.interact_size.y = 44.0;
            style.spacing.slider_width = 140.0;
            style
                .text_styles
                .insert(egui::TextStyle::Heading, FontId::proportional(23.0));
            style
                .text_styles
                .insert(egui::TextStyle::Body, FontId::proportional(15.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, FontId::proportional(15.0));
            style
                .text_styles
                .insert(egui::TextStyle::Small, FontId::proportional(13.0));
            style
                .text_styles
                .insert(egui::TextStyle::Monospace, FontId::monospace(13.0));

            let visuals = &mut style.visuals;
            visuals.panel_fill = panel_fill(dark);
            visuals.window_fill = card_fill(dark);
            visuals.extreme_bg_color = if dark {
                Color32::from_rgb(18, 20, 23)
            } else {
                Color32::from_rgb(250, 251, 253)
            };
            visuals.faint_bg_color = soft_fill(dark);
            visuals.code_bg_color = soft_fill(dark);
            visuals.window_corner_radius = CornerRadius::same(12);
            visuals.menu_corner_radius = CornerRadius::same(10);
            visuals.window_stroke = Stroke::new(1.0_f32, border(dark));
            visuals.selection.bg_fill = accent(dark).gamma_multiply(0.35);
            visuals.selection.stroke = Stroke::new(1.0_f32, accent(dark));
            visuals.hyperlink_color = accent(dark);
            visuals.warn_fg_color = warn_color(dark);
            visuals.error_fg_color = error_color(dark);

            for widget in [
                &mut visuals.widgets.inactive,
                &mut visuals.widgets.hovered,
                &mut visuals.widgets.active,
                &mut visuals.widgets.noninteractive,
            ] {
                widget.corner_radius = CornerRadius::same(9);
            }
            visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, border(dark));
            visuals.widgets.inactive.bg_fill = soft_fill(dark);
            visuals.widgets.inactive.weak_bg_fill = soft_fill(dark);
            visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, border(dark));
            visuals.widgets.hovered.bg_fill = if dark {
                Color32::from_rgb(48, 54, 62)
            } else {
                Color32::from_rgb(235, 240, 247)
            };
            visuals.widgets.hovered.weak_bg_fill = visuals.widgets.hovered.bg_fill;
            visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, accent(dark));
            visuals.widgets.active.bg_fill = accent(dark).gamma_multiply(0.22);
            visuals.widgets.active.weak_bg_fill = visuals.widgets.active.bg_fill;
        });
    }
}

fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let dark = ui.visuals().dark_mode;
    egui::Frame::new()
        .fill(card_fill(dark))
        .stroke(Stroke::new(1.0_f32, border(dark)))
        .corner_radius(14)
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            // Keep every card aligned to the full content width, even when its
            // content (for example a short error message) is narrower.
            ui.set_min_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn chip(ui: &mut egui::Ui, text: &str, tooltip: &str) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let inner = egui::Frame::new()
        .fill(accent(dark).gamma_multiply(0.12))
        .stroke(Stroke::new(1.0_f32, accent(dark).gamma_multiply(0.35)))
        .corner_radius(20)
        .inner_margin(Margin::symmetric(12, 5))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().strong().color(accent(dark)));
        });
    inner.response.on_hover_text(tooltip)
}

fn note(ui: &mut egui::Ui, text: &str, color: Color32) {
    let dark = ui.visuals().dark_mode;
    egui::Frame::new()
        .fill(color.gamma_multiply(if dark { 0.16 } else { 0.09 }))
        .corner_radius(9)
        .inner_margin(Margin::symmetric(12, 9))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(color));
        });
}

fn big_button(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(egui::Button::new(text).min_size(Vec2::new(0.0, 44.0)))
}

fn primary_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let fill = if enabled {
        accent_fill(dark)
    } else if dark {
        Color32::from_rgb(61, 66, 74)
    } else {
        Color32::from_rgb(205, 211, 220)
    };
    let text = if enabled {
        Color32::WHITE
    } else if dark {
        Color32::from_rgb(150, 156, 165)
    } else {
        Color32::from_rgb(120, 128, 138)
    };
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(label).size(18.0).strong().color(text))
            .fill(fill)
            .corner_radius(10)
            .min_size(Vec2::new(230.0, 50.0)),
    )
}

fn text_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(egui::Button::new(label).frame(false))
}

fn spinner(ui: &mut egui::Ui) {
    let color = accent(ui.visuals().dark_mode);
    ui.add(egui::Spinner::new().size(18.0).color(color));
}

fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(20.0).strong());
}

fn status_dot(ui: &mut egui::Ui, ready: bool, attention: bool) {
    let dark = ui.visuals().dark_mode;
    let (glyph, color) = if ready {
        ("•", ok_color(dark))
    } else if attention {
        ("!", warn_color(dark))
    } else {
        ("•", muted(ui))
    };
    ui.label(RichText::new(glyph).strong().size(17.0).color(color));
}

fn svg_badge(ui: &mut egui::Ui, badge: &str) {
    let dark = ui.visuals().dark_mode;
    let color = accent(dark);
    egui::Frame::new()
        .fill(color.gamma_multiply(0.12))
        .corner_radius(6)
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(badge).small().strong().color(color));
        });
}

fn card_gap(ui: &mut egui::Ui) {
    ui.add_space(12.0);
}

/// Entry point called from `eframe::App::update`.
pub fn draw(app: &mut OcrApp, ctx: &egui::Context) {
    apply_style(ctx);
    header(app, ctx);
    status_bar(app, ctx);
    action_bar(app, ctx);

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(panel_fill(ctx.style().visuals.dark_mode))
                .inner_margin(Margin::symmetric(20, 16)),
        )
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("main-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if app.running || app.cancelling {
                        // While a job runs the progress card is the most
                        // important thing on screen; the setup and output
                        // cards would only push it below the fold.
                        progress_card(app, ui);
                        card_gap(ui);
                        files_card(app, ui);
                    } else {
                        if app.last_report.is_some() {
                            result_card(app, ui);
                            card_gap(ui);
                        }
                        if app.last_error.is_some() {
                            error_card(app, ui);
                            card_gap(ui);
                        }
                        setup_card(app, ui);
                        card_gap(ui);
                        files_card(app, ui);
                        card_gap(ui);
                        output_card(app, ui);
                    }
                    card_gap(ui);
                    advanced_card(app, ui);
                    ui.add_space(24.0);
                });
        });
}

fn header(app: &mut OcrApp, ctx: &egui::Context) {
    let dark = ctx.style().visuals.dark_mode;
    egui::TopBottomPanel::top("header")
        .frame(
            egui::Frame::new()
                .fill(panel_fill(dark))
                .inner_margin(Margin::symmetric(20, 12)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("ocr-tool").size(26.0).strong());
                ui.add_space(2.0);
                ui.label(
                    RichText::new("Распознавание документов")
                        .size(15.0)
                        .color(muted(ui)),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("?").size(18.0).strong())
                                .min_size(Vec2::splat(40.0))
                                .corner_radius(20),
                        )
                        .on_hover_text("Как это работает")
                        .clicked()
                    {
                        app.show_help = !app.show_help;
                    }
                    chip(
                        ui,
                        "Локально • приватно",
                        "Файлы не отправляются в интернет. Сеть нужна только для загрузки модели и движка.",
                    );
                });
            });
        });
    if app.show_help {
        help_window(app, ctx);
    }
}

fn help_window(app: &mut OcrApp, ctx: &egui::Context) {
    let mut open = app.show_help;
    let mut close = false;
    egui::Window::new("Как это работает")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(460.0)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            let dark = ui.visuals().dark_mode;
            ui.label(RichText::new("Три шага").strong());
            for (number, text) in [
                (
                    "1.",
                    "Добавьте PDF или изображение — можно перетащить в окно.",
                ),
                (
                    "2.",
                    "Выберите папку для результата и нажмите «Распознать».",
                ),
                (
                    "3.",
                    "Готовые файлы появятся в этой папке. Их можно открыть сразу.",
                ),
            ] {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(number).strong().color(accent(dark)));
                    ui.label(text);
                });
            }
            ui.add_space(6.0);
            ui.separator();
            ui.label(
                "Модель (~3,4 ГБ) и движок (~69 МБ) скачиваются один раз. Документы \
                 остаются на этом компьютере.",
            );
            ui.label(
                "На процессоре (CPU) распознавание медленнее — примерно 10–15 минут на \
                 страницу. На видеокарте заметно быстрее.",
            );
            ui.add_space(10.0);
            if big_button(ui, "Понятно").clicked() {
                close = true;
            }
        });
    app.show_help = open && !close;
}

fn status_bar(app: &mut OcrApp, ctx: &egui::Context) {
    let dark = ctx.style().visuals.dark_mode;
    egui::TopBottomPanel::bottom("status")
        .frame(
            egui::Frame::new()
                .fill(panel_fill(dark))
                .inner_margin(Margin::symmetric(20, 8)),
        )
        .show(ctx, |ui| {
            let phase = app.status_phase();
            let status = status_text(phase, &app.status);
            ui.horizontal(|ui| {
                let busy = app.running || app.downloading || app.runtime_downloading;
                if busy {
                    spinner(ui);
                } else {
                    let (glyph, color) = match phase {
                        StatusPhase::Error => ("!", error_color(dark)),
                        StatusPhase::Notice => ("!", warn_color(dark)),
                        StatusPhase::Result {
                            cancelled: true, ..
                        } => ("!", warn_color(dark)),
                        StatusPhase::Result { .. } => ("•", ok_color(dark)),
                        _ => ("•", muted(ui)),
                    };
                    ui.label(RichText::new(glyph).strong().color(color));
                }
                ui.label(&status);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if app.logs.iter().any(|line| line.error) && text_button(ui, "Журнал").clicked()
                    {
                        app.show_advanced = true;
                        app.scroll_log = true;
                    }
                    ui.label(
                        RichText::new("Ctrl+Enter — распознать · Esc — отмена")
                            .small()
                            .color(muted(ui)),
                    );
                });
            });
        });
}

fn setup_card(app: &mut OcrApp, ui: &mut egui::Ui) {
    if app.setup_complete() {
        card(ui, |ui| {
            let dark = ui.visuals().dark_mode;
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("• Готово к работе")
                        .strong()
                        .size(17.0)
                        .color(ok_color(dark)),
                );
                ui.label(
                    RichText::new("Модель и движок на месте.")
                        .small()
                        .color(muted(ui)),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if big_button(ui, "Настроить…")
                        .on_hover_text("Папки модели и движка, устройство и журнал")
                        .clicked()
                    {
                        app.show_advanced = true;
                    }
                });
            });
        });
        return;
    }

    card(ui, |ui| {
        section_title(ui, "Осталось подготовить инструменты");
        ui.label(
            RichText::new("Это разовая загрузка. Документы никуда не отправляются.")
                .small()
                .color(muted(ui)),
        );
        ui.add_space(6.0);
        model_row(app, ui);
        ui.separator();
        runtime_row(app, ui);
    });
}

fn model_row(app: &mut OcrApp, ui: &mut egui::Ui) {
    let ready = app.model_status.is_ready();
    let incomplete = matches!(&app.model_status, ModelStatus::Incomplete { .. });
    let dark = ui.visuals().dark_mode;
    let busy = app.downloading || app.runtime_downloading || app.running;

    ui.horizontal(|ui| {
        status_dot(ui, ready, !ready);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Модель распознавания").strong());
                ui.label(
                    RichText::new("~3,4 ГБ · скачивается один раз")
                        .small()
                        .color(muted(ui)),
                );
            });
            let (text, color) = if ready {
                ("Готова".to_string(), ok_color(dark))
            } else if incomplete {
                (
                    "Файлы повреждены или неполные — нажмите «Заменить…».".to_string(),
                    warn_color(dark),
                )
            } else {
                (
                    "Ещё не скачана. Это самая большая часть подготовки.".to_string(),
                    muted(ui),
                )
            };
            ui.label(RichText::new(text).small().color(color));
        });
    });

    ui.horizontal_wrapped(|ui| {
        if !ready {
            let can_download = !busy;
            if ui
                .add_enabled(
                    can_download,
                    egui::Button::new("Скачать модель (~3,4 ГБ)").min_size(Vec2::new(0.0, 44.0)),
                )
                .clicked()
            {
                app.start_download();
            }
        }
        if ui
            .add_enabled(
                !busy,
                egui::Button::new("Указать папку…").min_size(Vec2::new(0.0, 44.0)),
            )
            .clicked()
        {
            app.pick_model_dir();
        }
        // Replacing only makes sense when some model files exist (incomplete
        // pair or a ready pair the user wants to re-download).
        if ready || incomplete {
            if ui
                .add_enabled(
                    !busy,
                    egui::Button::new("Заменить…").min_size(Vec2::new(0.0, 44.0)),
                )
                .on_hover_text("Удалить файлы модели и скачать заново")
                .clicked()
            {
                app.confirm_replace = true;
            }
        }
    });
    if app.confirm_replace {
        note(
            ui,
            "Замена удалит существующие файлы модели (включая недокачанные) и скачает \
             закреплённую версию заново.",
            warn_color(dark),
        );
        ui.horizontal(|ui| {
            if big_button(ui, RichText::new("Да, заменить").strong()).clicked() {
                app.start_download_force();
            }
            if big_button(ui, "Отмена").clicked() {
                app.confirm_replace = false;
            }
        });
    }
    if app.downloading {
        ui.add(
            egui::ProgressBar::new(app.progress)
                .desired_height(12.0)
                .fill(accent_fill(ui.visuals().dark_mode))
                .animate(true),
        );
        ui.label(
            RichText::new(if app.download_text.is_empty() {
                "Готовим загрузку…"
            } else {
                app.download_text.as_str()
            })
            .small()
            .color(muted(ui)),
        );
        ui.horizontal(|ui| {
            if big_button(ui, "Отменить загрузку").clicked() {
                app.download_cancel
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                app.status = "Останавливаем загрузку модели…".to_string();
            }
            spinner(ui);
        });
    }
}

fn runtime_row(app: &mut OcrApp, ui: &mut egui::Ui) {
    let ready = app.runtime.is_ready();
    let llama_ready = app.runtime.llama_ready();
    let dark = ui.visuals().dark_mode;
    let busy = app.downloading || app.runtime_downloading || app.running;
    let size = runtime_download_mb()
        .map(|mb| format!("~{mb} МБ"))
        .unwrap_or_else(|| "~69 МБ".to_string());

    ui.horizontal(|ui| {
        status_dot(ui, ready, !ready);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Движок распознавания и просмотра PDF").strong());
                ui.label(
                    RichText::new(format!("{size} · скачивается один раз"))
                        .small()
                        .color(muted(ui)),
                );
            });
            let (text, color) = if ready {
                ("Готов".to_string(), ok_color(dark))
            } else if llama_ready {
                (
                    "Движок есть, но не хватает программы просмотра PDF.".to_string(),
                    warn_color(dark),
                )
            } else {
                (
                    "Нужен для распознавания и для PDF. Скачивается один раз.".to_string(),
                    muted(ui),
                )
            };
            ui.label(RichText::new(text).small().color(color));
        });
    });

    ui.horizontal_wrapped(|ui| {
        if !ready {
            let label = match runtime_download_mb() {
                Some(mb) => format!("Скачать движок (~{mb} МБ)"),
                None => "Скачать движок".to_string(),
            };
            if ui
                .add_enabled(
                    !busy,
                    egui::Button::new(label).min_size(Vec2::new(0.0, 44.0)),
                )
                .clicked()
            {
                app.start_runtime_download(false);
            }
        }
        if ui
            .add_enabled(
                !busy,
                egui::Button::new("Указать папку…").min_size(Vec2::new(0.0, 44.0)),
            )
            .clicked()
        {
            app.pick_runtime_dir();
        }
        // Reinstalling is meaningful only when an engine is already present
        // (ready, or a broken/incomplete runtime with llama-server found).
        if ready || llama_ready {
            if ui
                .add_enabled(
                    !busy,
                    egui::Button::new("Переустановить…").min_size(Vec2::new(0.0, 44.0)),
                )
                .on_hover_text("Скачать закреплённую версию движка заново")
                .clicked()
            {
                app.confirm_runtime_replace = true;
            }
        }
    });
    if app.confirm_runtime_replace {
        note(
            ui,
            "Переустановка заменит движок и программу просмотра PDF в папке движка \
             закреплённой версией.",
            warn_color(dark),
        );
        ui.horizontal(|ui| {
            if big_button(ui, RichText::new("Да, переустановить").strong()).clicked()
            {
                app.start_runtime_download(true);
            }
            if big_button(ui, "Отмена").clicked() {
                app.confirm_runtime_replace = false;
            }
        });
    }

    if app.runtime_downloading {
        ui.add(
            egui::ProgressBar::new(app.progress)
                .desired_height(12.0)
                .fill(accent_fill(ui.visuals().dark_mode))
                .animate(true),
        );
        ui.label(
            RichText::new(if app.runtime_text.is_empty() {
                "Готовим загрузку…"
            } else {
                app.runtime_text.as_str()
            })
            .small()
            .color(muted(ui)),
        );
        ui.horizontal(|ui| {
            if big_button(ui, "Отменить загрузку").clicked() {
                app.runtime_cancel
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                app.status = "Останавливаем загрузку движка…".to_string();
            }
            spinner(ui);
        });
    }
}

fn files_card(app: &mut OcrApp, ui: &mut egui::Ui) {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            section_title(ui, "1. Что распознать?");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                chip(
                    ui,
                    "Файлы остаются на этом компьютере",
                    "Текст распознаётся локально. В интернет уходят только загрузки модели и движка.",
                );
            });
        });

        drop_zone(app, ui);

        ui.horizontal_wrapped(|ui| {
            let can_edit = !app.running;
            if ui
                .add_enabled(
                    can_edit,
                    egui::Button::new("Выбрать файлы…").min_size(Vec2::new(0.0, 44.0)),
                )
                .on_hover_text("Ctrl+O")
                .clicked()
            {
                app.pick_files();
            }
            if ui
                .add_enabled(
                    can_edit,
                    egui::Button::new("Выбрать папку…").min_size(Vec2::new(0.0, 44.0)),
                )
                .clicked()
            {
                app.pick_folder();
            }
            if !app.inputs.is_empty()
                && ui
                    .add_enabled(can_edit, egui::Button::new("Очистить"))
                    .clicked()
            {
                app.inputs.clear();
                app.status = "Список очищен. Добавьте файлы, чтобы начать.".to_string();
            }
            ui.label(
                RichText::new("PDF, PNG, JPEG, TIFF, WebP, BMP")
                    .small()
                    .color(muted(ui)),
            );
        });

        if app.inputs.is_empty() {
            ui.label(
                "Пока пусто. Добавьте счёт, договор или скан — текст распознается на этом \
                 компьютере.",
            );
            ui.label(
                RichText::new(
                    "Поддерживаются PDF и изображения. Можно перетащить файл в любое место окна.",
                )
                .small()
                .color(muted(ui)),
            );
        } else {
            let mut remove = None;
            egui::ScrollArea::vertical()
                .id_salt("input-files")
                .max_height(230.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for (index, path) in app.inputs.iter().enumerate() {
                        let badge = if path.is_dir() {
                            "ПАПКА"
                        } else if is_pdf(path) {
                            "PDF"
                        } else {
                            "ФОТО"
                        };
                        ui.horizontal(|ui| {
                            svg_badge(ui, badge);
                            let row = ui
                                .vertical(|ui| {
                                    ui.label(RichText::new(display_name(path)).strong().size(14.5));
                                    ui.label(
                                        RichText::new(input_detail(path)).small().color(muted(ui)),
                                    );
                                })
                                .response;
                            row.on_hover_text(path.display().to_string());
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add_enabled(
                                        !app.running,
                                        egui::Button::new("×")
                                            .min_size(Vec2::new(38.0, 38.0))
                                            .corner_radius(8),
                                    )
                                    .on_hover_text("Убрать из списка")
                                    .clicked()
                                {
                                    remove = Some(index);
                                }
                            });
                        });
                        ui.add_space(2.0);
                    }
                });
            if let Some(index) = remove {
                app.inputs.remove(index);
                if app.inputs.is_empty() {
                    app.status = "Список очищен. Добавьте файлы, чтобы начать.".to_string();
                }
            }
        }

        let has_pdf = app.inputs.iter().any(|path| path.is_dir() || is_pdf(path));
        if has_pdf {
            let force_open = app.show_password;
            app.show_password = false;
            egui::CollapsingHeader::new("PDF с паролем?")
                .open(if force_open { Some(true) } else { None })
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(
                            "Пароль нужен только для защищённых PDF. Он не сохраняется и не \
                             попадает в журнал.",
                        )
                        .small()
                        .color(muted(ui)),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut app.password)
                            .password(true)
                            .hint_text("Пароль (необязательно)")
                            .desired_width(280.0),
                    );
                });
        }
    });
}

fn drop_zone(app: &mut OcrApp, ui: &mut egui::Ui) {
    let height = 86.0;
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        egui::Sense::click(),
    );
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let dark = ui.visuals().dark_mode;
    let hovered = response.hovered() && !app.running;
    let fill = if hovered {
        accent(dark).gamma_multiply(0.09)
    } else {
        soft_fill(dark)
    };
    let stroke = Stroke::new(
        if hovered { 1.5_f32 } else { 1.0_f32 },
        if hovered { accent(dark) } else { border(dark) },
    );
    ui.painter().rect(
        rect,
        CornerRadius::same(12),
        fill,
        stroke,
        StrokeKind::Inside,
    );
    let main_color = if app.running {
        muted(ui)
    } else {
        ui.visuals().text_color()
    };
    ui.painter().text(
        rect.center() - Vec2::new(0.0, 13.0),
        Align2::CENTER_CENTER,
        "Перетащите PDF или изображение сюда",
        FontId::proportional(17.0),
        main_color,
    );
    ui.painter().text(
        rect.center() + Vec2::new(0.0, 14.0),
        Align2::CENTER_CENTER,
        if app.running {
            "Дождитесь окончания распознавания"
        } else {
            "или нажмите, чтобы выбрать файлы"
        },
        FontId::proportional(13.0),
        muted(ui),
    );
    if response.clicked() && !app.running {
        app.pick_files();
    }
}

fn output_card(app: &mut OcrApp, ui: &mut egui::Ui) {
    card(ui, |ui| {
        section_title(ui, "2. Куда сохранить?");
        ui.horizontal(|ui| {
            ui.label(RichText::new("Папка").color(muted(ui)));
            let path = if app.output_dir.trim().is_empty() {
                "не выбрана".to_string()
            } else {
                app.output_dir.clone()
            };
            let path_response = ui.add(
                egui::Label::new(RichText::new(path).strong())
                    .truncate()
                    .sense(egui::Sense::hover()),
            );
            path_response.on_hover_text(app.output_dir.clone());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if big_button(ui, "Открыть").clicked() {
                    app.open_output_folder();
                }
                if big_button(ui, "Изменить…").clicked() {
                    app.pick_output_dir();
                }
            });
        });
        if app.output_dir.trim().is_empty() {
            ui.colored_label(
                error_color(ui.visuals().dark_mode),
                "Выберите папку для результата.",
            );
        }

        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            let dark = ui.visuals().dark_mode;
            for (value, label) in [
                (OutputFormat::Md, "Markdown"),
                (OutputFormat::Html, "HTML"),
                (OutputFormat::Both, "Оба"),
            ] {
                let selected = app.format == value;
                let text = if selected {
                    RichText::new(label).strong().color(Color32::WHITE)
                } else {
                    RichText::new(label)
                };
                let button = egui::Button::new(text)
                    .selected(selected)
                    .fill(if selected {
                        accent_fill(dark)
                    } else {
                        soft_fill(dark)
                    })
                    .stroke(Stroke::new(
                        1.0_f32,
                        if selected {
                            accent_fill(dark)
                        } else {
                            border(dark)
                        },
                    ))
                    .min_size(Vec2::new(120.0, 44.0));
                if ui.add(button).clicked() {
                    if app.format != value {
                        app.format = value;
                        app.save_prefs();
                    }
                }
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new(format_hint(app.format))
                    .small()
                    .color(muted(ui)),
            );
        });
    });
}

/// Persistent primary action above the status bar. Keeping it outside the
/// scroll area means «Распознать» stays visible with a long file list or while
/// the setup card is on screen.
fn action_bar(app: &mut OcrApp, ctx: &egui::Context) {
    let dark = ctx.style().visuals.dark_mode;
    egui::TopBottomPanel::bottom("primary-action")
        .frame(
            egui::Frame::new()
                .fill(card_fill(dark))
                .stroke(Stroke::new(1.0_f32, border(dark)))
                .inner_margin(Margin::symmetric(20, 10)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let enabled = app.can_start();
                let response = primary_button(ui, "Распознать", enabled);
                let response = if let Some(hint) = app.start_blocker() {
                    response.on_disabled_hover_text(hint)
                } else {
                    response.on_hover_text("Ctrl+Enter")
                };
                if response.clicked() {
                    app.start();
                }
                if app.running {
                    if big_button(ui, "Отмена")
                        .on_hover_text("Esc · уже готовые страницы сохранятся")
                        .clicked()
                    {
                        app.cancel_job();
                    }
                    spinner(ui);
                }
                let cpu = app.is_cpu_mode() && !app.running && app.start_blocker().is_none();
                let hint = if app.running {
                    "Идёт распознавание — можно свернуть окно.".to_string()
                } else if let Some(blocker) = app.start_blocker() {
                    blocker.to_string()
                } else if cpu {
                    "Процессор: ~10–15 мин на страницу. Можно свернуть окно.".to_string()
                } else {
                    "Можно свернуть окно — распознавание продолжится.".to_string()
                };
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(hint).small().color(if cpu {
                        warn_color(dark)
                    } else {
                        muted(ui)
                    }));
                });
            });
        });
}

fn progress_card(app: &mut OcrApp, ui: &mut egui::Ui) {
    let dark = ui.visuals().dark_mode;
    card(ui, |ui| {
        ui.horizontal(|ui| {
            spinner(ui);
            if app.cancelling {
                section_title(ui, "Останавливаем…");
            } else {
                section_title(ui, "Распознаём…");
            }
            if let Some(file) = &app.current_file {
                ui.label(RichText::new(file).color(muted(ui)));
            }
        });
        if app.current_file.is_none() {
            ui.label("Готовим документы и движок…");
        } else if let Some(line) = progress_line(app) {
            ui.label(RichText::new(line).strong());
        }
        ui.add(
            egui::ProgressBar::new(app.job_progress)
                .desired_height(14.0)
                .fill(accent_fill(dark))
                .animate(!app.cancelling),
        );
        if app.is_cpu_mode() {
            note(
                ui,
                "Работаем на CPU — это медленнее, но надёжно: ~10–15 минут на страницу.",
                warn_color(dark),
            );
        }
        ui.horizontal(|ui| {
            if big_button(ui, "Отмена")
                .on_hover_text("Esc · готовые страницы сохранятся")
                .clicked()
            {
                app.cancel_job();
            }
            if app.cancelling {
                ui.label(RichText::new("Ждём текущую страницу…").color(muted(ui)));
            }
        });
        ui.label(
            RichText::new("Можно свернуть окно — распознавание продолжится.")
                .small()
                .color(muted(ui)),
        );
        details_block(app, ui);
    });
}

fn details_block(app: &mut OcrApp, ui: &mut egui::Ui) {
    // A button may request the section even after the user closed it once, so
    // honour the flag as a one-frame force-open command.
    let force_open = app.show_details;
    app.show_details = false;
    egui::CollapsingHeader::new("Подробности")
        .open(if force_open { Some(true) } else { None })
        .show(ui, |ui| {
            job_log(app, ui, 220.0);
        });
}

fn result_card(app: &mut OcrApp, ui: &mut egui::Ui) {
    let Some(report) = app.last_report.clone() else {
        return;
    };
    let dark = ui.visuals().dark_mode;
    let partial = report.documents_failed > 0 || report.pages_failed > 0;
    let html = first_html(&report);
    card(ui, |ui| {
        if report.cancelled {
            section_title(ui, "Остановлено");
            ui.label(format!(
                "Сохранено: {} {}, {} {}.",
                report.documents_ok,
                plural_ru(report.documents_ok, "файл", "файла", "файлов"),
                report.pages_ok,
                plural_ru(report.pages_ok, "страница", "страницы", "страниц"),
            ));
        } else {
            section_title(
                ui,
                &format!(
                    "Готово: {} {}, {} {}",
                    report.documents_ok,
                    plural_ru(report.documents_ok, "файл", "файла", "файлов"),
                    report.pages_ok,
                    plural_ru(report.pages_ok, "страница", "страницы", "страниц"),
                ),
            );
            if partial {
                ui.label(
                    RichText::new("С замечаниями: часть страниц не удалась, готовые сохранены.")
                        .color(warn_color(dark)),
                );
            }
        }

        ui.horizontal(|ui| {
            ui.label(RichText::new("Папка").color(muted(ui)));
            let path = report.output_dir.clone();
            ui.add(
                egui::Label::new(RichText::new(&path).strong())
                    .truncate()
                    .sense(egui::Sense::hover()),
            )
            .on_hover_text(&path);
        });

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
                    "{} не удалось, остальные сохранены. Проверьте журнал — там причина без \
                     потери уже готовых файлов.",
                    failures.join(" и "),
                ),
                warn_color(dark),
            );
            ui.horizontal(|ui| {
                if big_button(ui, "Показать журнал").clicked() {
                    app.show_details = true;
                }
                if big_button(ui, "Открыть файл журнала").clicked() {
                    app.open_log_file();
                }
            });
        }

        note(
            ui,
            "Проверьте юридические и финансовые документы: печати, рукописный текст и плохие \
             сканы могут распознаваться с ошибками.",
            warn_color(dark),
        );

        ui.horizontal_wrapped(|ui| {
            if big_button(ui, "Открыть папку").clicked() {
                app.open_folder_path(std::path::PathBuf::from(&report.output_dir));
            }
            if let Some(html) = html {
                if big_button(ui, "Открыть HTML").clicked() {
                    app.open_path(&html);
                }
            }
            if big_button(ui, RichText::new("Распознать ещё").strong()).clicked() {
                app.reset_for_new_batch();
            }
        });
        details_block(app, ui);
    });
}

fn error_card(app: &mut OcrApp, ui: &mut egui::Ui) {
    let Some(message) = app.last_error.clone() else {
        return;
    };
    let dark = ui.visuals().dark_mode;
    let kind = app.last_error_kind;
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("!")
                    .strong()
                    .size(20.0)
                    .color(error_color(dark)),
            );
            section_title(ui, "Не получилось");
        });
        ui.label(RichText::new(message).size(15.0));
        ui.label(
            RichText::new("Документы не изменены. Подробности с технической причиной — в журнале.")
                .small()
                .color(muted(ui)),
        );
        ui.horizontal_wrapped(|ui| {
            match kind {
                FailureKind::Model => {
                    if big_button(ui, "Скачать модель").clicked() {
                        app.start_download();
                    }
                    if big_button(ui, "Указать папку…").clicked() {
                        app.pick_model_dir();
                    }
                }
                FailureKind::Runtime | FailureKind::Pdfium => {
                    if big_button(ui, "Скачать движок").clicked() {
                        app.start_runtime_download(false);
                    }
                    if big_button(ui, "Указать папку…").clicked() {
                        app.pick_runtime_dir();
                    }
                }
                FailureKind::File => {
                    if big_button(ui, "Выбрать другой файл…").clicked() {
                        app.pick_files();
                    }
                }
                FailureKind::Output => {
                    if big_button(ui, "Выбрать другую папку…").clicked() {
                        app.pick_output_dir();
                    }
                }
                FailureKind::Password => {
                    if big_button(ui, "Ввести пароль").clicked() {
                        app.show_password = true;
                    }
                }
                FailureKind::Pages => {
                    if big_button(ui, "Исправить страницы").clicked() {
                        app.show_advanced = true;
                    }
                }
                FailureKind::Timeout | FailureKind::Unknown => {}
            }
            if kind != FailureKind::Model
                && kind != FailureKind::Runtime
                && kind != FailureKind::Pdfium
                && matches!(
                    kind,
                    FailureKind::Timeout | FailureKind::Unknown | FailureKind::Password
                )
                && big_button(ui, "Повторить").clicked()
            {
                app.start();
            }
            if big_button(ui, "Журнал").clicked() {
                app.show_details = true;
            }
            if text_button(ui, "Скрыть").clicked() {
                app.last_error = None;
                app.status = "Ошибка скрыта. Можно попробовать снова.".to_string();
            }
        });
        details_block(app, ui);
    });
}

fn advanced_card(app: &mut OcrApp, ui: &mut egui::Ui) {
    let dark = ui.visuals().dark_mode;
    let force_open = app.show_advanced;
    app.show_advanced = false;
    let mut scrolled_log = false;
    let response = egui::CollapsingHeader::new("Для продвинутых")
        .open(if force_open { Some(true) } else { None })
        .show(ui, |ui| {
            egui::Frame::new()
                .fill(card_fill(dark))
                .stroke(Stroke::new(1.0_f32, border(dark)))
                .corner_radius(14)
                .inner_margin(Margin::same(14))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(
                            "Технические пути и параметры. Обычно здесь ничего менять не нужно.",
                        )
                        .small()
                        .color(muted(ui)),
                    );
                    advanced_grid(app, ui);
                    ui.add_space(6.0);
                    let log_header = job_log_header(app, ui);
                    if app.scroll_log {
                        app.scroll_log = false;
                        log_header.scroll_to_me(Some(Align::TOP));
                        scrolled_log = true;
                    }
                    job_log(app, ui, 220.0);
                });
        });
    if force_open && !scrolled_log {
        response.header_response.scroll_to_me(Some(Align::TOP));
    }
}

fn advanced_grid(app: &mut OcrApp, ui: &mut egui::Ui) {
    let mut changed = false;
    egui::Grid::new("advanced-grid")
        .num_columns(2)
        .spacing([14.0, 10.0])
        .show(ui, |ui| {
            ui.label("Model dir");
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut app.model_dir)
                            .hint_text("авто: OCR_MODEL_DIR или app-data")
                            .desired_width(380.0),
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui.button("…").clicked() {
                    app.pick_model_dir();
                }
            });
            ui.end_row();

            ui.label("Runtime dir");
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut app.runtime_dir)
                            .hint_text("авто: app-data/runtime")
                            .desired_width(380.0),
                    )
                    .changed()
                {
                    app.apply_runtime_dir_text();
                }
                if ui.button("…").clicked() {
                    app.pick_runtime_dir();
                }
            });
            ui.end_row();

            ui.label("llama-server");
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut app.llama_dir)
                            .hint_text("авто: OCR_LLAMA_SERVER или runtime/")
                            .desired_width(380.0),
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui.button("…").clicked() {
                    app.pick_llama_dir();
                }
            });
            ui.end_row();

            ui.label("PDFium");
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut app.pdfium)
                            .hint_text("авто: OCR_PDFIUM, рядом с exe, runtime/")
                            .desired_width(380.0),
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui.button("…").clicked() {
                    app.pick_pdfium();
                }
            });
            ui.end_row();

            ui.label("Устройство");
            egui::ComboBox::from_id_salt("device")
                .selected_text(device_label(app.device))
                .show_ui(ui, |ui| {
                    for device in [Device::Auto, Device::Gpu, Device::Cpu] {
                        if ui
                            .selectable_value(&mut app.device, device, device_label(device))
                            .changed()
                        {
                            app.save_prefs();
                        }
                    }
                });
            ui.end_row();

            ui.label("DPI");
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

            ui.label("Страницы");
            ui.add(
                egui::TextEdit::singleline(&mut app.pages)
                    .hint_text("все, например 1-5,7")
                    .desired_width(220.0),
            );
            ui.end_row();

            ui.label("Max pixels");
            ui.add(
                egui::DragValue::new(&mut app.max_pixels)
                    .range(1_000_000..=400_000_000)
                    .speed(1_000_000.0),
            );
            ui.end_row();

            ui.label("Max tokens");
            ui.add(
                egui::DragValue::new(&mut app.max_tokens)
                    .range(128..=32_768)
                    .speed(64.0),
            );
            ui.end_row();

            ui.label("Timeout, с");
            ui.add(
                egui::DragValue::new(&mut app.timeout_secs)
                    .range(30..=7200)
                    .speed(10.0),
            );
            ui.end_row();

            ui.label("Подпапки");
            ui.checkbox(&mut app.recursive, "Обходить вложенные папки");
            ui.end_row();

            ui.label("Пути");
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(format!("Модель: {}", app.model_status.dir().display()))
                        .small()
                        .color(muted(ui)),
                );
                ui.label(
                    RichText::new(format!(
                        "PDFium: {}",
                        app.runtime
                            .pdfium
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|error| format!("не найден ({error})")),
                    ))
                    .small()
                    .color(muted(ui)),
                );
                ui.label(
                    RichText::new(format!(
                        "llama-server: {} вариант(ов), просмотрено {} мест",
                        app.runtime.llama.len(),
                        app.runtime.llama_searched.len()
                    ))
                    .small()
                    .color(muted(ui)),
                );
                if let Some(warning) = app.runtime.llama_warnings.first() {
                    ui.label(
                        RichText::new(warning)
                            .small()
                            .color(warn_color(ui.visuals().dark_mode)),
                    );
                }
            });
            ui.end_row();
        });
    if changed {
        app.refresh_setup();
    }
}

fn job_log_header(app: &mut OcrApp, ui: &mut egui::Ui) -> egui::Response {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Журнал").strong());
        if text_button(ui, "Копировать").clicked() {
            let text = app
                .logs
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            ui.ctx().copy_text(text);
        }
        if text_button(ui, "Очистить").clicked() {
            app.logs.clear();
        }
        if text_button(ui, "Открыть файл журнала").clicked() {
            app.open_log_file();
        }
    })
    .response
}

fn job_log(app: &mut OcrApp, ui: &mut egui::Ui, height: f32) {
    egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .corner_radius(8)
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("job-log")
                .max_height(height)
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if app.logs.is_empty() {
                        ui.label(RichText::new("События появятся здесь.").color(muted(ui)));
                    }
                    for line in &app.logs {
                        let mut text = RichText::new(&line.text).monospace().size(12.5);
                        if line.error {
                            text = text.color(error_color(ui.visuals().dark_mode));
                        }
                        ui.label(text);
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

fn format_hint(format: OutputFormat) -> &'static str {
    match format {
        OutputFormat::Md => "Markdown — для текста, поиска и правок.",
        OutputFormat::Html => "HTML — для чтения в браузере с форматированием.",
        OutputFormat::Both => "Markdown — для текста, HTML — для просмотра. Сохраняются оба.",
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
    if let Some(eta) = page_eta(app) {
        parts.push(format!("осталось ~{eta}"));
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

fn page_eta(app: &OcrApp) -> Option<String> {
    if app.pages_done == 0 || app.page_selected_total == 0 {
        return None;
    }
    let remaining_pages = app
        .page_selected_total
        .saturating_sub(app.page_selected_index);
    if remaining_pages == 0 {
        return None;
    }
    let average = app.page_seconds_total / app.pages_done as f64;
    let seconds = average * remaining_pages as f64;
    if !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    Some(format_seconds(seconds))
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
        self.status = "Добавьте файлы — начнём с шага 1.".to_string();
    }

    pub(crate) fn save_prefs(&mut self) {
        prefs::save(&prefs::GuiPrefs {
            output_dir: Some(self.output_dir.clone()),
            format: Some(self.format.as_str().to_string()),
            device: Some(self.device.as_str().to_string()),
            dpi: Some(self.dpi),
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
    fn eta_formats_seconds_and_minutes() {
        assert_eq!(format_seconds(12.4), "13 с");
        assert_eq!(format_seconds(59.1), "60 с");
        assert_eq!(format_seconds(61.0), "2 мин");
        assert_eq!(format_seconds(3700.0), "1 ч 2 мин");
    }

    #[test]
    fn download_eta_needs_progress_and_a_clock() {
        assert_eq!(download_eta(0, 100, Some(Instant::now())), None);
        assert_eq!(download_eta(10, 0, Some(Instant::now())), None);
        assert_eq!(download_eta(10, 100, None), None);
    }

    #[test]
    fn format_hints_stay_human() {
        assert!(format_hint(OutputFormat::Both).contains("Markdown"));
        assert!(format_hint(OutputFormat::Html).contains("браузере"));
        assert!(!format_hint(OutputFormat::Md).contains("HTML"));
    }
}
