//! Culling actions: moving the current image to the trash.
//!
//! The trash move itself lives in `crate::trash_bin`; this module decides
//! which panes take part, keeps every pane that lists the file in step,
//! and shows the outcome to the user.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::trash_bin;

use super::App;

/// How long the outcome stays on screen.
const TOAST_DURATION: Duration = Duration::from_millis(2500);

pub(super) struct TrashJob {
    results: std::sync::mpsc::Receiver<(PathBuf,Result<(),String>)>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    total:usize,
    done:usize,
    errors:Vec<String>,
}

/// A short message painted over the image area, e.g. "Moved to Trash".
pub(crate) struct Toast {
    text: String,
    shown_at: Instant,
    is_error: bool,
}

impl App {
    /// Move the current image of every active pane to the trash. In synced
    /// dual-pane mode both panes show the same file, so it is moved once.
    /// On Windows, a location without a Recycle Bin gets a confirmation
    /// first because the shell would delete permanently.
    pub(super) fn trash_current_images(&mut self, ctx: &egui::Context) {
        let use_selection = self.is_independent();
        let mut paths: Vec<PathBuf> = Vec::new();
        for pane in &self.panes {
            if use_selection && !pane.selected {
                continue;
            }
            if let Some(path) = pane.image_paths.get(pane.current_index) {
                if !paths.contains(path) {
                    paths.push(path.clone());
                }
            }
        }
        self.trash_or_confirm(paths, ctx);
    }

    /// The footer button of one pane: move that pane's current image to
    /// the trash, whatever the pane selection is.
    pub(super) fn trash_pane_image(&mut self, pane_idx: usize, ctx: &egui::Context) {
        let Some(path) = self
            .panes
            .get(pane_idx)
            .and_then(|p| p.image_paths.get(p.current_index).cloned())
        else {
            return;
        };
        self.trash_or_confirm(vec![path], ctx);
    }

    pub(super) fn trash_or_confirm(&mut self, paths: Vec<PathBuf>, ctx: &egui::Context) {
        if paths.is_empty() {
            return;
        }
        if paths.iter().any(|p| trash_bin::lacks_recycle_bin(p)) {
            self.pending_permanent_delete = Some(paths);
            return;
        }
        self.trash_paths(paths, ctx);
    }

    /// Move `paths` to the trash and drop each one that succeeded from every
    /// pane that lists it, whichever pane triggered the action. The pane
    /// showing the file does the move through `Pane::remove_current`, so a
    /// failed move leaves it untouched; the other panes follow by position.
    pub(super) fn trash_paths(&mut self, paths: Vec<PathBuf>, ctx: &egui::Context) {
        if self.trash_job.is_some(){return;}
        let (tx,rx)=std::sync::mpsc::sync_channel(16);
        let cancel=std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.trash_job=Some(TrashJob{results:rx,cancel:cancel.clone(),total:paths.len(),done:0,errors:Vec::new()});
        let wake=ctx.clone();
        std::thread::spawn(move||{
            for path in paths {
                if cancel.load(std::sync::atomic::Ordering::Relaxed){break;}
                let result=trash_bin::move_to_trash(&path).map_err(|e|e.to_string());
                if tx.send((path,result)).is_err(){break;}wake.request_repaint();
            }
            wake.request_repaint();
        });
        ctx.request_repaint();
    }

    fn poll_trash(&mut self,ctx:&egui::Context) {
        let mut changed=false;let mut finished=false;
        if let Some(job)=&mut self.trash_job {
            for _ in 0..32 {
                match job.results.try_recv() {
                    Ok((path,result))=>{
                        job.done+=1;
                        if let Err(error)=result {job.errors.push(format!("{}: {error}",path.display()));}
                        else {changed=true;for pane in &mut self.panes {if let Some(i)=pane.image_paths.iter().position(|p|*p==path){pane.remove_index(i,ctx);}}}
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected)=>{finished=true;break;}
                    Err(_)=>break,
                }
            }
        }
        if changed {self.library.refresh();}
        if finished {
            let job=self.trash_job.take().unwrap();
            let text=if job.errors.is_empty(){format!("{} / {} — {}",job.done,job.total,self.settings.language.text("обработано","processed"))}else{job.errors.join("\n")};
            self.show_toast(text,!job.errors.is_empty());
            self.trash_errors=job.errors;
        }
    }

    pub(super) fn show_toast(&mut self, text: String, is_error: bool) {
        self.toast = Some(Toast {
            text,
            shown_at: Instant::now(),
            is_error,
        });
    }

