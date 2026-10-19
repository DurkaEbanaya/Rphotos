//! Single in-flight OS integration job. No shell interpolation, no silent sends.
use crate::library::Language;
use eframe::egui;
use std::{path::PathBuf, sync::mpsc};

pub(crate) enum Job {
    SaveCopy(PathBuf),
    OpenWith(PathBuf),
    Reveal(PathBuf),
    Copy(Vec<PathBuf>),
    Print(Vec<PathBuf>),
    Export(Vec<PathBuf>),
    Email(Vec<PathBuf>),
}

#[derive(Default)]
pub(crate) struct Jobs {
    result: Option<mpsc::Receiver<Result<(), String>>>,
    pub message: Option<String>,
}

impl Jobs {
    pub fn share(&mut self, ctx: &egui::Context, lang: Language, paths: &[PathBuf]) -> bool {
        let response=egui::Modal::new(egui::Id::new("library_share")).show(ctx,|ui|{
            ui.set_max_width(360.0);ui.heading(lang.text("Поделиться","Share"));
            ui.label(lang.text("Действия Rphotos. Это не системный Share-флайаут; файлы не отправляются автоматически.","Rphotos actions, not the system Share flyout. Nothing is sent automatically."));
            let email=ui.add_enabled(cfg!(target_os="linux")&&!self.busy(),egui::Button::new(lang.text("Черновик письма с вложениями","Email attachment draft"))).clicked();
            let export=ui.add_enabled(!self.busy(),egui::Button::new(lang.text("Сохранить PDF…","Save PDF…"))).clicked();
            let copy=ui.add_enabled(!self.busy(),egui::Button::new(lang.text("Копировать файлы","Copy files"))).clicked();
            let cancel=ui.button(lang.text("Отмена","Cancel")).clicked();(email,export,copy,cancel)
        });
        if response.inner.0 {
            self.start(Job::Email(paths.to_vec()), ctx);
        }
        if response.inner.1 {
            self.start(Job::Export(paths.to_vec()), ctx);
        }
        if response.inner.2 {
            self.start(Job::Copy(paths.to_vec()), ctx);
        }
        response.inner.0
            || response.inner.1
            || response.inner.2
            || response.inner.3
            || response.should_close()
    }
    pub fn busy(&self) -> bool {
        self.result.is_some()
    }
    pub fn start(&mut self, job: Job, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let wake = ctx.clone();
        #[cfg(target_os = "macos")]
        let output = match &job {
            Job::Export(_) => Some(
                rfd::FileDialog::new()
                    .add_filter("PDF", &["pdf"])
                    .set_file_name("photos.pdf")
                    .save_file(),
            ),
            Job::SaveCopy(path) => Some(copy_dialog(path)),
            _ => None,
        };
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| match job {
                Job::SaveCopy(path) => {
                    #[cfg(not(target_os = "macos"))]
                    let output = copy_dialog(&path);
                    #[cfg(target_os = "macos")]
                    let output = output.flatten();
                    output.map_or(Ok(()), |destination| {
                        super::files::save_copy(&path, &destination)
                    })
                }
                Job::OpenWith(path) => super::files::open_with(&path),
                Job::Reveal(path) => super::files::reveal(&path),
                Job::Copy(paths) => super::clipboard::copy_files(&paths),
                Job::Print(paths) => super::printing::print(&paths),
                Job::Export(paths) => {
                    #[cfg(not(target_os = "macos"))]
                    let output = rfd::FileDialog::new()
                        .add_filter("PDF", &["pdf"])
                        .set_file_name("photos.pdf")
                        .save_file();
                    #[cfg(target_os = "macos")]
                    let output = output.flatten();
                    if let Some(path) = output {
                        super::printing::export(&paths, &path)
                    } else {
                        Ok(())
                    }
                }
                Job::Email(paths) => email(&paths),
            })
            .unwrap_or_else(|_| Err(anyhow::anyhow!("Integration worker failed")))
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(result);
            wake.request_repaint();
        });
        self.result = Some(rx);
    }
    pub fn poll(&mut self) {
        if let Some(rx) = &self.result {
            match rx.try_recv() {
                Ok(result) => {
                    self.result = None;
                    self.message = result.err();
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.result = None;
                    self.message = Some("Integration worker disconnected".into());
                }
                _ => {}
            }
        }
    }
    pub fn show(&mut self, ctx: &egui::Context, lang: Language) {
        self.poll();
        if let Some(message) = self.message.clone() {
            let response = egui::Modal::new(egui::Id::new("integration_error")).show(ctx, |ui| {
                ui.set_max_width(440.0);
                ui.heading(lang.text("Системная операция", "System operation"));
                ui.label(message);
                ui.button(lang.text("Закрыть", "Close")).clicked()
            });
            if response.inner || response.should_close() {
                self.message = None;
            }
        }
    }
}

fn copy_dialog(path: &std::path::Path) -> Option<PathBuf> {
    let name = path.file_stem().unwrap_or_default().to_string_lossy();
    let extension = path.extension().unwrap_or_default().to_string_lossy();
    let suggested = if extension.is_empty() {
        format!("{name}-copy")
    } else {
        format!("{name}-copy.{extension}")
    };
    rfd::FileDialog::new().set_file_name(suggested).save_file()
}

fn email(paths: &[PathBuf]) -> anyhow::Result<()> {
    if paths.is_empty() {
        anyhow::bail!("No photos selected");
    }
    #[cfg(target_os = "linux")]
    {
        let mut command = std::process::Command::new("xdg-email");
        for path in paths {
            command.arg("--attach").arg(path.canonicalize()?);
        }
        if !command.status()?.success() {
            anyhow::bail!("The mail application could not open an attachment draft");
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        anyhow::bail!("Email attachment drafts are not implemented on this platform yet")
    }
}
