//! File conversion and archive extraction through installed tools
//! (FR-FILE-02, FR-FILE-03). Output never overwrites an existing file.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::process::Command;

use crate::{ActionError, Capabilities, Outcome, file_name};

const TIMEOUT: Duration = Duration::from_secs(600);

pub async fn convert(caps: &Capabilities, input: &Path, to: &str) -> Result<Outcome, ActionError> {
    let to = to.to_ascii_lowercase();
    if !to.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(ActionError::Invalid(format!("bad target format {to}")));
    }
    let dir = input.parent().unwrap_or(Path::new("."));
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let output = unique_path(dir, stem, &to);

    match to.as_str() {
        "pdf" => {
            let soffice = caps
                .soffice
                .as_ref()
                .ok_or_else(|| missing("LibreOffice"))?;
            // LibreOffice picks the output name itself, so convert into a
            // private folder and move the result into place.
            let tmp = dir.join(format!(".sidekick-{}", ulid::Ulid::new()));
            std::fs::create_dir_all(&tmp).map_err(fail)?;
            let result = run(Command::new(soffice)
                .args(["--headless", "--convert-to", "pdf", "--outdir"])
                .arg(&tmp)
                .arg(input))
            .await;
            let produced = tmp.join(format!("{stem}.pdf"));
            let moved = result.and_then(|_| std::fs::rename(&produced, &output).map_err(fail));
            let _ = std::fs::remove_dir_all(&tmp);
            moved?;
        }
        "docx" => {
            let pandoc = caps.pandoc.as_ref().ok_or_else(|| missing("pandoc"))?;
            run(Command::new(pandoc).arg(input).arg("-o").arg(&output)).await?;
        }
        "webp" | "png" | "jpg" | "jpeg" | "avif" if caps.magick.is_some() => {
            let magick = caps.magick.as_ref().ok_or_else(|| missing("ImageMagick"))?;
            run(Command::new(magick).arg(input).arg(&output)).await?;
        }
        "mp3" => {
            let ffmpeg = caps.ffmpeg.as_ref().ok_or_else(|| missing("ffmpeg"))?;
            run(Command::new(ffmpeg)
                .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
                .arg(input)
                .args(["-vn", "-q:a", "2"])
                .arg(&output))
            .await?;
        }
        _ => {
            let ffmpeg = caps.ffmpeg.as_ref().ok_or_else(|| missing("ffmpeg"))?;
            run(Command::new(ffmpeg)
                .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
                .arg(input)
                .arg(&output))
            .await?;
        }
    }

    Ok(Outcome {
        message: format!("Saved {}", file_name(&output)),
        path: Some(output.display().to_string()),
    })
}

pub async fn extract(caps: &Capabilities, archive: &Path) -> Result<Outcome, ActionError> {
    let tar = caps.tar.as_ref().ok_or_else(|| missing("tar"))?;
    let dir = archive.parent().unwrap_or(Path::new("."));
    let name = file_name(archive);
    let stem = name
        .strip_suffix(".tar.gz")
        .or_else(|| name.strip_suffix(".tar.xz"))
        .or_else(|| name.strip_suffix(".tar.bz2"))
        .or_else(|| name.rsplit_once('.').map(|(s, _)| s))
        .unwrap_or(&name)
        .to_string();
    let dest = unique_path(dir, &stem, "");
    std::fs::create_dir_all(&dest).map_err(fail)?;
    // bsdtar (built into Windows 10 and later) reads zip as well as tar.
    let result = run(Command::new(tar)
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(&dest))
    .await;
    if let Err(err) = result {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(err);
    }
    Ok(Outcome {
        message: format!("Extracted to {}", file_name(&dest)),
        path: Some(dest.display().to_string()),
    })
}

