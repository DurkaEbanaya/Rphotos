//! File references, not a text imitation of a file clipboard. Originals stay untouched.
use anyhow::{bail, Context, Result};
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub(crate) fn copy_files(paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() || paths.len() > 4096 {
        bail!("Select between 1 and 4096 files");
    }
    let paths = paths
        .iter()
        .map(|p| p.canonicalize())
        .collect::<std::io::Result<Vec<_>>>()?;
    if paths.iter().any(|p| !p.is_file()) {
        bail!("Only existing files can be copied");
    }
    #[cfg(target_os = "linux")]
    {
        let payload = uri_list(&paths);
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            match run(
                Command::new("wl-copy").args(["--type", "text/uri-list"]),
                &payload,
            ) {
                Ok(()) => return Ok(()),
                Err(error) if std::env::var_os("DISPLAY").is_none() => {
                    return Err(error.context("Install wl-clipboard to copy files on Wayland"))
                }
                Err(_) => {}
            }
        }
        if std::env::var_os("DISPLAY").is_some() {
            return run(
                Command::new("xclip").args([
                    "-selection",
                    "clipboard",
                    "-target",
                    "text/uri-list",
                    "-in",
                ]),
                &payload,
            )
            .context("File clipboard requires xclip (X11) or wl-clipboard (Wayland)");
        }
        bail!("No supported clipboard display; install wl-clipboard on Wayland");
    }
    #[cfg(target_os = "windows")]
    {
        use base64::Engine;
        let entries = paths
            .iter()
            .map(|p| format!("'{}'", p.to_string_lossy().replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(",");
        let script = format!("$ErrorActionPreference='Stop'; Add-Type -AssemblyName System.Windows.Forms; $files=New-Object System.Collections.Specialized.StringCollection; $files.AddRange([string[]]@({entries})); [System.Windows.Forms.Clipboard]::SetFileDropList($files)");
        let encoded = base64::engine::general_purpose::STANDARD.encode(
            script
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        );
        run(
            Command::new("powershell.exe").args([
                "-NoProfile",
                "-NonInteractive",
                "-STA",
                "-EncodedCommand",
                &encoded,
            ]),
            b"",
        )
    }
    #[cfg(target_os = "macos")]
    {
        let entries = paths
            .iter()
            .map(|p| {
                format!(
                    "(POSIX file \"{}\" as alias)",
                    p.to_string_lossy()
                        .replace('\\', "\\\\")
                        .replace('"', "\\\"")
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        run(
            Command::new("osascript").arg("-"),
            format!("set the clipboard to {{{entries}}}\n").as_bytes(),
        )
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        bail!("File clipboard is not supported on this platform")
    }
}

fn run(command: &mut Command, input: &[u8]) -> Result<()> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Cannot start clipboard helper")?;
    if let Err(error) = child
        .stdin
        .take()
        .context("Missing clipboard input")?
        .write_all(input)
    {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error.into());
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                bail!("Clipboard helper failed; clipboard may be unavailable");
            }
            return Ok(());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Clipboard helper timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(target_os = "linux")]
fn uri_list(paths: &[PathBuf]) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    let mut output = String::new();
    for path in paths {
        output.push_str("file://");
        for &byte in path.as_os_str().as_bytes() {
            if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
                output.push(byte as char);
            } else {
                output.push_str(&format!("%{byte:02X}"));
            }
        }
        output.push_str("\r\n");
    }
    output.into_bytes()
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    #[test]
    fn file_uris_escape_delimiters_unicode_and_non_unicode_bytes() {
        use std::os::unix::ffi::OsStringExt;
        let paths = vec![
            PathBuf::from("/photos/a #?\n.png"),
            PathBuf::from("/photos/кот.png"),
            PathBuf::from(std::ffi::OsString::from_vec(b"/photos/\xff.png".to_vec())),
        ];
        let value = String::from_utf8(uri_list(&paths)).unwrap();
        assert_eq!(value,"file:///photos/a%20%23%3F%0A.png\r\nfile:///photos/%D0%BA%D0%BE%D1%82.png\r\nfile:///photos/%FF.png\r\n");
        assert!(copy_files(&[]).is_err());
    }
}
