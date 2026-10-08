//! App doctor: why an app crashes or will not start. Reads the Windows
//! crash record (Application log: Application Error 1000, .NET Runtime
//! 1026, Application Hang 1002), names the likely cause from the faulting
//! module and exception code, and says which fix to offer. Fixes come from
//! Microsoft (winget) or Windows itself, run only after the user's tap, and
//! compatibility mode can be cleared again.

use serde::Serialize;

use crate::{ActionError, Outcome};

/// One crash or hang from the Application log.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Crash {
    pub when: String,
    pub app: String,
    pub path: String,
    pub module: String,
    pub code: String,
    /// "crash", "hang" or "dotnet".
    pub kind: String,
    pub detail: String,
}

/// A likely cause and the fix to offer, if Sidekick can run one.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cause {
    pub what: String,
    /// The action to propose and its args, e.g. ("install_app", {"id": ...}).
    pub fix: Option<(String, serde_json::Value, String)>,
}

fn install(id: &str, label: &str) -> Option<(String, serde_json::Value, String)> {
    Some((
        "install_app".into(),
        serde_json::json!({ "id": id }),
        label.into(),
    ))
}

/// Likely causes for a crash, most likely first.
pub fn causes(crash: &Crash) -> Vec<Cause> {
    let m = crash.module.to_lowercase();
    let code = crash.code.to_lowercase();
    let detail = crash.detail.to_lowercase();
    let mut out = Vec::new();
    let vc = [
        "msvcp",
        "vcruntime",
        "ucrtbase",
        "concrt",
        "vcomp",
        "mfc1",
        "msvcr",
    ];
    if vc.iter().any(|p| m.starts_with(p)) || code == "0xc000007b" {
        out.push(Cause {
            what: "A Visual C++ runtime file is missing or broken.".into(),
            fix: install(
                "Microsoft.VCRedist.2015+.x64",
                "Install Visual C++ runtime (x64)",
            ),
        });
        if code == "0xc000007b" {
            out.push(Cause {
                what: "A 32-bit and 64-bit file mix-up; the 32-bit runtime often fixes it.".into(),
                fix: install(
                    "Microsoft.VCRedist.2015+.x86",
                    "Install Visual C++ runtime (x86)",
                ),
            });
        }
    }
    if m.starts_with("d3dx")
        || m.starts_with("xinput1_")
        || m.starts_with("xaudio2_")
        || m.starts_with("d3dcompiler_4")
    {
        out.push(Cause {
            what: "An older DirectX file the app needs is missing.".into(),
            fix: install("Microsoft.DirectX", "Install DirectX runtime"),
        });
    }
    if crash.kind == "dotnet"
        || m == "clr.dll"
        || m == "coreclr.dll"
        || m == "kernelbase.dll" && code == "0xe0434352"
    {
        out.push(Cause {
            what: ".NET threw an error the app did not handle; a missing or old .NET runtime is common.".into(),
            fix: install("Microsoft.DotNet.DesktopRuntime.8", "Install .NET 8 Desktop Runtime"),
        });
    }
    let gpu = [
        "nvwgf2um",
        "nvoglv",
        "nvlddmkm",
        "atio6axx",
        "amdxx",
        "aticfx",
        "igd10",
        "igxelpicd",
        "ig9icd",
    ];
    if gpu.iter().any(|p| m.starts_with(p)) {
        out.push(Cause {
            what: "The graphics driver crashed. An updated driver usually fixes it.".into(),
            fix: Some((
                "open_system_page".into(),
                serde_json::json!({ "page": "windows_update" }),
                "Check for driver updates".into(),
            )),
        });
    }
    if code == "0xc0000135" || detail.contains("was not found") {
        out.push(Cause {
            what: "A file the app needs to start was not found; reinstalling the app or its runtime fixes it.".into(),
            fix: None,
        });
    }
    if crash.kind == "hang" {
        out.push(Cause {
            what: "The app stopped responding rather than crashing; it may be waiting on a disk, network or another program.".into(),
            fix: None,
        });
    }
    if out.is_empty() && !crash.path.is_empty() && (code == "0xc0000005" || code == "0xc0000409") {
        out.push(Cause {
            what: "A memory error inside the app. Older apps often run in Windows 8 compatibility mode.".into(),
            fix: Some((
                "set_compat".into(),
                serde_json::json!({ "exe": crash.path, "mode": "WIN8RTM" }),
                "Run it in Windows 8 compatibility mode".into(),
            )),
        });
    }
    out
}

