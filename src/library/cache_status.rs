//! Quiet, inline Photos-style status rows for indexing and thumbnail caching.
use super::{thumbnail_jobs::BulkStatus, ui::Tokens, Language};
use eframe::egui::{self, pos2, vec2, Align2, FontId, Rect, Sense, Stroke};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Action {
    None,
    Pause,
    Stop,
    Dismiss,
    Errors,
}

pub(super) fn show(
    ui: &mut egui::Ui,
    status: BulkStatus,
    lang: Language,
    t: &Tokens,
    margin: f32,
) -> Action {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
    let fraction = status.done as f32 / status.total.max(1) as f32;
    let title = if status.running {
        if status.paused {
            lang.text("Прогрузка приостановлена", "Caching paused")
        } else {
            lang.text("Прогрузка миниатюр", "Caching thumbnails")
        }
    } else if status.done >= status.total {
        if status.failed > 0 {
            lang.text("Готово с ошибками", "Finished with errors")
        } else {
            lang.text("Миниатюры готовы", "Thumbnails ready")
        }
    } else {
        lang.text("Прогрузка остановлена", "Caching stopped")
    };
    let counts = format!(
        "{} / {}{}",
        status.done,
        status.total,
        if status.skipped > 0 {
            format!(
                " · {}: {}",
                lang.text("Недоступно", "Unavailable"),
                status.skipped
            )
        } else {
            String::new()
        }
    );
    let errors = if status.failed > 0 {
        format!(" · {}: {}", lang.text("Ошибок", "Errors"), status.failed)
    } else {
        String::new()
    };
    let detail = format!(
        "{counts} · {:.0}%{errors}",
        fraction.clamp(0.0, 1.0) * 100.0
    );
    task_row(
        ui,
        r,
        title,
        &detail,
        Some(fraction),
        status.running,
        status.paused,
        status.failed > 0,
        lang,
        t,
        margin,
        false,
    )
}

/// Scanning uses the same visual language, never egui's default spinner/pill.
pub(super) fn scan(
    ui: &mut egui::Ui,
    done: usize,
    total: usize,
    paused: bool,
    lang: Language,
    t: &Tokens,
    margin: f32,
) -> Action {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
    let (detail, fraction) = if total == usize::MAX {
        (
            format!(
                "{}: {done} · {}",
                lang.text("Найдено", "Found"),
                lang.text("подсчёт файлов…", "counting files…")
            ),
            None,
        )
    } else if total < usize::MAX - 1 {
        let total = total.max(done);
        (
            format!("{}: {done} / {total}", lang.text("Обработано", "Processed")),
            Some(done as f32 / total.max(1) as f32),
        )
    } else {
        (
            format!("{}: {done}", lang.text("Обработка", "Processing")),
            None,
        )
    };
    let title = if paused {
        lang.text("Задача приостановлена", "Task paused")
    } else {
        lang.text("Обновление библиотеки", "Updating library")
    };
    task_row(
        ui, r, title, &detail, fraction, true, paused, false, lang, t, margin, true,
    )
}

