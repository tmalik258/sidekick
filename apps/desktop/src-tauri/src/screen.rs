//! "What's on my screen?": captures the window the user was in (or the
//! primary screen) as a PNG for the AI. Only runs when the user asks.

/// Captures the window owned by `pid`, or the primary screen when there is
/// no such window.
pub fn capture(pid: Option<u32>) -> Result<Vec<u8>, String> {
    capture_at(pid).map(|(png, _, _)| png)
}

/// Like `capture`, with where the image's top-left corner is on the
/// desktop, so a spot in the image can be clicked.
#[cfg(windows)]
pub fn capture_at(pid: Option<u32>) -> Result<(Vec<u8>, i32, i32), String> {
    use xcap::image::ImageFormat;
    let window = pid.and_then(|pid| {
        xcap::Window::all().ok()?.into_iter().find(|w| {
            w.pid().ok() == Some(pid)
                && !w.is_minimized().unwrap_or(true)
                && !w.title().unwrap_or_default().is_empty()
        })
    });
    let (image, x, y) = match window {
        Some(w) => (
            w.capture_image().map_err(|e| e.to_string())?,
            w.x().unwrap_or(0),
            w.y().unwrap_or(0),
        ),
        None => {
            let monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
            let monitor = monitors
                .iter()
                .find(|m| m.is_primary().unwrap_or(false))
                .or(monitors.first())
                .ok_or("no screen found")?;
            (
                monitor.capture_image().map_err(|e| e.to_string())?,
                monitor.x().unwrap_or(0),
                monitor.y().unwrap_or(0),
            )
        }
    };
    let mut png = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut png, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok((png.into_inner(), x, y))
}

/// A left click at a desktop point (physical pixels, as captured).
#[cfg(windows)]
pub fn click(x: i32, y: i32) -> Result<(), String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, mouse_event,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos;
    // SAFETY: plain Win32 calls with value arguments.
    unsafe {
        if SetCursorPos(x, y) == 0 {
            return Err("the pointer could not move there".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(60));
        mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);
        mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn click(_x: i32, _y: i32) -> Result<(), String> {
    Err("clicking on the screen works on Windows only".into())
}

/// A PNG's width and height, from its header.
pub fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    if png.len() < 24 || &png[1..4] != b"PNG" {
        return None;
    }
    let w = u32::from_be_bytes(png[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(png[20..24].try_into().ok()?);
    Some((w, h))
}

/// Development fallback: the whole screen through a common Linux tool.
#[cfg(not(windows))]
pub fn capture_at(_pid: Option<u32>) -> Result<(Vec<u8>, i32, i32), String> {
    use std::process::Command;
    let out = std::env::temp_dir().join(format!("sidekick-screen-{}.png", std::process::id()));
    let path = out.to_string_lossy().into_owned();
    let tools: [(&str, Vec<&str>); 3] = [
        ("grim", vec![path.as_str()]),
        ("scrot", vec!["-o", path.as_str()]),
        ("import", vec!["-window", "root", path.as_str()]),
    ];
    for (tool, args) in tools {
        let ok = Command::new(tool)
            .args(&args)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            let png = std::fs::read(&out).map_err(|e| e.to_string());
            let _ = std::fs::remove_file(&out);
            return png.map(|p| (p, 0, 0));
        }
    }
    Err("no screenshot tool found (install grim, scrot or ImageMagick)".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_png_size() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(1280u32.to_be_bytes());
        png.extend(720u32.to_be_bytes());
        assert_eq!(super::png_size(&png), Some((1280, 720)));
        assert_eq!(super::png_size(b"nope"), None);
    }
}
