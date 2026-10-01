//! Extra facts about a finished download, worked out before skills see it:
//! an identical file already in the folder (FR-FILE-05) and, for
//! installers, who signed it (FR-FILE-04).

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Larger files are not hashed (it would take too long to matter).
const MAX_HASH: u64 = 1024 * 1024 * 1024;

fn hash(path: &Path) -> Option<[u8; 32]> {
    let mut r = BufReader::new(File::open(path).ok()?);
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = r.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(h.finalize().into())
}

/// Another file in the same folder with the same size and content.
pub fn duplicate_of(path: &Path) -> Option<PathBuf> {
    let size = path.metadata().ok()?.len();
    if size == 0 || size > MAX_HASH {
        return None;
    }
    let dir = path.parent()?;
    let same_size: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p != path && p.is_file() && p.metadata().is_ok_and(|m| m.len() == size))
        .collect();
    if same_size.is_empty() {
        return None;
    }
    let mine = hash(path)?;
    same_size.into_iter().find(|p| hash(p) == Some(mine))
}

/// "Signed by Publisher", "Not signed", or a warning, for an installer.
/// Uses Windows' own Authenticode check; the path goes in through an
/// environment variable, never into the script text.
#[cfg(windows)]
pub fn signature(path: &Path) -> String {
    use std::os::windows::process::CommandExt;
    const SCRIPT: &str = "$s = Get-AuthenticodeSignature -LiteralPath $env:SIDEKICK_FILE; \
        \"$($s.Status)|$($s.SignerCertificate.Subject)\"";
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .env("SIDEKICK_FILE", path)
        .creation_flags(0x0800_0000)
        .output();
    match out {
        Ok(o) if o.status.success() => describe(&String::from_utf8_lossy(&o.stdout)),
        _ => String::new(),
    }
}

#[cfg(not(windows))]
pub fn signature(_path: &Path) -> String {
    String::new()
}

/// Turns `Status|Subject` into words.
pub fn describe(line: &str) -> String {
    let (status, subject) = line.trim().split_once('|').unwrap_or((line.trim(), ""));
    let signer = subject
        .split(", ")
        .find_map(|part| part.strip_prefix("CN="))
        .map(|cn| cn.trim_matches('"').to_owned());
    match (status, signer) {
        ("Valid", Some(cn)) => format!("Signed by {cn}"),
        ("Valid", None) => "Signed".into(),
        ("NotSigned", _) => "Not signed: run it only if you trust the source".into(),
        ("HashMismatch", _) => "Signature broken: the file was changed after signing".into(),
        ("", _) => String::new(),
        (other, _) => format!("Signature {other}: be careful"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_signatures() {
        assert_eq!(
            describe("Valid|CN=\"Microsoft Corporation\", O=Microsoft Corporation, L=Redmond\n"),
            "Signed by Microsoft Corporation"
        );
        assert!(describe("NotSigned|").starts_with("Not signed"));
        assert!(describe("HashMismatch|CN=X").starts_with("Signature broken"));
        assert_eq!(describe(""), "");
    }

    #[test]
    fn finds_identical_files() {
        let dir = std::env::temp_dir().join(format!("sidekick-dup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("report.pdf"), b"same bytes").unwrap();
        std::fs::write(dir.join("other.pdf"), b"diff bytes").unwrap();
        std::fs::write(dir.join("report (1).pdf"), b"same bytes").unwrap();
        assert_eq!(
            duplicate_of(&dir.join("report (1).pdf")),
            Some(dir.join("report.pdf"))
        );
        assert_eq!(duplicate_of(&dir.join("other.pdf")), None);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