    /// Paint the toast at the bottom center of the window until it expires.
    pub(super) fn paint_toast(&mut self, ctx: &egui::Context) {
        if let Some(error)=crate::settings::take_error(){self.settings_error=Some(error);}
        if let Some(error)=self.settings_error.clone() {
            let response=egui::Modal::new(egui::Id::new("settings_write_error")).show(ctx,|ui|{
                ui.set_max_width(520.0);ui.heading(self.settings.language.text("Ошибка настроек","Settings error"));ui.label(error);
                ui.button(self.settings.language.text("Закрыть","Close")).clicked()
            });if response.inner||response.should_close(){self.settings_error=None;}
        }
        self.poll_trash(ctx);
        if !self.trash_errors.is_empty() {
            let response=egui::Modal::new(egui::Id::new("trash_results")).show(ctx,|ui|{
                ui.set_max_width(760.0);ui.heading(self.settings.language.text("Не удалось переместить в корзину","Could not move to Trash"));
                egui::ScrollArea::vertical().max_height(400.0).show(ui,|ui|{for error in &self.trash_errors{ui.label(error);}});
                ui.button(self.settings.language.text("Закрыть","Close")).clicked()
            });if response.inner||response.should_close(){self.trash_errors.clear();}
        }
        if let Some(job)=&self.trash_job {
            egui::Area::new(egui::Id::new("trash_progress")).anchor(egui::Align2::CENTER_BOTTOM,[0.0,-72.0]).show(ctx,|ui|{
                egui::Frame::popup(ui.style()).show(ui,|ui|{
                    ui.label(format!("{}: {} / {}",self.settings.language.text("Корзина","Trash"),job.done,job.total));
                    if ui.button(self.settings.language.text("Остановить очередь","Stop queued files")).clicked(){job.cancel.store(true,std::sync::atomic::Ordering::Relaxed);}
                });
            });ctx.request_repaint_after(Duration::from_millis(50));
        }
        let Some(toast) = &self.toast else {
            return;
        };
        let elapsed = toast.shown_at.elapsed();
        if elapsed >= TOAST_DURATION {
            self.toast = None;
            return;
        }
        let fade_start = TOAST_DURATION.as_secs_f32() - 0.4;
        let alpha = ((TOAST_DURATION.as_secs_f32() - elapsed.as_secs_f32()) / 0.4).clamp(0.0, 1.0);
        let alpha = if elapsed.as_secs_f32() < fade_start { 1.0 } else { alpha };

        let text_color = egui::Color32::from_rgba_unmultiplied(230, 230, 230, (alpha * 255.0) as u8);
        let bg = if toast.is_error {
            egui::Color32::from_rgba_unmultiplied(120, 40, 40, (alpha * 220.0) as u8)
        } else {
            egui::Color32::from_rgba_unmultiplied(20, 20, 20, (alpha * 200.0) as u8)
        };
        let text = toast.text.clone();

        egui::Area::new(egui::Id::new("culling_toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -72.0])
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::default()
                    .fill(bg)
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(text).size(14.0).color(text_color));
                    });
            });
        ctx.request_repaint_after(Duration::from_millis(50));
    }

    /// Confirmation shown when the trash move would be a permanent delete
    /// (Windows network shares and removable media). Enter confirms,
    /// Escape cancels.
    pub(super) fn show_permanent_delete_modal(&mut self, ctx: &egui::Context) {
        let Some(paths) = self.pending_permanent_delete.clone() else {
            return;
        };
        let (backdrop, card_bg, card_stroke) =
            (self.theme.backdrop, self.theme.card_bg, self.theme.card_stroke);
        let screen = ctx.screen_rect();
        let mut decision: Option<bool> = None;

        egui::Area::new(egui::Id::new("permanent_delete_backdrop"))
            .fixed_pos(screen.min)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let response = ui.allocate_response(screen.size(), egui::Sense::click());
                ui.painter().rect_filled(screen, 0.0, backdrop);
                if response.clicked() {
                    decision = Some(false);
                }
            });

        egui::Area::new(egui::Id::new("permanent_delete_modal"))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                let max_width = (screen.width() * 0.8).min(460.0);
                egui::Frame::default()
                    .fill(card_bg)
                    .stroke(egui::Stroke::new(1.0_f32, card_stroke))
                    .corner_radius(8.0)
                    .inner_margin(20.0)
                    .show(ui, |ui| {
                        ui.set_max_width(max_width);
                        ui.label(egui::RichText::new("No Recycle Bin here").size(18.0).strong());
                        ui.add_space(10.0);
                        ui.label(
                            "This location has no Recycle Bin, so Windows will delete the \
                             file permanently instead of moving it to the trash.",
                        );
                        ui.add_space(8.0);
                        for path in &paths {
                            ui.label(egui::RichText::new(file_name(path)).monospace());
                        }
                        ui.add_space(16.0);
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                decision = Some(false);
                            }
                            ui.add_space(8.0);
                            let delete = egui::Button::new(
                                egui::RichText::new("Delete permanently").color(egui::Color32::WHITE),
                            )
                            .fill(egui::Color32::from_rgb(160, 50, 50));
                            if ui.add(delete).clicked() {
                                decision = Some(true);
                            }
                        });
                    });
            });

        let (enter, escape) = ctx.input(|i| {
            (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape))
        });
        if enter {
            decision = Some(true);
        } else if escape {
            decision = Some(false);
        }

        match decision {
            Some(true) => {
                self.pending_permanent_delete = None;
                self.trash_paths(paths, ctx);
            }
            Some(false) => self.pending_permanent_delete = None,
            None => {}
        }
    }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
