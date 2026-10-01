//! "What's on my screen?": captures the window the user was in (or the
//! primary screen) as a PNG for the AI. Only runs when the user asks.

/// Captures the window owned by `pid`, or the primary screen when there is
/// no such window.
#[cfg(windows)]
pub fn capture(pid: Option<u32>) -> Result<Vec<u8>, String> {
    use xcap::image::ImageFormat;
    let window = pid.and_then(|pid| {
        xcap::Window::all().ok()?.into_iter().find(|w| {
            w.pid().ok() == Some(pid)
                && !w.is_minimized().unwrap_or(true)
                && !w.title().unwrap_or_default().is_empty()
        })
    });
    let image = match window {
        Some(w) => w.capture_image().map_err(|e| e.to_string())?,
        None => {
            let monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
            let monitor = monitors
                .iter()
                .find(|m| m.is_primary().unwrap_or(false))
                .or(monitors.first())
                .ok_or("no screen found")?;
            monitor.capture_image().map_err(|e| e.to_string())?
        }
    };
    let mut png = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut png, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(png.into_inner())
}

/// Development fallback: the whole screen through a common Linux tool.
#[cfg(not(windows))]
pub fn capture(_pid: Option<u32>) -> Result<Vec<u8>, String> {
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
            return png;
        }
    }
    Err("no screenshot tool found (install grim, scrot or ImageMagick)".into())
}
