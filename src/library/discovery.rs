//! Collection recommendations use real albums and indexed text, never invented AI results.
use super::{store::Snapshot, ui::Tokens, Language, Library, Section};
use eframe::egui::{self, pos2, vec2, Color32, Rect, Sense};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Discovery {
    pub hidden: bool,
    pub search_open: bool,
    terms: Vec<String>,
}

impl Discovery {
    pub fn rebuild(&mut self, snapshot: &Snapshot) {
        self.terms = snapshot.suggestions.clone();
    }
}

pub(super) fn suggestions(snapshot: &Snapshot) -> Vec<String> {
    let mut counts = BTreeMap::<String, usize>::new();
    // A bounded vocabulary, built on snapshot changes, not each UI frame.
    for photo in &snapshot.photos {
        for word in photo.text.split(|c: char| !c.is_alphanumeric()).take(256) {
            let word = word.to_lowercase();
            if word.chars().count() >= 4
                && word.chars().count() <= 32
                && (counts.len() < 4096 || counts.contains_key(&word))
            {
                *counts.entry(word).or_default() += 1;
            }
        }
    }
    let mut terms: Vec<_> = counts.into_iter().collect();
    terms.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    terms.into_iter().take(24).map(|(word, _)| word).collect()
}

impl Library {
    pub(super) fn discovery_height(&self) -> f32 {
        if self.section != Section::Collection
            || self.album.is_some()
            || self.selecting
            || self.semantic_zoom
            || !self.query.trim().is_empty()
            || self.state.albums.is_empty()
        {
            0.0
        } else if self.discovery.hidden {
            38.0
        } else {
            212.0
        }
    }
    pub(super) fn search_suggestions(&mut self, ctx: &egui::Context, search: Rect, lang: Language) {
        let needle = self.query.trim().to_lowercase();
        let saved: Vec<_> = self
            .state
            .searches
            .iter()
            .filter(|s| needle.is_empty() || s.name.to_lowercase().contains(&needle))
            .take(4)
            .map(|s| (s.name.clone(), s.query.clone()))
            .collect();
        let terms: Vec<_> = self
            .discovery
            .terms
            .iter()
            .filter(|word| needle.is_empty() || word.contains(&needle))
            .take(6)
            .cloned()
            .collect();
        if saved.is_empty() && terms.is_empty() {
            return;
        }
        let mut chosen = None;
        let output = egui::Area::new(egui::Id::new("search_suggestions"))
            .order(egui::Order::Foreground)
            .fixed_pos(search.left_bottom())
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .corner_radius(0.0)
                    .show(ui, |ui| {
                        ui.set_width((search.width() - 16.0).max(80.0));
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if !saved.is_empty() {
                            ui.small(lang.text("СОХРАНЁННЫЕ ПОИСКИ", "SAVED SEARCHES"));
                            for (name, query) in saved {
                                if ui
                                    .add_sized(
                                        [ui.available_width(), 32.0],
                                        egui::Button::new(name).frame(false),
                                    )
                                    .clicked()
                                {
                                    chosen = Some(query);
                                }
                            }
                        }
                        if !terms.is_empty() {
                            ui.small(lang.text("ТЕКСТ И ОПИСАНИЯ", "TEXT AND DESCRIPTIONS"));
                            for term in terms {
                                if ui
                                    .add_sized(
                                        [ui.available_width(), 32.0],
                                        egui::Button::new(&term).frame(false),
                                    )
                                    .clicked()
                                {
                                    chosen = Some(term);
                                }
                            }
                        }
                    });
            });
        crate::reveal::ignore(&output.response);
        // Prevent the same click reaching a thumbnail under this foreground area.
        self.popup_open_last_frame = true;
        if let Some(query) = chosen {
            self.discovery.search_open = false;
            self.query = query;
            self.section = Section::Collection;
            self.album = None;
            self.folder = None;
            ctx.memory_mut(|m| m.surrender_focus(egui::Id::new("library_search")));
        }
        if ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| !search.contains(p) && !output.response.rect.contains(p))
        }) {
            self.discovery.search_open = false;
            ctx.memory_mut(|m| m.surrender_focus(egui::Id::new("library_search")));
        }
    }

    pub(super) fn discovery_cards(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        t: &Tokens,
        margin: f32,
    ) {
        if self.discovery_height() == 0.0 {
            return;
        }
        ui.horizontal(|ui| {
            ui.add_space(margin);
            let label = if self.discovery.hidden {
                lang.text("Показать рекомендации", "Show recommendations")
            } else {
                lang.text("Скрыть рекомендации", "Hide recommendations")
            };
            if ui
                .add_sized([200.0, 32.0], egui::Button::new(label).frame(false))
                .clicked()
            {
                self.discovery.hidden = !self.discovery.hidden;
            }
        });
        if self.discovery.hidden {
            return;
        }
        let width = (ui.available_width() - margin * 2.0 - 12.0).max(1.0);
        let count = ((width + 12.0) / 255.0).floor().max(1.0) as usize;
        let albums: Vec<_> = self
            .state
            .albums
            .iter()
            .take(count.min(5))
            .cloned()
            .collect();
        let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), 174.0), Sense::hover());
        for (n, album) in albums.into_iter().enumerate() {
            let r = Rect::from_min_size(
                slot.min + vec2(margin + n as f32 * 255.0, 0.0),
                vec2(243.0_f32.min(width), 162.0),
            );
            let response = ui.interact(
                r,
                ui.id().with(("recommendation", album.id)),
                Sense::click(),
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    ui.is_enabled(),
                    format!(
                        "{}: {}",
                        lang.text("Рекомендация", "Recommendation"),
                        album.name
                    ),
                )
            });
            ui.painter().rect_filled(r, 0.0, t.tile);
            let cover = self
                .state
                .photos
                .iter()
                .find(|p| Some(p.id) == album.cover && p.albums.contains(&album.id))
                .or_else(|| {
                    self.state
                        .photos
                        .iter()
                        .find(|p| p.albums.contains(&album.id))
                })
                .cloned();
            if let Some(photo) = cover {
                if let Some(texture) = self.thumbnails.get(&photo) {
                    let image = Rect::from_min_max(r.min, r.max - vec2(0.0, 42.0));
                    let size = texture.size_vec2();
                    let scale = (image.width() / size.x).max(image.height() / size.y);
                    ui.painter().image(
                        texture.id(),
                        image,
                        Rect::from_center_size(pos2(0.5, 0.5), image.size() / (size * scale)),
                        Color32::WHITE,
                    );
                }
            }
            ui.painter().with_clip_rect(r.shrink(10.0)).text(
                r.left_bottom() + vec2(12.0, -21.0),
                egui::Align2::LEFT_CENTER,
                &album.name,
                egui::FontId::proportional(14.0),
                t.text,
            );
            if response.clicked() {
                self.enter_album(album.id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vocabulary_is_derived_from_real_index_and_bounded() {
        let mut snapshot = Snapshot::default();
        snapshot.photos.push(super::super::store::Photo {
            id: 1,
            path: "test.png".into(),
            modified: 0,
            taken: None,
            key: "key".into(),
            albums: vec![],
            text: "Кошка кошка grass GRASS x 123".into(),
        });
        snapshot.suggestions = suggestions(&snapshot);
        let mut d = Discovery::default();
        d.rebuild(&snapshot);
        assert_eq!(d.terms, vec!["grass", "кошка"]);
        d.rebuild(&Snapshot::default());
        assert!(d.terms.is_empty());
    }
}
