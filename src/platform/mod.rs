#[cfg(target_os = "macos")]
pub mod macos;
pub(crate) mod printing;
pub(crate) mod jobs;
pub(crate) mod backdrop;
pub(crate) mod clipboard;
mod files;
pub(crate) mod edits;
pub(crate) mod resize;
mod recovery;
pub(crate) mod color;
pub(crate) mod raw_decode;
pub(crate) mod sharing;
