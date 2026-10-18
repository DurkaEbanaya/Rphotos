//! One-finger section pager: vertical motion stays with ScrollArea, multi-touch
//! cancels the pager. Pointer/mouse events alone never trigger section changes.
use super::{Library, Section};
use eframe::egui::{self, Pos2, Vec2};

#[derive(Default)]
pub(super) struct Preview {
    section: Option<Section>,
    rows: Vec<super::GridRow>,
    months: Vec<super::Month>,
    offsets: Vec<f32>,
    filtered: Vec<usize>,
    layout: Option<super::LayoutKey>,
    scroll: f32,
    anchor: Option<(i64, f32)>,
    month_target: Option<String>,
    focus: Option<i64>,
    widget: Option<egui::Id>,
}

impl Preview {
    fn exchange(&mut self, library: &mut Library) {
        std::mem::swap(&mut self.rows, &mut library.rows);
        std::mem::swap(&mut self.months, &mut library.months);
        std::mem::swap(&mut self.offsets, &mut library.row_offsets);
        std::mem::swap(&mut self.filtered, &mut library.filtered);
        std::mem::swap(&mut self.layout, &mut library.layout_key);
        std::mem::swap(&mut self.scroll, &mut library.scroll_offset);
        std::mem::swap(&mut self.anchor, &mut library.scroll_anchor);
        std::mem::swap(&mut self.month_target, &mut library.scroll_to_month);
        std::mem::swap(&mut self.focus, &mut library.focused_photo);
        std::mem::swap(&mut self.widget, &mut library.focused_photo_widget);
    }
}

#[derive(Default)]
pub(super) struct Pager {
    start: Option<Pos2>,
    delta: Vec2,
    horizontal: bool,
    pub offset: f32,
    pub consumed: bool,
    // Relative index of the page exposed next to the current page. After a
    // committed swipe this points back at the departing page until snap ends.
    pub adjacent: Option<i32>,
    preview: Preview,
}

impl Pager {
    fn update(
        &mut self,
        events: &[egui::Event],
        top: f32,
        width: f32,
        enabled: bool,
        dt: f32,
    ) -> Option<i32> {
        let mut direction = None;
        self.consumed = false;
        if !enabled {
            self.start = None;
            self.horizontal = false;
            self.delta = Vec2::ZERO;
            self.offset = 0.0;
            self.adjacent = None;
        }
        for event in events {
            if let egui::Event::Touch { phase, pos, .. } = event {
                if !enabled {
                    continue;
                }
                match phase {
                    egui::TouchPhase::Start => {
                        if self.start.is_some() {
                            self.start = None;
                            self.horizontal = false;
                        } else if pos.y > top {
                            self.start = Some(*pos);
                            self.delta = Vec2::ZERO;
                        }
                    }
                    egui::TouchPhase::Move => {
                        if let Some(start) = self.start {
                            self.delta = *pos - start;
                            if !self.horizontal
                                && self.delta.y.abs() > 16.0
                                && self.delta.y.abs() > self.delta.x.abs()
                            {
                                self.start = None;
                            } else if self.delta.x.abs() > 16.0
                                && self.delta.x.abs() > self.delta.y.abs() * 1.4
                            {
                                self.horizontal = true;
                            }
                            if self.horizontal {
                                self.offset = self.delta.x.clamp(-width * 0.85, width * 0.85);
                            }
                        }
                    }
                    egui::TouchPhase::End => {
                        self.consumed = self.horizontal;
                        if self.start.take().is_some()
                            && self.horizontal
                            && self.delta.x.abs() > 72.0
                            && self.delta.x.abs() > self.delta.y.abs() * 1.4
                        {
                            direction = Some(if self.delta.x < 0.0 { 1 } else { -1 });
                        }
                        self.horizontal = false;
                    }
                    egui::TouchPhase::Cancel => {
                        self.start = None;
                        self.horizontal = false;
                    }
                }
            }
        }
        if !self.horizontal && direction.is_none() {
            self.offset *= (-dt.clamp(0.0, 0.1) * 22.0).exp();
            if self.offset.abs() < 0.5 {
                self.offset = 0.0;
            }
        }
        self.consumed |= self.horizontal || self.offset.abs() > 0.5;
        direction
    }
}

