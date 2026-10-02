//! Moving files for Ask: one item into a folder, never over an existing one.

use std::path::Path;

use crate::convert::unique_path;
use crate::{ActionError, Outcome, file_name};

/// Moves `path` into the folder `to`. A name already there gets " (2)".
/// Works across drives by copying, then removing the original.
pub fn move_into(path: &Path, to: &Path) -> Result<Outcome, ActionError> {
    let name = file_name(path);
    let (stem, ext) = match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if path.is_file() => (
            name.trim_end_matches(&format!(".{ext}")).to_owned(),
            ext.to_owned(),
        ),
        _ => (name.clone(), String::new()),
    };
    let dest = unique_path(to, &stem, &ext);
    if std::fs::rename(path, &dest).is_err() {
        if !path.is_file() {
            return Err(ActionError::Failed(format!("could not move {name} there")));
        }
        std::fs::copy(path, &dest).map_err(|e| ActionError::Failed(e.to_string()))?;
        std::fs::remove_file(path).map_err(|e| ActionError::Failed(e.to_string()))?;
    }
    Ok(Outcome {
        message: format!("Moved {name} to {}", file_name(to)),
        path: Some(dest.display().to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_without_overwriting() {
        let root = std::env::temp_dir().join(format!("sidekick-move-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let to = root.join("to");
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(root.join("a.txt"), "new").unwrap();
        std::fs::write(to.join("a.txt"), "old").unwrap();
        let out = move_into(&root.join("a.txt"), &to).unwrap();
        assert!(!root.join("a.txt").exists());
        assert_eq!(std::fs::read_to_string(to.join("a.txt")).unwrap(), "old");
        assert!(out.path.unwrap().ends_with("a (2).txt"));
        let _ = std::fs::remove_dir_all(root);
    }
}