/// The 20 newest crashes and hangs in the last 14 days, matching `app` by
/// name when given.
#[cfg(windows)]
pub fn crashes(app: Option<&str>) -> Result<Vec<Crash>, ActionError> {
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
$since = (Get-Date).AddDays(-14)
$ev = Get-WinEvent -FilterHashtable @{ LogName = 'Application'; Id = 1000, 1002, 1026; StartTime = $since } -MaxEvents 200
foreach ($e in $ev) {
  $p = $e.Properties | ForEach-Object { "$($_.Value)" }
  $msg = ($e.Message -replace "`r?`n", ' ')
  if ($msg.Length -gt 400) { $msg = $msg.Substring(0, 400) }
  "$($e.TimeCreated.ToString('s'))`t$($e.Id)`t$($p -join '|')`t$msg"
}
"#;
    let out = crate::pc::powershell(script, &[])?;
    let mut list: Vec<Crash> = out.lines().filter_map(parse_line).collect();
    if let Some(app) = app.map(str::to_lowercase).filter(|a| !a.trim().is_empty()) {
        let words: Vec<&str> = app.split_whitespace().collect();
        list.retain(|c| {
            let hay = format!("{} {}", c.app, c.path).to_lowercase();
            words.iter().all(|w| hay.contains(w))
        });
    }
    list.truncate(20);
    Ok(list)
}

#[cfg(not(windows))]
pub fn crashes(_app: Option<&str>) -> Result<Vec<Crash>, ActionError> {
    Ok(Vec::new())
}

/// One line of the script's output: time, event id, properties, message.
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_line(line: &str) -> Option<Crash> {
    let mut parts = line.splitn(4, '\t');
    let when = parts.next()?.to_owned();
    let id = parts.next()?;
    let props: Vec<&str> = parts.next()?.split('|').collect();
    let msg = parts.next().unwrap_or_default().to_owned();
    let get = |i: usize| {
        props
            .get(i)
            .map(|s| s.trim().to_owned())
            .unwrap_or_default()
    };
    match id {
        // Application Error: app, version, time, module, version, time, code, offset, pid, start, path.
        "1000" => Some(Crash {
            when,
            app: get(0),
            path: get(10),
            module: get(3),
            code: normal_code(&get(6)),
            kind: "crash".into(),
            detail: msg,
        }),
        // Application Hang: app, version, pid, start, timeout, path.
        "1002" => Some(Crash {
            when,
            app: get(0),
            path: get(5),
            module: String::new(),
            code: String::new(),
            kind: "hang".into(),
            detail: msg,
        }),
        // .NET Runtime: one text property starting "Application: x.exe".
        "1026" => {
            let app = msg
                .split("Application: ")
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or_default()
                .to_owned();
            Some(Crash {
                when,
                app,
                path: String::new(),
                module: String::new(),
                code: String::new(),
                kind: "dotnet".into(),
                detail: msg,
            })
        }
        _ => None,
    }
}

/// "c0000005" or "3221225477" as "0xc0000005".
#[cfg_attr(not(windows), allow(dead_code))]
fn normal_code(raw: &str) -> String {
    let raw = raw.trim().trim_start_matches("0x");
    if raw.is_empty() {
        return String::new();
    }
    if raw.len() > 8
        && let Ok(n) = raw.parse::<u64>()
    {
        return format!("0x{n:08x}");
    }
    format!("0x{}", raw.to_lowercase())
}

/// Compatibility layers Sidekick will set.
const MODES: &[&str] = &[
    "WIN8RTM",
    "WIN7RTM",
    "WINXPSP3",
    "RUNASADMIN",
    "DISABLEDXMAXIMIZEDWINDOWEDMODE",
];
const LAYERS: &str = r"HKCU\Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers";