#[allow(clippy::too_many_arguments)]
fn task_row(
    ui: &mut egui::Ui,
    r: Rect,
    title: &str,
    detail: &str,
    fraction: Option<f32>,
    running: bool,
    paused: bool,
    errors: bool,
    lang: Language,
    t: &Tokens,
    margin: f32,
    scanning: bool,
) -> Action {
    let p = ui.painter();
    p.rect_filled(r, 0.0, t.background);
    p.line_segment(
        [r.left_top(), r.right_top()],
        Stroke::new(1.0_f32, t.muted.gamma_multiply(0.16)),
    );
    let right = r.right() - margin;
    let buttons = if running { 80.0 } else { 40.0 };
    let text_rect = Rect::from_min_max(
        pos2(r.left() + margin + 36.0, r.top() + 4.0),
        pos2(
            (right - buttons - 12.0).max(r.left() + margin + 36.0),
            r.bottom() - 3.0,
        ),
    );
    // Own small photo-stack glyph; no animated spinner or filled icon badge.
    let icon = Rect::from_center_size(
        pos2(r.left() + margin + 11.0, r.center().y - 1.0),
        vec2(18.0, 14.0),
    );
    let stroke = Stroke::new(1.0_f32, t.muted);
    p.line_segment(
        [
            icon.left_top() + vec2(-3.0, 3.0),
            icon.left_bottom() + vec2(-3.0, 3.0),
        ],
        stroke,
    );
    p.line_segment(
        [
            icon.left_bottom() + vec2(-3.0, 3.0),
            icon.right_bottom() + vec2(-3.0, 3.0),
        ],
        stroke,
    );
    p.rect_stroke(icon, 0.0, stroke, egui::StrokeKind::Inside);
    p.add(egui::Shape::line(
        vec![
            icon.left_bottom() + vec2(2.0, -3.0),
            icon.center() + vec2(-2.0, -1.0),
            icon.center() + vec2(1.0, 2.0),
            icon.right_center() + vec2(-2.0, -1.0),
        ],
        stroke,
    ));
    let label = format!("{title}\n{detail}");
    let text = ui.interact(
        text_rect,
        ui.id().with("cache_task_text"),
        if errors {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    text.widget_info(|| {
        egui::WidgetInfo::labeled(
            if errors {
                egui::WidgetType::Button
            } else {
                egui::WidgetType::Label
            },
            ui.is_enabled(),
            &label,
        )
    });
    let show_errors = errors && text.clicked();
    text.on_hover_text(if errors {
        lang.text(
            "Показать файлы и причины ошибок",
            "Show files and error details",
        )
    } else {
        &label
    });
    let painter = ui
        .painter()
        .with_clip_rect(text_rect.intersect(ui.clip_rect()));
    painter.text(
        text_rect.left_top(),
        Align2::LEFT_TOP,
        title,
        FontId::proportional(13.0),
        t.text,
    );
    let detail_rect = Rect::from_min_size(
        text_rect.left_top() + vec2(0.0, 18.0),
        vec2(text_rect.width(), 17.0),
    );
    painter.text(
        detail_rect.left_top(),
        Align2::LEFT_TOP,
        detail,
        FontId::proportional(11.0),
        if errors { t.accent } else { t.muted },
    );
    // A separate accessible detail keeps processed/found counts discoverable.
    ui.interact(detail_rect, ui.id().with("task_detail"), Sense::hover())
        .widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), detail)
        });
    if running {
        let progress_painter = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
        let track = Rect::from_min_size(
            pos2(text_rect.left(), r.bottom() - 3.0),
            vec2(text_rect.width().clamp(0.0, 280.0), 1.0),
        );
        progress_painter.rect_filled(track, 0.0, t.muted.gamma_multiply(0.18));
        if let Some(fraction) = fraction {
            progress_painter.rect_filled(
                Rect::from_min_size(
                    track.min,
                    vec2(track.width() * fraction.clamp(0.0, 1.0), 1.0),
                ),
                0.0,
                if paused { t.muted } else { t.accent },
            );
        }
    }
    let mut action = if show_errors {
        Action::Errors
    } else {
        Action::None
    };
    for n in 0..if running { 2 } else { 1 } {
        let rect = Rect::from_min_size(
            pos2(right - buttons + n as f32 * 40.0, r.top() + 4.0),
            vec2(40.0, 40.0),
        );
        let pause = running && n == 0;
        let label = if pause {
            if scanning {
                if paused {
                    lang.text("Продолжить", "Resume")
                } else {
                    lang.text("Пауза", "Pause")
                }
            } else if paused {
                lang.text("Продолжить прогрузку", "Resume caching")
            } else {
                lang.text("Пауза прогрузки", "Pause caching")
            }
        } else if running {
            if scanning {
                lang.text("Остановить", "Stop")
            } else {
                lang.text("Остановить прогрузку", "Stop caching")
            }
        } else {
            lang.text("Скрыть уведомление", "Dismiss notification")
        };
        let response = ui.interact(rect, ui.id().with(("cache_task_action", n)), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
        });
        if response.is_pointer_button_down_on() {
            ui.painter().rect_filled(rect, 0.0, crate::reveal::ACCENT);
        }
        let c = rect.center();
        let s = Stroke::new(1.0_f32, t.text);
        if pause && paused {
            ui.painter().add(egui::Shape::closed_line(
                vec![
                    c + vec2(-4.0, -6.0),
                    c + vec2(5.0, 0.0),
                    c + vec2(-4.0, 6.0),
                ],
                s,
            ));
        } else if pause {
            for x in [-3.0, 3.0] {
                ui.painter()
                    .line_segment([c + vec2(x, -6.0), c + vec2(x, 6.0)], s);
            }
        } else {
            ui.painter()
                .line_segment([c + vec2(-5.0, -5.0), c + vec2(5.0, 5.0)], s);
            ui.painter()
                .line_segment([c + vec2(-5.0, 5.0), c + vec2(5.0, -5.0)], s);
        }
        if response.on_hover_text(label).clicked() {
            action = if pause {
                Action::Pause
            } else if running {
                Action::Stop
            } else {
                Action::Dismiss
            };
        }
    }
    action
}

