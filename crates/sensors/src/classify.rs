//! Pure classifiers used by sensors, kept separate so they are easy to test.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

/// What kind of file a path is, by extension.
pub fn file_kind(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "bmp" | "tif" | "tiff" | "heic"
        | "svg" => "image",
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" => "video",
        "mp3" | "wav" | "flac" | "m4a" | "ogg" | "aac" => "audio",
        "pdf" | "doc" | "docx" | "odt" | "rtf" | "txt" | "md" | "ppt" | "pptx" | "xls" | "xlsx"
        | "csv" => "document",
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" => "archive",
        "exe" | "msi" | "msix" | "appx" | "dmg" | "pkg" | "deb" | "rpm" | "appimage" => "installer",
        "js" | "ts" | "tsx" | "jsx" | "py" | "rs" | "go" | "java" | "json" | "yaml" | "yml"
        | "toml" | "sql" | "sh" | "ps1" => "code",
        _ => "other",
    }
}

/// True for files a browser or tool writes while a download is in progress,
/// and for hidden or lock files nobody wants suggestions about.
pub fn is_partial_or_hidden(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let lower = name.to_ascii_lowercase();
    lower.is_empty()
        || lower.starts_with('.')
        || lower.starts_with("~$")
        || [
            ".crdownload",
            ".part",
            ".partial",
            ".tmp",
            ".download",
            ".opdownload",
            ".!ut",
        ]
        .iter()
        .any(|suffix| lower.ends_with(suffix))
        || lower == "desktop.ini"
}

/// What kind of text was copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipKind {
    Secret,
    Url,
    Json,
    Color,
    Email,
    Path,
    StackTrace,
    Code,
    Text,
}

impl ClipKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ClipKind::Secret => "secret",
            ClipKind::Url => "url",
            ClipKind::Json => "json",
            ClipKind::Color => "color",
            ClipKind::Email => "email",
            ClipKind::Path => "path",
            ClipKind::StackTrace => "stacktrace",
            ClipKind::Code => "code",
            ClipKind::Text => "text",
        }
    }
}

static SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(AKIA[0-9A-Z]{16})",                                 // AWS access key
        r"|(sk-(ant-|proj-)?[A-Za-z0-9_\-]{20,})",              // OpenAI / Anthropic
        r"|(gh[pousr]_[A-Za-z0-9]{30,})",                       // GitHub tokens
        r"|(xox[abpr]-[A-Za-z0-9\-]{10,})",                     // Slack
        r"|(-----BEGIN [A-Z ]*PRIVATE KEY-----)",               // private keys
        r"|(eyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,})", // JWT
        r"|((?i:password|passwd|secret|api[_-]?key|access[_-]?token|auth[_-]?token)\s*[:=]\s*\S{6,})",
    ))
    .expect("secret regex")
});
static URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(https?://|www\.)\S+$").expect("url regex"));
static COLOR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(#[0-9a-fA-F]{3,8}|rgba?\([^)]+\)|hsla?\([^)]+\)|oklch\([^)]+\))$")
        .expect("color regex")
});
static EMAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}$").expect("email regex")
});
static PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^("?[A-Za-z]:\\|\\\\|/(home|Users|mnt|usr|etc|var|tmp)/|~/)"#)
        .expect("path regex")
});
static STACK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(Traceback \(most recent call last\))|(\n\s+at .+[:(]\d+)|(^\w*(Error|Exception):)|(panicked at )|(error\[E\d{4}\])")
        .expect("stack regex")
});

pub fn clip_kind(text: &str) -> ClipKind {
    let t = text.trim();
    if SECRET.is_match(t) {
        return ClipKind::Secret;
    }
    if !t.contains('\n') {
        if URL.is_match(t) {
            return ClipKind::Url;
        }
        if COLOR.is_match(t) {
            return ClipKind::Color;
        }
        if EMAIL.is_match(t) {
            return ClipKind::Email;
        }
        if PATH.is_match(t) {
            return ClipKind::Path;
        }
    }
    if (t.starts_with('{') && t.ends_with('}') || t.starts_with('[') && t.ends_with(']'))
        && serde_json::from_str::<serde_json::Value>(t).is_ok()
    {
        return ClipKind::Json;
    }
    if STACK.is_match(t) {
        return ClipKind::StackTrace;
    }
    let code_marks = [
        "fn ",
        "def ",
        "const ",
        "import ",
        "function ",
        "class ",
        "=> ",
        "};",
        "</",
    ];
    if t.lines().count() > 1 && code_marks.iter().any(|m| t.contains(m)) {
        return ClipKind::Code;
    }
    ClipKind::Text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_files() {
        assert_eq!(file_kind(Path::new("a/Report.PDF")), "document");
        assert_eq!(file_kind(Path::new("x.webp")), "image");
        assert_eq!(file_kind(Path::new("setup.msi")), "installer");
        assert_eq!(file_kind(Path::new("noext")), "other");
    }

    #[test]
    fn spots_partial_downloads() {
        assert!(is_partial_or_hidden(Path::new("movie.mp4.crdownload")));
        assert!(is_partial_or_hidden(Path::new("a.zip.part")));
        assert!(is_partial_or_hidden(Path::new("~$doc.docx")));
        assert!(!is_partial_or_hidden(Path::new("invoice.pdf")));
    }

    #[test]
    fn classifies_clipboard_text() {
        assert_eq!(
            clip_kind("https://github.com/tmalik258/sidekick"),
            ClipKind::Url
        );
        assert_eq!(clip_kind(r#"{"a": [1, 2]}"#), ClipKind::Json);
        assert_eq!(clip_kind("#0a84ff"), ClipKind::Color);
        assert_eq!(clip_kind("tmalik.dev@example.com"), ClipKind::Email);
        assert_eq!(clip_kind(r"C:\Users\talha\Downloads"), ClipKind::Path);
        assert_eq!(
            clip_kind("ghp_abcdefghijklmnopqrstuvwxyz0123456789AB"),
            ClipKind::Secret
        );
        assert_eq!(clip_kind("API_KEY=supersecretvalue"), ClipKind::Secret);
        assert_eq!(
            clip_kind("TypeError: x is undefined\n    at foo (app.js:10:5)"),
            ClipKind::StackTrace
        );
        assert_eq!(
            clip_kind("const a = 1;\nexport default a;\n};"),
            ClipKind::Code
        );
        assert_eq!(clip_kind("hello there"), ClipKind::Text);
    }
}
