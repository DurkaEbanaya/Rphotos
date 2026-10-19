//! Keyboard navigation must also reach photos outside the virtualized viewport.
use super::{GridRow, Library, OpenPhoto};
use eframe::egui::{self, Key};

impl Library {
    pub(super) fn grid_keyboard(&mut self, ui: &egui::Ui, prefix: f32) -> Option<OpenPhoto> {
        if !ui.is_enabled()
            || self.semantic_zoom
            || self.popup_open_last_frame
            || self.has_dialog()
            || ui.ctx().wants_keyboard_input()
            || self.filtered.is_empty()
        {
            return None;
        }
        if let Some(focus) = ui.memory(|m| m.focused()) {
            if self.focused_photo_widget != Some(focus) {
                return None; // Enter/Space must still operate focused toolbar controls.
            }
        }
        let direction = ui.input(|i| {
            if i.modifiers.command || i.modifiers.alt {
                return None;
            }
            [
                Key::ArrowLeft,
                Key::ArrowRight,
                Key::ArrowUp,
                Key::ArrowDown,
                Key::Home,
                Key::End,
                Key::Enter,
                Key::Space,
            ]
            .into_iter()
            .find(|k| i.key_pressed(*k))
        })?;
        if let Some(id) = self.focused_photo_widget.take() {
            ui.memory_mut(|m| m.surrender_focus(id));
        }
        let current = self
            .focused_photo
            .and_then(|id| {
                self.filtered
                    .iter()
                    .position(|&i| self.state.photos[i].id == id)
            })
            .unwrap_or(0);
        if direction == Key::Enter {
            if self
                .thumbnails
                .unavailable(&self.state.photos[self.filtered[current]])
            {
                return None;
            }
            return Some(self.open(self.filtered[current]));
        }
        if direction == Key::Space {
            self.selecting = true;
            let id = self.state.photos[self.filtered[current]].id;
            if !self.selection.insert(id) {
                self.selection.remove(&id);
            }
            self.focused_photo = Some(id);
            return None;
        }
        let target = match direction {
            Key::Home => 0,
            Key::End => self.filtered.len() - 1,
            Key::ArrowLeft => current.saturating_sub(1),
            Key::ArrowRight => (current + 1).min(self.filtered.len() - 1),
            Key::ArrowUp | Key::ArrowDown => self
                .vertical_neighbor(self.filtered[current], direction == Key::ArrowDown)
                .and_then(|index| self.filtered.iter().position(|&i| i == index))
                .unwrap_or(current),
            _ => current,
        };
        let index = self.filtered[target];
        self.focused_photo = Some(self.state.photos[index].id);
        if ui.input(|i| i.modifiers.shift) {
            self.selecting = true;
            for &i in &self.filtered[current.min(target)..=current.max(target)] {
                self.selection.insert(self.state.photos[i].id);
            }
        }
        if let Some((r, row)) = self.rows.iter().enumerate().find_map(|(n, r)| match r {
            GridRow::Photos(row) if row.cells.iter().any(|c| c.index == index) => Some((n, row)),
            _ => None,
        }) {
            let top = self.row_offsets[r] + prefix;
            let bottom = top + row.height;
            let height = ui.available_height().max(1.0);
            if top < self.scroll_offset {
                self.scroll_offset = top;
            } else if bottom > self.scroll_offset + height {
                self.scroll_offset = (bottom - height).max(top);
            }
            self.scroll_anchor = None;
        }
        ui.ctx().request_repaint();
        None
    }

    fn vertical_neighbor(&self, index: usize, down: bool) -> Option<usize> {
        let (row, x) = self.rows.iter().enumerate().find_map(|(n, r)| match r {
            GridRow::Photos(row) => row
                .cells
                .iter()
                .find(|c| c.index == index)
                .map(|c| (n, c.x + c.width / 2.0)),
            _ => None,
        })?;
        let next = if down {
            self.rows.iter().skip(row + 1).find_map(|r| match r {
                GridRow::Photos(row) => Some(row),
                _ => None,
            })
        } else {
            self.rows[..row].iter().rev().find_map(|r| match r {
                GridRow::Photos(row) => Some(row),
                _ => None,
            })
        }?;
        next.cells
            .iter()
            .min_by(|a, b| {
                ((a.x + a.width / 2.0) - x)
                    .abs()
                    .total_cmp(&((b.x + b.width / 2.0) - x).abs())
            })
            .map(|c| c.index)
    }
}