impl super::Library {
    pub(super) fn cache_task_action(&mut self, action: Action) {
        match action {
            Action::None => {}
            Action::Pause => self.thumbnails.pause_bulk(),
            Action::Stop => self.thumbnails.cancel_bulk(),
            Action::Dismiss => self.thumbnails.dismiss_bulk(),
            Action::Errors => self.cache_errors_open = true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{kittest::Queryable, Harness};

    #[test]
    fn flat_task_strip_geometry_actions_and_completion_states() {
        for width in [1500.0, 500.0] {
            let mut h = Harness::builder()
                .with_size(vec2(width, 300.0))
                .build_state(
                    |ctx, (status, action)| {
                        let t = Tokens::new(super::super::Appearance::Light, ctx);
                        t.apply(ctx);
                        egui::CentralPanel::default()
                            .frame(egui::Frame::NONE)
                            .show(ctx, |ui| {
                                *action = show(ui, *status, Language::En, &t, 16.0);
                            });
                    },
                    (
                        BulkStatus {
                            total: 30000,
                            done: 12000,
                            running: true,
                            ..Default::default()
                        },
                        Action::None,
                    ),
                );
            h.run_steps(3);
            let pause = h.get_by_label("Pause caching").raw_bounds().unwrap();
            let stop = h.get_by_label("Stop caching").raw_bounds().unwrap();
            assert_eq!(pause.width(), 40.0);
            assert_eq!(pause.height(), 40.0);
            assert_eq!(pause.y0, 4.0);
            assert_eq!(stop.x1, f64::from(width) - 16.0);
            h.get_by_label("Pause caching").click();
            h.run_steps(1);
            assert_eq!(h.state().1, Action::Pause);
            h.state_mut().0.paused = true;
            h.run_steps(2);
            h.get_by_label("Resume caching");
            h.get_by_label("Stop caching").click();
            h.run_steps(1);
            assert_eq!(h.state().1, Action::Stop);
            h.state_mut().0.running = false;
            h.run_steps(2);
            h.get_by_label("Dismiss notification").click();
            h.run_steps(1);
            assert_eq!(h.state().1, Action::Dismiss);
            h.state_mut().0.failed = 18;
            h.state_mut().0.done = h.state().0.total;
            h.run_steps(3);
            let label = "Finished with errors\n30000 / 30000 · 100% · Errors: 18";
            h.get_by_label(label).click();
            h.run_steps(1);
            assert_eq!(h.state().1, Action::Errors);
        }
    }

    #[test]
    fn progress_is_a_local_hairline_not_a_full_width_bar_or_badge() {
        let ctx = egui::Context::default();
        let t = Tokens::new(super::super::Appearance::Light, &ctx);
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    show(
                        ui,
                        BulkStatus {
                            total: 10,
                            done: 4,
                            running: true,
                            ..Default::default()
                        },
                        Language::En,
                        &t,
                        16.0,
                    );
                });
        });
        let progress: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Rect(rect) if rect.fill == t.accent => Some(rect),
                _ => None,
            })
            .collect();
        assert_eq!(progress.len(), 1);
        assert_eq!(progress[0].rect.height(), 1.0);
        assert!(progress[0].rect.width() <= 280.0 * 0.4 + 0.01);
        assert!(progress[0].rect.left() >= 52.0);
        assert_eq!(progress[0].corner_radius, egui::CornerRadius::ZERO);
    }

    #[test]
    fn scanning_uses_same_row_and_keeps_pause_stop_and_counts() {
        let mut h = Harness::builder()
            .with_size(vec2(500.0, 200.0))
            .build_state(
                |ctx, action| {
                    let t = Tokens::new(super::super::Appearance::Light, ctx);
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            *action = scan(ui, 128, 1000, false, Language::En, &t, 16.0);
                        });
                },
                Action::None,
            );
        h.run_steps(3);
        h.get_by_label("Processed: 128 / 1000");
        h.get_by_label("Pause").click();
        h.run_steps(1);
        assert_eq!(*h.state(), Action::Pause);
        h.get_by_label("Stop").click();
        h.run_steps(1);
        assert_eq!(*h.state(), Action::Stop);
    }

    #[test]
    #[ignore = "requires an offscreen GPU and RPHOTOS_CAPTURE_DIR"]
    fn capture_cache_task_strip() {
        let output = std::path::PathBuf::from(
            std::env::var_os("RPHOTOS_CAPTURE_DIR").expect("set capture directory"),
        );
        std::fs::create_dir_all(&output).unwrap();
        for (name, width, dark, status) in [
            (
                "cache-active",
                1000.0,
                false,
                BulkStatus {
                    total: 30323,
                    done: 12000,
                    running: true,
                    ..Default::default()
                },
            ),
            (
                "cache-paused-narrow",
                500.0,
                false,
                BulkStatus {
                    total: 30323,
                    done: 12000,
                    running: true,
                    paused: true,
                    ..Default::default()
                },
            ),
            (
                "cache-finished",
                1000.0,
                false,
                BulkStatus {
                    total: 30323,
                    done: 30323,
                    ..Default::default()
                },
            ),
            (
                "cache-errors-dark",
                1000.0,
                true,
                BulkStatus {
                    total: 30323,
                    done: 30323,
                    failed: 12,
                    ..Default::default()
                },
            ),
            (
                "cache-stopped",
                500.0,
                false,
                BulkStatus {
                    total: 30323,
                    done: 12000,
                    ..Default::default()
                },
            ),
        ] {
            let mut h = Harness::builder()
                .with_size(vec2(width, 128.0))
                .build(move |ctx| {
                    let t = Tokens::new(
                        if dark {
                            super::super::Appearance::Dark
                        } else {
                            super::super::Appearance::Light
                        },
                        ctx,
                    );
                    t.apply(ctx);
                    egui::TopBottomPanel::bottom("task")
                        .exact_height(48.0)
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            show(ui, status, Language::Ru, &t, 16.0);
                        });
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE.fill(t.background))
                        .show(ctx, |_| {});
                });
            h.run_steps(3);
            h.render()
                .unwrap()
                .save(output.join(format!("{name}.png")))
                .unwrap();
        }
    }
}
