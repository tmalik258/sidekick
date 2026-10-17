//! Installed apps, from the Windows uninstall list, and opening an app's own
//! uninstaller. Nothing is removed silently: Windows shows the uninstaller,
//! and the person finishes it there.

use serde::{Deserialize, Serialize};

use crate::{ActionError, Outcome, pc};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledApp {
    pub name: String,
    #[serde(default)]
    pub publisher: String,
    #[serde(default)]
    pub uninstall: String,
}

const LIST: &str = r#"$k='HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*','HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*'
Get-ItemProperty $k -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -and $_.UninstallString -and -not $_.SystemComponent } | ForEach-Object { [pscustomobject]@{ name=$_.DisplayName; publisher="$($_.Publisher)"; uninstall=$_.UninstallString } } | ConvertTo-Json -Compress"#;

/// Apps from PowerShell's JSON: one object or a list, sorted, no duplicates.
pub fn parse(json: &str) -> Vec<InstalledApp> {
    let json = json.trim();
    let mut apps: Vec<InstalledApp> = if json.starts_with('[') {
        serde_json::from_str(json).unwrap_or_default()
    } else {
        serde_json::from_str(json)
            .map(|a| vec![a])
            .unwrap_or_default()
    };
    apps.retain(|a| !a.name.trim().is_empty() && !a.uninstall.trim().is_empty());
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
    apps
}

pub fn list() -> Result<Vec<InstalledApp>, ActionError> {
    Ok(parse(&pc::powershell(LIST, &[])?))
}

/// Apps whose name has `query` in it, for the model to show and confirm.
pub fn find(apps: &[InstalledApp], query: &str) -> Vec<InstalledApp> {
    let q = query.trim().to_lowercase();
    apps.iter()
        .filter(|a| q.is_empty() || a.name.to_lowercase().contains(&q))
        .cloned()
        .collect()
}

/// An uninstall command line split into the program and its arguments.
pub fn split_command(line: &str) -> (String, String) {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix('"')
        && let Some(end) = rest.find('"')
    {
        return (rest[..end].to_owned(), rest[end + 1..].trim().to_owned());
    }
    // Unquoted: the program ends at ".exe" when there is one (paths may
    // have spaces), else at the first space.
    let lower = line.to_lowercase();
    let cut = lower
        .find(".exe")
        .map(|i| i + 4)
        .or_else(|| line.find(' '))
        .unwrap_or(line.len());
    (line[..cut].to_owned(), line[cut..].trim().to_owned())
}

/// Opens the uninstaller of the app named exactly `name`.
pub fn uninstall(name: &str) -> Result<Outcome, ActionError> {
    let apps = list()?;
    let app = apps
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(name.trim()))
        .ok_or_else(|| {
            ActionError::Failed(format!("{name} is not in the list of installed apps"))
        })?;
    let (exe, args) = split_command(&app.uninstall);
    let script = "if($env:ARGS){Start-Process -FilePath $env:EXE -ArgumentList $env:ARGS}else{Start-Process -FilePath $env:EXE}";
    pc::powershell(script, &[("EXE", &exe), ("ARGS", &args)])?;
    Ok(Outcome::msg(format!(
        "Opened the uninstaller for {}. Finish it there; this cannot be undone.",
        app.name
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_one_or_many_and_drops_blanks() {
        let one =
            parse(r#"{"name":"Zoom","publisher":"Zoom","uninstall":"\"C:\\Zoom\\uninst.exe\""}"#);
        assert_eq!(one.len(), 1);
        let many = parse(
            r#"[{"name":"b app","uninstall":"x.exe"},{"name":"A App","uninstall":"y.exe"},{"name":"B App","uninstall":"z.exe"},{"name":"","uninstall":"q"}]"#,
        );
        assert_eq!(
            many.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(),
            ["A App", "b app"]
        );
        assert!(parse("garbage").is_empty());
        assert_eq!(find(&many, "a ap").len(), 1);
    }

    #[test]
    fn splits_uninstall_lines() {
        assert_eq!(
            split_command(r#""C:\Program Files\Zoom\uninstall.exe" /silent"#),
            (
                "C:\\Program Files\\Zoom\\uninstall.exe".into(),
                "/silent".into()
            )
        );
        assert_eq!(
            split_command("MsiExec.exe /X{1234}"),
            ("MsiExec.exe".into(), "/X{1234}".into())
        );
        assert_eq!(
            split_command(r"C:\Program Files\App\unins000.exe"),
            ("C:\\Program Files\\App\\unins000.exe".into(), String::new())
        );
    }
}
