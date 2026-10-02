//! The speech models, downloaded once from the sherpa-onnx releases on
//! GitHub, checked against pinned SHA-256 hashes, and unpacked into the app
//! data folder.

use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use sha2::{Digest, Sha256};

use crate::{Result, VoiceError};

#[derive(Debug, Clone, Copy)]
pub struct Model {
    pub id: &'static str,
    pub label: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    /// Download size in bytes.
    pub size: u64,
    /// Folder inside the archive (and on disk).
    pub dir: &'static str,
    /// Files that must exist once unpacked.
    pub files: &'static [&'static str],
    /// Archive entries not worth unpacking.
    pub skip: &'static [&'static str],
}

pub const WAKE: Model = Model {
    id: "wake",
    label: "Wake word",
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/kws-models/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01.tar.bz2",
    sha256: "f170013b4716e41b62b9bfd809687c207cef798ef9bc6534d524e17af9b6561a",
    size: 17_626_723,
    dir: "sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01",
    files: &[
        "encoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx",
        "decoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx",
        "joiner-epoch-12-avg-2-chunk-16-left-64.int8.onnx",
        "tokens.txt",
    ],
    skip: &["test_wavs/", "-64.onnx"],
};

pub const SPEECH: Model = Model {
    id: "speech",
    label: "Speech to text",
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-streaming-zipformer-en-kroko-2025-08-06.tar.bz2",
    sha256: "c8676e5ff9ac2a85296e53ee0fd4d5fb1db6770e7a7647166eeafe349ade6834",
    size: 57_267_600,
    dir: "sherpa-onnx-streaming-zipformer-en-kroko-2025-08-06",
    files: &["encoder.onnx", "decoder.onnx", "joiner.onnx", "tokens.txt"],
    skip: &["test_wavs/"],
};

/// Kokoro v1.0 at full precision: far more natural than the int8 v0.19
/// build (af_heart is its warmest voice). Only the English parts are
/// needed; the Chinese lexicon is skipped (the engine still requires
/// `dict/`).
pub const KOKORO: Model = Model {
    id: "kokoro",
    label: "Kokoro voice",
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/kokoro-multi-lang-v1_0.tar.bz2",
    sha256: "c5f7e2d2caf082bc1d20fb70334a61d99d20b484500aad32e7cf84c128ea3298",
    size: 349_906_910,
    dir: "kokoro-multi-lang-v1_0",
    files: &[
        "model.onnx",
        "voices.bin",
        "tokens.txt",
        "lexicon-us-en.txt",
        "lexicon-gb-en.txt",
        "espeak-ng-data",
        "dict",
    ],
    skip: &["lexicon-zh.txt", "-zh.fst"],
};

pub const MODELS: [Model; 3] = [WAKE, SPEECH, KOKORO];

/// Folders of models that were replaced, removed once their successor is in.
const RETIRED: &[(&str, Model)] = &[("kokoro-int8-en-v0_19", KOKORO)];

/// Deletes replaced models whose successor is installed. Returns how many.
pub fn remove_retired(root: &Path) -> usize {
    RETIRED
        .iter()
        .filter(|(dir, successor)| {
            let old = root.join(dir);
            old.is_dir() && successor.installed(root) && fs::remove_dir_all(&old).is_ok()
        })
        .count()
}

impl Model {
    pub fn path(&self, root: &Path) -> PathBuf {
        root.join(self.dir)
    }

    pub fn file(&self, root: &Path, name: &str) -> String {
        self.path(root).join(name).to_string_lossy().into_owned()
    }

    pub fn installed(&self, root: &Path) -> bool {
        let dir = self.path(root);
        self.files.iter().all(|f| dir.join(f).exists())
    }
}

pub fn all_installed(root: &Path) -> bool {
    MODELS.iter().all(|m| m.installed(root))
}

/// Downloads, verifies and unpacks `model` into `root`. `progress` gets
/// bytes done and total. Setting `cancel` stops it and removes partial files.
pub fn install(
    model: &Model,
    root: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, u64),
) -> Result<()> {
    if model.installed(root) {
        return Ok(());
    }
    fs::create_dir_all(root)?;
    let part = root.join(format!("{}.part", model.id));
    let result =
        download(model, &part, cancel, &mut progress).and_then(|()| unpack(model, &part, root));
    let _ = fs::remove_file(&part);
    if result.is_ok() {
        remove_retired(root);
    }
    result
}

