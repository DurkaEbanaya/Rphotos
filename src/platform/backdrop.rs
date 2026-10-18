//! Real compositor blur on KDE Wayland; other backends stay opaque.
//! Only the header is translucent. Never samples/captures other applications.
use eframe::egui;

pub(crate) fn active(ctx: &egui::Context) -> bool {
    ctx.data(|d| {
        d.get_temp::<bool>(egui::Id::new("rphotos.backdrop"))
            .unwrap_or(false)
    }) && ctx.data(|d| {
        d.get_temp::<bool>(egui::Id::new("rphotos.blur.capability"))
            .unwrap_or(true)
    }) && ctx.input(|i| i.viewport().focused.unwrap_or(true))
}

#[derive(Default)]
pub(crate) struct Backdrop {
    #[cfg(target_os = "linux")]
    wayland: Option<linux::Wayland>,
}

impl Backdrop {
    pub(crate) fn new(cc: &eframe::CreationContext<'_>) -> Self {
        #[cfg(target_os = "linux")]
        {
            Self {
                wayland: linux::Wayland::new(cc)
                    .map_err(|e| log::info!("Opaque backdrop fallback: {e}"))
                    .ok(),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = cc;
            Self::default()
        }
    }
    pub(crate) fn update(&mut self, ctx: &egui::Context, library: bool) {
        let supported = {
            #[cfg(target_os = "linux")]
            {
                if let Some(w) = &mut self.wayland {
                    w.update(ctx, library);
                    true
                } else {
                    false
                }
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = library;
                false
            }
        };
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("rphotos.backdrop"), supported));
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use anyhow::{Context, Result};
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
    use wayland_client::{
        delegate_noop,
        protocol::{wl_compositor, wl_region, wl_registry, wl_surface},
        Connection, Dispatch, Proxy, QueueHandle,
    };
    use wayland_protocols::ext::background_effect::v1::client::{
        ext_background_effect_manager_v1 as ext_manager,
        ext_background_effect_surface_v1 as ext_surface,
    };
    use wayland_protocols_plasma::blur::client::{org_kde_kwin_blur, org_kde_kwin_blur_manager};

    #[derive(Default)]
    struct Globals {
        compositor: Option<wl_compositor::WlCompositor>,
        manager: Option<org_kde_kwin_blur_manager::OrgKdeKwinBlurManager>,
        ext: Option<ext_manager::ExtBackgroundEffectManagerV1>,
        can_blur: bool,
    }
    impl Dispatch<wl_registry::WlRegistry, ()> for Globals {
        fn event(
            state: &mut Self,
            registry: &wl_registry::WlRegistry,
            event: wl_registry::Event,
            _: &(),
            _: &Connection,
            qh: &QueueHandle<Self>,
        ) {
            if let wl_registry::Event::Global {
                name, interface, ..
            } = event
            {
                match interface.as_str() {
                    "wl_compositor" => state.compositor = Some(registry.bind(name, 1, qh, ())),
                    "org_kde_kwin_blur_manager" => {
                        state.manager = Some(registry.bind(name, 1, qh, ()))
                    }
                    "ext_background_effect_manager_v1" => {
                        state.ext = Some(registry.bind(name, 1, qh, ()))
                    }
                    _ => {}
                }
            }
        }
    }
    delegate_noop!(Globals:ignore wl_compositor::WlCompositor);
    delegate_noop!(Globals:ignore wl_region::WlRegion);
    delegate_noop!(Globals:ignore org_kde_kwin_blur_manager::OrgKdeKwinBlurManager);
    delegate_noop!(Globals:ignore org_kde_kwin_blur::OrgKdeKwinBlur);
    delegate_noop!(Globals:ignore ext_surface::ExtBackgroundEffectSurfaceV1);
    impl Dispatch<ext_manager::ExtBackgroundEffectManagerV1, ()> for Globals {
        fn event(
            state: &mut Self,
            _: &ext_manager::ExtBackgroundEffectManagerV1,
            event: ext_manager::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if let ext_manager::Event::Capabilities { flags } = event {
                state.can_blur = matches!(flags,wayland_client::WEnum::Value(f) if f.contains(ext_manager::Capability::Blur));
            }
        }
    }
    enum Blur {
        Ext(ext_surface::ExtBackgroundEffectSurfaceV1),
        Kde(org_kde_kwin_blur::OrgKdeKwinBlur),
    }