impl Library {
    pub(super) fn invalidate_pager_preview(&mut self) {
        self.pager.preview.layout = None;
    }
    /// Paint the actual neighboring scene, with isolated layout/scroll/focus and
    /// disabled hit targets. This reuses the bounded thumbnail queue, not a
    /// screenshot of the desktop or a second scanner/database worker.
    pub(super) fn pager_preview(
        &mut self,
        ui: &mut egui::Ui,
        settings: &crate::settings::AppSettings,
        tokens: &super::ui::Tokens,
        margin: f32,
    ) {
        let Some(relative) = self.pager.adjacent else {
            return;
        };
        let current = match self.section {
            Section::Collection => 0,
            Section::Albums => 1,
            Section::Folders => 2,
        };
        let rect = ui.max_rect().translate(egui::vec2(
            self.pager.offset + relative as f32 * ui.max_rect().width(),
            0.0,
        ));
        let original = self.section;
        let mut preview = std::mem::take(&mut self.pager.preview);
        let target = section(current + relative);
        if preview.section != Some(target) {
            preview = Preview::default();
            preview.section = Some(target);
        }
        preview.exchange(self);
        self.section = target;
        // A detached child must not advance the parent cursor: otherwise the
        // current page would be laid out below the preview rather than beside it.
        let mut preview_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("pager_preview")
                .max_rect(rect),
        );
        preview_ui.disable();
        preview_ui.set_clip_rect(ui.clip_rect().intersect(rect));
        preview_ui
            .painter()
            .rect_filled(rect, 0.0, tokens.background);
        self.section_content(&mut preview_ui, settings, tokens, margin);
        self.section = original;
        preview.exchange(self);
        self.pager.preview = preview;
    }

    pub(super) fn pager(&mut self, ctx: &egui::Context, top: f32, blocked: bool) {
        let enabled = !blocked
            && !ctx.wants_keyboard_input()
            && !self.selecting
            && !self.semantic_zoom
            && self.album.is_none()
            && self.folder.is_none()
            && !self.popup_open_last_frame;
        let (events, width, dt, multi) = ctx.input(|i| {
            (
                i.events.clone(),
                i.screen_rect.width(),
                i.stable_dt,
                i.multi_touch().is_some_and(|t| t.num_touches > 1),
            )
        });
        let direction = self
            .pager
            .update(&events, top, width, enabled && !multi, dt);
        let current = match self.section {
            Section::Collection => 0,
            Section::Albums => 1,
            Section::Folders => 2,
        };
        if let Some(direction) = direction {
            let next = current + direction;
            if (0..=2).contains(&next) {
                let mut preview = std::mem::take(&mut self.pager.preview);
                if preview.section != Some(section(next)) {
                    preview = Preview::default();
                }
                preview.exchange(self);
                preview.section = Some(self.section);
                self.pager.preview = preview;
                self.section = section(next);
                // Preserve both page positions at release, rather than jumping
                // to an unrelated fixed offset when the selected tab changes.
                self.pager.offset += direction as f32 * width;
                self.pager.adjacent = Some(-direction);
            }
        } else if self.pager.horizontal {
            let relative = if self.pager.offset < 0.0 { 1 } else { -1 };
            self.pager.adjacent = (0..=2).contains(&(current + relative)).then_some(relative);
            if self.pager.adjacent.is_none() {
                self.pager.offset = self.pager.delta.x * 0.15;
            }
        }
        if self.pager.offset == 0.0 {
            self.pager.adjacent = None;
        }
        if self.pager.offset != 0.0 {
            ctx.request_repaint();
        }
    }
}

fn section(index: i32) -> Section {
    match index {
        0 => Section::Collection,
        1 => Section::Albums,
        _ => Section::Folders,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn touch(phase: egui::TouchPhase, x: f32, y: f32) -> egui::Event {
        egui::Event::Touch {
            device_id: egui::TouchDeviceId(1),
            id: egui::TouchId(1),
            phase,
            pos: egui::pos2(x, y),
            force: None,
        }
    }
    #[test]
    fn horizontal_vertical_cancel_and_toolbar() {
        use egui::TouchPhase::*;
        let mut p = Pager::default();
        assert_eq!(
            p.update(
                &[touch(Start, 300.0, 300.0), touch(Move, 160.0, 305.0)],
                128.0,
                500.0,
                true,
                0.016
            ),
            None
        );
        assert_eq!(p.offset, -140.0);
        assert_eq!(
            p.update(&[touch(End, 160.0, 305.0)], 128.0, 500.0, true, 0.016),
            Some(1)
        );
        assert_eq!(p.offset, -140.0, "release must preserve page position");
        assert_eq!(
            p.update(
                &[
                    touch(Start, 300.0, 300.0),
                    touch(Move, 290.0, 500.0),
                    touch(End, 100.0, 505.0)
                ],
                128.0,
                500.0,
                true,
                0.016
            ),
            None
        );
        assert_eq!(
            p.update(
                &[
                    touch(Start, 300.0, 50.0),
                    touch(Move, 100.0, 50.0),
                    touch(End, 100.0, 50.0)
                ],
                128.0,
                500.0,
                true,
                0.016
            ),
            None
        );
        assert_eq!(
            p.update(
                &[touch(Start, 300.0, 300.0), touch(Move, 100.0, 300.0)],
                128.0,
                500.0,
                false,
                0.016
            ),
            None
        );
    }

    #[test]
    fn short_drag_cancel_and_modal_do_not_commit() {
        use egui::TouchPhase::*;
        let mut pager = Pager::default();
        pager.update(
            &[touch(Start, 300.0, 300.0), touch(Move, 250.0, 302.0)],
            128.0,
            500.0,
            true,
            1.0 / 60.0,
        );
        assert_eq!(pager.offset, -50.0);
        assert_eq!(
            pager.update(&[touch(End, 250.0, 302.0)], 128.0, 500.0, true, 1.0 / 60.0),
            None
        );
        for _ in 0..60 {
            pager.update(&[], 128.0, 500.0, true, 1.0 / 60.0);
        }
        assert_eq!(pager.offset, 0.0);
        assert!(!pager.consumed);
        pager.update(
            &[touch(Start, 300.0, 300.0), touch(Move, 100.0, 302.0)],
            128.0,
            500.0,
            true,
            1.0 / 60.0,
        );
        assert_eq!(
            pager.update(
                &[touch(Cancel, 100.0, 302.0)],
                128.0,
                500.0,
                true,
                1.0 / 60.0
            ),
            None
        );
        pager.update(&[], 128.0, 500.0, false, 1.0 / 60.0);
        assert_eq!(pager.offset, 0.0);
        assert!(!pager.consumed);
        assert!(pager.adjacent.is_none());
    }
}