/// Zips files or folders that sit in one folder into `name.zip` next to
/// them. bsdtar picks the zip format from the extension.
pub async fn zip(
    caps: &Capabilities,
    paths: &[PathBuf],
    name: &str,
) -> Result<Outcome, ActionError> {
    let tar = caps.tar.as_ref().ok_or_else(|| missing("tar"))?;
    let first = paths.first().ok_or(ActionError::MissingArg("paths"))?;
    let dir = first.parent().unwrap_or(Path::new("."));
    if paths.iter().any(|p| p.parent() != Some(dir)) {
        return Err(ActionError::Invalid(
            "the items must be in one folder".into(),
        ));
    }
    let stem = if name.trim().is_empty() {
        first
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Archive".into())
    } else {
        name.trim().trim_end_matches(".zip").to_owned()
    };
    let dest = unique_path(dir, &stem, "zip");
    let mut cmd = Command::new(tar);
    cmd.arg("-a").arg("-cf").arg(&dest).arg("-C").arg(dir);
    for p in paths {
        cmd.arg(
            p.file_name()
                .ok_or(ActionError::Invalid("not a file".into()))?,
        );
    }
    if let Err(err) = run(&mut cmd).await {
        let _ = std::fs::remove_file(&dest);
        return Err(err);
    }
    Ok(Outcome {
        message: format!("Zipped into {}", file_name(&dest)),
        path: Some(dest.display().to_string()),
    })
}

/// The text in an image, read with Tesseract. Empty when there is none.
pub async fn ocr_text(caps: &Capabilities, image: &Path) -> Result<String, ActionError> {
    let tesseract = caps
        .tesseract
        .as_ref()
        .ok_or_else(|| missing("Tesseract"))?;
    let mut cmd = Command::new(tesseract);
    cmd.arg(image)
        .arg("stdout")
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000);
    }
    let out = tokio::time::timeout(TIMEOUT, cmd.output())
        .await
        .map_err(|_| ActionError::Failed("reading the text took too long".into()))?
        .map_err(fail)?;
    if !out.status.success() {
        return Err(ActionError::Failed(
            "Tesseract could not read the image".into(),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Reads the text in an image with Tesseract and puts it on the clipboard.
pub async fn ocr(caps: &Capabilities, image: &Path) -> Result<Outcome, ActionError> {
    let text = ocr_text(caps, image).await?;
    if text.is_empty() {
        return Ok(Outcome {
            message: "No text found".into(),
            path: None,
        });
    }
    crate::system::set_clipboard_text(&text)?;
    let words = text.split_whitespace().count();
    Ok(Outcome {
        message: format!("Copied {words} words of text"),
        path: None,
    })
}

/// `dir/stem.ext`, or `dir/stem (2).ext` and so on when taken. An empty ext
/// makes a folder name.
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let make = |n: u32| {
        let base = if n == 1 {
            stem.to_string()
        } else {
            format!("{stem} ({n})")
        };
        if ext.is_empty() {
            dir.join(base)
        } else {
            dir.join(format!("{base}.{ext}"))
        }
    };
    (1..)
        .map(make)
        .find(|p| !p.exists())
        .unwrap_or_else(|| make(1))
}

async fn run(cmd: &mut Command) -> Result<(), ActionError> {
    cmd.kill_on_drop(true).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        // No console window flashing up for each conversion.
        cmd.creation_flags(0x0800_0000);
    }
    let output = tokio::time::timeout(TIMEOUT, cmd.output())
        .await
        .map_err(|_| ActionError::Failed("conversion took too long".into()))?
        .map_err(fail)?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let line = stderr
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("tool failed");
        Err(ActionError::Failed(line.trim().chars().take(160).collect()))
    }
}

fn missing(tool: &str) -> ActionError {
    ActionError::Failed(format!("{tool} is not installed"))
}

fn fail(err: impl std::fmt::Display) -> ActionError {
    ActionError::Failed(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_paths_never_collide() {
        let dir = std::env::temp_dir().join(format!("sidekick-unique-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(unique_path(&dir, "a", "png"), dir.join("a.png"));
        std::fs::write(dir.join("a.png"), b"x").unwrap();
        assert_eq!(unique_path(&dir, "a", "png"), dir.join("a (2).png"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn extracts_a_tar_archive() {
        let caps = crate::Capabilities::detect();
        if caps.tar.is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("sidekick-tar-{}", std::process::id()));
        let src = dir.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("hello.txt"), b"hi").unwrap();
        let archive = dir.join("bundle.tar");
        let ok = std::process::Command::new(caps.tar.as_ref().unwrap())
            .arg("-cf")
            .arg(&archive)
            .arg("-C")
            .arg(&src)
            .arg("hello.txt")
            .status()
            .unwrap();
        assert!(ok.success());
        let out = extract(&caps, &archive).await.unwrap();
        assert_eq!(out.message, "Extracted to bundle");
        assert!(dir.join("bundle/hello.txt").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
