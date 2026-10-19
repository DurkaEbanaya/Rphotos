//! Native share choosers. Only explicit user actions provide files to the OS;
//! opening a chooser never chooses a recipient or submits a send operation.
use eframe::egui;
use std::path::PathBuf;

pub(crate) fn install(cc: &eframe::CreationContext<'_>) {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        use raw_window_handle::HasWindowHandle;
        if let Ok(handle) = cc.window_handle() {
            let address = match handle.as_raw() {
                #[cfg(target_os = "windows")]
                raw_window_handle::RawWindowHandle::Win32(h) => h.hwnd.get() as usize,
                #[cfg(target_os = "macos")]
                raw_window_handle::RawWindowHandle::AppKit(h) => h.ns_view.as_ptr() as usize,
                _ => 0,
            };
            cc.egui_ctx
                .data_mut(|d| d.insert_temp(egui::Id::new("native.share.window"), address));
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _ = cc;
}

pub(crate) fn supported() -> bool {
    cfg!(any(target_os = "windows", target_os = "macos"))
}

pub(crate) fn show(ctx: &egui::Context, paths: &[PathBuf]) -> anyhow::Result<()> {
    if paths.is_empty() || paths.len() > 4096 {
        anyhow::bail!("Select between1 and4096 photos to share");
    }
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let address = ctx
            .data(|d| d.get_temp::<usize>(egui::Id::new("native.share.window")))
            .filter(|v| *v != 0)
            .ok_or_else(|| anyhow::anyhow!("No native window for share chooser"))?;
        native::show(address, paths)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = (ctx, paths);
        anyhow::bail!(
            "No standard desktop share chooser on this platform; use file clipboard or email draft"
        )
    }
}

#[cfg(target_os = "windows")]
mod native {
    use super::*;
    use windows::{
        core::{factory, Interface, HSTRING},
        ApplicationModel::DataTransfer::{DataRequestedEventArgs, DataTransferManager},
        Foundation::{Collections::IIterable, TypedEventHandler},
        Storage::{IStorageItem, StorageFile},
        Win32::{Foundation::HWND, UI::Shell::IDataTransferManagerInterop},
    };
    struct Session {
        manager: DataTransferManager,
        token: windows::Foundation::EventRegistrationToken,
    }
    impl Drop for Session {
        fn drop(&mut self) {
            let _ = self.manager.RemoveDataRequested(self.token);
        }
    }
    thread_local! {static SESSION:std::cell::RefCell<Option<Session>>=const{std::cell::RefCell::new(None)};}
    pub fn show(address: usize, paths: &[PathBuf]) -> anyhow::Result<()> {
        let interop: IDataTransferManagerInterop =
            factory::<DataTransferManager, IDataTransferManagerInterop>()?;
        let hwnd = HWND(address as _);
        let manager: DataTransferManager = unsafe { interop.GetForWindow(hwnd) }?;
        let paths = paths.to_vec();
        let handler = TypedEventHandler::<DataTransferManager, DataRequestedEventArgs>::new(
            move |_, args| {
                let args = args.as_ref().ok_or_else(|| {
                    windows::core::Error::from_hresult(windows::core::HRESULT(
                        0x80004003_u32 as i32,
                    ))
                })?;
                let request = args.Request()?;
                let deferral = request.GetDeferral()?;
                let paths = paths.clone();
                std::thread::spawn(move || {
                    let result = (|| -> windows::core::Result<()> {
                        unsafe {
                            windows::Win32::System::WinRT::RoInitialize(
                                windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
                            )
                        }?;
                        let mut items = Vec::new();
                        let result = (|| {
                            for path in &paths {
                                // Blocking StorageFile access stays off the UI/COM callback.
                                let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(
                                    path.as_os_str(),
                                ))?
                                .get()?;
                                items.push(Some(file.cast::<IStorageItem>()?));
                            }
                            let items: IIterable<IStorageItem> = items.try_into()?;
                            let data = request.Data()?;
                            data.Properties()?.SetTitle(&HSTRING::from("Rphotos"))?;
                            data.SetStorageItems(&items, true)
                        })();
                        unsafe { windows::Win32::System::WinRT::RoUninitialize() };
                        result
                    })();
                    if let Err(error) = result {
                        let _ = request.FailWithDisplayText(&HSTRING::from(format!(
                            "Cannot share photos: {error}"
                        )));
                    }
                    let _ = deferral.Complete();
                });
                Ok(())
            },
        );
        let token = manager.DataRequested(&handler)?;
        SESSION.with(|slot| *slot.borrow_mut() = Some(Session { manager, token }));
        unsafe { interop.ShowShareUIForWindow(hwnd) }?;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use objc2::{rc::Retained, ClassType};
    use objc2_app_kit::{NSSharingServicePicker, NSView};
    use objc2_foundation::{NSArray, NSRectEdge, NSString, NSURL};
    thread_local! {static PICKER:std::cell::RefCell<Option<Retained<NSSharingServicePicker>>>=const{std::cell::RefCell::new(None)};}
    pub fn show(address: usize, paths: &[PathBuf]) -> anyhow::Result<()> {
        // Called by egui on the application main thread, with the live NSView.
        unsafe {
            let view = &*(address as *const NSView);
            let urls: Vec<_> = paths
                .iter()
                .map(|p| NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy())))
                .collect();
            // Keep NSURL's concrete immutable mutability for collection retain
            // safety. Neither AnyObject nor NSObject's Root mutability provides
            // the IsRetainable contract required by objc2 0.5 from_slice.
            let refs: Vec<&NSURL> = urls.iter().map(|u| u.as_ref()).collect();
            let items = NSArray::from_slice(&refs);
            // Objective-C lightweight generics are erased at runtime. The picker
            // accepts an untyped NSArray; all retained objects above are NSURLs.
            let untyped = &*(items.as_ref() as *const NSArray<NSURL> as *const NSArray);
            let picker =
                NSSharingServicePicker::initWithItems(NSSharingServicePicker::alloc(), untyped);
            picker.showRelativeToRect_ofView_preferredEdge(view.bounds(), view, NSRectEdge::MaxY);
            PICKER.with(|slot| *slot.borrow_mut() = Some(picker));
        }
        Ok(())
    }
}