/// Runs `exe` in a compatibility mode for this user (undo: clear_compat).
pub fn set_compat(exe: &str, mode: &str) -> Result<Outcome, ActionError> {
    let mode = mode.to_uppercase();
    if !MODES.contains(&mode.as_str()) {
        return Err(ActionError::Invalid(format!(
            "{mode} is not a mode Sidekick sets"
        )));
    }
    if !exe.to_lowercase().ends_with(".exe") || !std::path::Path::new(exe).is_file() {
        return Err(ActionError::Invalid("pick the app's .exe file".into()));
    }
    reg(&[
        "add",
        LAYERS,
        "/v",
        exe,
        "/t",
        "REG_SZ",
        "/d",
        &format!("~ {mode}"),
        "/f",
    ])?;
    Ok(Outcome::msg(format!(
        "{} will run in {} mode. Say \"clear compatibility mode\" to undo.",
        name_of(exe),
        plain_mode(&mode)
    )))
}

/// Removes Sidekick's (or any) compatibility mode for `exe`.
pub fn clear_compat(exe: &str) -> Result<Outcome, ActionError> {
    reg(&["delete", LAYERS, "/v", exe, "/f"])?;
    Ok(Outcome::msg(format!(
        "{} runs normally again",
        name_of(exe)
    )))
}

fn reg(args: &[&str]) -> Result<(), ActionError> {
    let status = std::process::Command::new("reg")
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(crate::fail)?;
    if status.success() {
        Ok(())
    } else {
        Err(ActionError::Failed(
            "Windows did not accept the change".into(),
        ))
    }
}

fn name_of(exe: &str) -> String {
    std::path::Path::new(exe)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| exe.to_owned())
}

fn plain_mode(mode: &str) -> &'static str {
    match mode {
        "WIN8RTM" => "Windows 8 compatibility",
        "WIN7RTM" => "Windows 7 compatibility",
        "WINXPSP3" => "Windows XP compatibility",
        "RUNASADMIN" => "run as administrator",
        _ => "fullscreen optimizations off",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crash(module: &str, code: &str) -> Crash {
        Crash {
            when: "2026-10-01T10:00:00".into(),
            app: "game.exe".into(),
            path: r"C:\Games\game.exe".into(),
            module: module.into(),
            code: code.into(),
            kind: "crash".into(),
            detail: String::new(),
        }
    }

    fn first_fix(c: &Crash) -> Option<String> {
        causes(c).into_iter().find_map(|c| c.fix).map(|f| f.2)
    }

    #[test]
    fn names_the_runtime_from_the_faulting_module() {
        assert_eq!(
            first_fix(&crash("MSVCP140.dll", "0xc0000005")).as_deref(),
            Some("Install Visual C++ runtime (x64)")
        );
        assert_eq!(
            first_fix(&crash("d3dx9_43.dll", "0xc0000135")).as_deref(),
            Some("Install DirectX runtime")
        );
        assert_eq!(
            first_fix(&crash("nvwgf2umx.dll", "0xc0000005")).as_deref(),
            Some("Check for driver updates")
        );
        assert_eq!(
            first_fix(&crash("ntdll.dll", "0xc0000005")).as_deref(),
            Some("Run it in Windows 8 compatibility mode")
        );
        let mut dotnet = crash("", "");
        dotnet.kind = "dotnet".into();
        assert_eq!(
            first_fix(&dotnet).as_deref(),
            Some("Install .NET 8 Desktop Runtime")
        );
        assert_eq!(causes(&crash("unknown.dll", "0x80000003")), Vec::new());
    }

    #[test]
    fn reads_crash_records() {
        let line = "2026-10-01T10:00:00\t1000\tgame.exe|1.0|0|MSVCP140.dll|14.0|0|c0000005|0x1|0x2|0|C:\\Games\\game.exe|C:\\Windows\\MSVCP140.dll\tFaulting application name: game.exe";
        let c = parse_line(line).unwrap();
        assert_eq!(c.app, "game.exe");
        assert_eq!(c.module, "MSVCP140.dll");
        assert_eq!(c.code, "0xc0000005");
        assert_eq!(c.path, r"C:\Games\game.exe");
        let net = parse_line(
            "2026-10-01T10:00:00\t1026\t\tApplication: Tool.exe Framework Version: v4.0",
        )
        .unwrap();
        assert_eq!(net.app, "Tool.exe");
        assert_eq!(net.kind, "dotnet");
        assert_eq!(normal_code("3221225477"), "0xc0000005");
    }

    #[test]
    fn only_known_modes_are_set() {
        assert!(set_compat(r"C:\x.exe", "HACK").is_err());
        assert!(set_compat(r"C:\not-there.exe", "WIN8RTM").is_err());
    }
}