    pub(super) struct Wayland {
        blur: Blur,
        compositor: wl_compositor::WlCompositor,
        conn: Connection,
        // Keep the guest event queue alive for objects we created. Never dispatch
        // or take ownership of the window's surface/event queue.
        queue: wayland_client::EventQueue<Globals>,
        globals: Globals,
        last: (i32, i32),
    }
    impl Wayland {
        pub fn new(cc: &eframe::CreationContext<'_>) -> Result<Self> {
            let (RawDisplayHandle::Wayland(display), RawWindowHandle::Wayland(window)) =
                (cc.display_handle()?.as_raw(), cc.window_handle()?.as_raw())
            else {
                anyhow::bail!("not a Wayland window");
            };
            // SAFETY: eframe's main window/display outlive its App. Guest mode
            // never disconnects the foreign display. We only borrow the surface;
            // it is never destroyed or assigned a different event handler here.
            let backend = unsafe {
                wayland_backend::sys::client::Backend::from_foreign_display(
                    display.display.as_ptr().cast(),
                )
            };
            let conn = Connection::from_backend(backend);
            let mut queue = conn.new_event_queue();
            let qh = queue.handle();
            let registry = conn.display().get_registry(&qh, ());
            let mut globals = Globals::default();
            queue.roundtrip(&mut globals)?;
            queue.roundtrip(&mut globals)?;
            let compositor = globals.compositor.clone().context("no wl_compositor")?;
            // SAFETY: wl_surface handle comes from this live eframe window.
            let id = unsafe {
                wayland_backend::sys::client::ObjectId::from_ptr(
                    wl_surface::WlSurface::interface(),
                    window.surface.as_ptr().cast(),
                )
            }?;
            let surface = wl_surface::WlSurface::from_id(&conn, id)?;
            let blur = if globals.can_blur {
                Blur::Ext(
                    globals
                        .ext
                        .as_ref()
                        .context("no background effect manager")?
                        .get_background_effect(&surface, &qh, ()),
                )
            } else {
                Blur::Kde(
                    globals
                        .manager
                        .as_ref()
                        .context("compositor does not advertise a supported blur protocol")?
                        .create(&surface, &qh, ()),
                )
            };
            drop(registry);
            log::info!(
                "Wayland header blur connected ({})",
                if matches!(blur, Blur::Ext(_)) {
                    "ext-background-effect-v1"
                } else {
                    "KDE"
                }
            );
            Ok(Self {
                blur,
                compositor,
                conn,
                queue,
                globals,
                last: (-1, -1),
            })
        }
        pub fn update(&mut self, ctx: &egui::Context, library: bool) {
            let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            let scale = ctx.zoom_factor();
            let width = (ctx.screen_rect().width() * scale).ceil() as i32;
            let height = if fullscreen {
                0
            } else {
                ((if library { 80.0 } else { 32.0 }) * scale).ceil() as i32
            };
            if self.last != (width, height) {
                let region = self.compositor.create_region(&self.queue.handle(), ());
                if height > 0 {
                    region.add(0, 0, width, height);
                }
                match &self.blur {
                    Blur::Ext(b) => b.set_blur_region(Some(&region)),
                    Blur::Kde(b) => {
                        b.set_region(Some(&region));
                        b.commit();
                    }
                }
                region.destroy();
                let _ = self.conn.flush();
                self.last = (width, height);
            }
            let _ = self.queue.dispatch_pending(&mut self.globals);
            ctx.data_mut(|d| {
                d.insert_temp(
                    egui::Id::new("rphotos.blur.capability"),
                    !matches!(self.blur, Blur::Ext(_)) || self.globals.can_blur,
                )
            });
        }
    }
    impl Drop for Wayland {
        fn drop(&mut self) {
            match &self.blur {
                Blur::Ext(b) => b.destroy(),
                Blur::Kde(b) => b.release(),
            };
            if let Some(m) = &self.globals.ext {
                m.destroy();
            }
            let _ = self.conn.flush();
        }
    }
}