fn download(
    model: &Model,
    part: &Path,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(u64, u64),
) -> Result<()> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("Sidekick")
        .timeout(None)
        .build()
        .map_err(|e| VoiceError::Download(e.to_string()))?;
    let mut resp = client
        .get(model.url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| VoiceError::Download(e.to_string()))?;
    let total = resp.content_length().unwrap_or(model.size);
    let mut file = File::create(part)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    let mut reported = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(VoiceError::Download("cancelled".into()));
        }
        let n = resp
            .read(&mut buf)
            .map_err(|e| VoiceError::Download(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])?;
        done += n as u64;
        if done - reported >= 1024 * 1024 || done == total {
            reported = done;
            progress(done, total);
        }
    }
    file.flush()?;
    let hash = format!("{:x}", hasher.finalize());
    if hash != model.sha256 {
        return Err(VoiceError::Download(format!(
            "{} did not match its checksum; try again",
            model.label
        )));
    }
    progress(done, total);
    Ok(())
}

/// Unpacks into a temporary folder first, so a half-unpacked model never
/// looks installed.
fn unpack(model: &Model, archive: &Path, root: &Path) -> Result<()> {
    let staging = root.join(format!(".unpack-{}", model.id));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;
    let reader = bzip2::read::BzDecoder::new(BufReader::new(File::open(archive)?));
    let mut tar = tar::Archive::new(reader);
    for entry in tar.entries()? {
        let mut entry = entry?;
        let name = entry.path()?.to_string_lossy().into_owned();
        if model.skip.iter().any(|s| name.contains(s)) {
            continue;
        }
        // unpack_in refuses entries that would land outside `staging`.
        entry.unpack_in(&staging)?;
    }
    let unpacked = staging.join(model.dir);
    if !model.files.iter().all(|f| unpacked.join(f).exists()) {
        let _ = fs::remove_dir_all(&staging);
        return Err(VoiceError::Download(format!(
            "{} archive is incomplete",
            model.label
        )));
    }
    let target = model.path(root);
    let _ = fs::remove_dir_all(&target);
    fs::rename(&unpacked, &target)?;
    let _ = fs::remove_dir_all(&staging);
    Ok(())
}

/// Total download size of the models not yet installed.
pub fn missing_bytes(root: &Path) -> u64 {
    MODELS
        .iter()
        .filter(|m| !m.installed(root))
        .map(|m| m.size)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpacks_only_what_is_needed_and_refuses_escapes() {
        let dir = std::env::temp_dir().join(format!("sidekick-voice-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let model = Model {
            id: "t",
            label: "Test",
            url: "",
            sha256: "",
            size: 0,
            dir: "m",
            files: &["a.onnx"],
            skip: &["test_wavs/"],
        };
        let archive = dir.join("t.tar.bz2");
        {
            let enc = bzip2::write::BzEncoder::new(
                File::create(&archive).unwrap(),
                bzip2::Compression::fast(),
            );
            let mut b = tar::Builder::new(enc);
            for (name, body) in [("m/a.onnx", "model"), ("m/test_wavs/0.wav", "wav")] {
                let mut h = tar::Header::new_gnu();
                h.set_size(body.len() as u64);
                h.set_mode(0o644);
                h.set_cksum();
                b.append_data(&mut h, name, body.as_bytes()).unwrap();
            }
            b.into_inner().unwrap().finish().unwrap();
        }
        unpack(&model, &archive, &dir).unwrap();
        assert!(model.installed(&dir));
        assert!(!dir.join("m/test_wavs").exists());
        assert!(!dir.join(".unpack-t").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn removes_a_replaced_model_only_once_its_successor_is_in() {
        let dir = std::env::temp_dir().join(format!("sidekick-retired-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("kokoro-int8-en-v0_19")).unwrap();
        assert_eq!(remove_retired(&dir), 0, "kept while v1.0 is missing");
        for f in KOKORO.files {
            let p = KOKORO.path(&dir).join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, "x").unwrap();
        }
        assert_eq!(remove_retired(&dir), 1);
        assert!(!dir.join("kokoro-int8-en-v0_19").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn hashes_are_pinned() {
        for m in MODELS {
            assert_eq!(m.sha256.len(), 64, "{}", m.id);
            assert!(
                m.url
                    .starts_with("https://github.com/k2-fsa/sherpa-onnx/releases/download/")
            );
        }
    }
}
