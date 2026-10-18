//! One-finger section pager: vertical motion stays with ScrollArea, multi-touch
//! cancels the pager. Pointer/mouse events alone never trigger section changes.
use super::{Library, Section};
use eframe::egui::{self, Pos2, Vec2};

#[derive(Default)]
pub(super) struct Pager {
    start: Option<Pos2>,
    delta: Vec2,
    horizontal: bool,
    pub offset: f32,
    pub consumed: bool,
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
        if !self.horizontal {
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
        if let Some(direction) = direction {
            let current = match self.section {
                Section::Collection => 0,
                Section::Albums => 1,
                Section::Folders => 2,
            };
            self.section = match (current + direction).clamp(0, 2) {
                0 => Section::Collection,
                1 => Section::Albums,
                _ => Section::Folders,
            };
            self.scroll_anchor = None;
            self.scroll_offset = 0.0;
            self.pager.offset = -(direction as f32) * width * 0.12;
        }
        if self.pager.offset != 0.0 {
            ctx.request_repaint();
        }
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
}
