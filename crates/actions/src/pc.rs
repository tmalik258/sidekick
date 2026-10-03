//! The PC itself: what is on (night light, Do Not Disturb, dark mode,
//! battery, Wi-Fi, brightness) and the everyday switches (volume,
//! brightness, dark mode, lock, settings pages, apps and windows).
//!
//! Windows has no API for most of these, so each runs one short hidden
//! PowerShell script. Anything the user names (an app, a window) reaches the
//! script through an environment variable, never inside its text.

use serde::Serialize;

use crate::{ActionError, Outcome};

/// On, off, or not readable here (another OS, a missing key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Switch {
    On,
    Off,
    #[default]
    Unknown,
}

impl Switch {
    pub fn as_str(self) -> &'static str {
        match self {
            Switch::On => "on",
            Switch::Off => "off",
            Switch::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct PcState {
    pub night_light: Switch,
    pub do_not_disturb: Switch,
    pub dark_mode: Switch,
    pub battery: Option<u8>,
    pub charging: Option<bool>,
    pub brightness: Option<u8>,
    pub wifi: Option<String>,
}

const STATUS_SCRIPT: &str = r#"$ErrorActionPreference='SilentlyContinue'
$k='HKCU:\Software\Microsoft\Windows\CurrentVersion\CloudStore\Store\DefaultAccount\Current\default$windows.data.bluelightreduction.bluelightreductionstate\windows.data.bluelightreduction.bluelightreductionstate'
$d=(Get-ItemProperty -LiteralPath $k).Data
if($d -and $d.Length -gt 18){"night_light_byte=$($d[18])"}
$n=Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings'
"toasts=$($n.NOC_GLOBAL_SETTING_TOASTS_ENABLED)"
$p=Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
"apps_light=$($p.AppsUseLightTheme)"
$b=Get-CimInstance Win32_Battery | Select-Object -First 1
if($b){"battery=$($b.EstimatedChargeRemaining)";"battery_status=$($b.BatteryStatus)"}
$br=Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightness | Select-Object -First 1
if($br){"brightness=$($br.CurrentBrightness)"}
$w=netsh wlan show interfaces | Select-String '^\s+SSID\s+:' | Select-Object -First 1
if($w){"wifi=$((($w.Line) -split ':',2)[1].Trim())"}
"#;

/// Reads the status script's `key=value` lines.
pub fn parse_state(text: &str) -> PcState {
    let mut s = PcState::default();
    // Without the key, Windows uses its defaults: notifications on.
    let mut saw_toasts_line = false;
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key {
            // Byte 18 of the state blob: 0x15 on, 0x13 off. A schedule that
            // is running right now reads as on.
            "night_light_byte" => {
                s.night_light = match value {
                    "21" => Switch::On,
                    "19" => Switch::Off,
                    _ => Switch::Unknown,
                }
            }
            "toasts" => {
                saw_toasts_line = true;
                s.do_not_disturb = if value == "0" {
                    Switch::On
                } else {
                    Switch::Off
                };
            }
            "apps_light" => {
                s.dark_mode = match value {
                    "0" => Switch::On,
                    "1" | "" => Switch::Off,
                    _ => Switch::Unknown,
                }
            }
            "battery" => s.battery = value.parse().ok(),
            // 2 is on AC power; 6 to 9 are charging states.
            "battery_status" => {
                s.charging = value
                    .parse::<u8>()
                    .ok()
                    .map(|v| v == 2 || (6..=9).contains(&v))
            }
            "brightness" => s.brightness = value.parse().ok(),
            "wifi" if !value.is_empty() => s.wifi = Some(value.to_owned()),
            _ => {}
        }
    }
    if !saw_toasts_line {
        s.do_not_disturb = Switch::Unknown;
    }
    s
}

/// The state, in a few lines a model can read.
pub fn describe(s: &PcState) -> String {
    let mut out = vec![
        format!("Night light: {}", s.night_light.as_str()),
        format!("Do Not Disturb: {}", s.do_not_disturb.as_str()),
        format!("Dark mode: {}", s.dark_mode.as_str()),
    ];
    if let Some(b) = s.battery {
        let plug = match s.charging {
            Some(true) => ", plugged in",
            Some(false) => ", on battery",
            None => "",
        };
        out.push(format!("Battery: {b}%{plug}"));
    }
    if let Some(b) = s.brightness {
        out.push(format!("Brightness: {b}%"));
    }
    out.push(format!(
        "Wi-Fi: {}",
        s.wifi.as_deref().unwrap_or("not connected or unknown")
    ));
    out.join("\n")
}

/// Runs a hidden PowerShell script and returns what it printed.
#[cfg(windows)]
fn powershell(script: &str, env: &[(&str, &str)]) -> Result<String, ActionError> {
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-WindowStyle",
        "Hidden",
        "-Command",
        script,
    ])
    .creation_flags(0x0800_0000);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd
        .output()
        .map_err(|e| ActionError::Failed(e.to_string()))?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(not(windows))]
fn powershell(_script: &str, _env: &[(&str, &str)]) -> Result<String, ActionError> {
    Err(ActionError::Failed("this works on Windows only".into()))
}

/// What is on right now. Everything reads as unknown off Windows.
pub fn read_state() -> PcState {
    powershell(STATUS_SCRIPT, &[])
        .map(|t| parse_state(&t))
        .unwrap_or_default()
}

/// Settings pages by name; nothing else can be opened through here.
pub const SETTINGS_PAGES: &[(&str, &str)] = &[
    ("home", "ms-settings:"),
    ("display", "ms-settings:display"),
    ("nightlight", "ms-settings:nightlight"),
    ("sound", "ms-settings:sound"),
    ("notifications", "ms-settings:notifications"),
    ("focus", "ms-settings:quiethours"),
    ("bluetooth", "ms-settings:bluetooth"),
    ("wifi", "ms-settings:network-wifi"),
    ("network", "ms-settings:network"),
    ("battery", "ms-settings:batterysaver"),
    ("power", "ms-settings:powersleep"),
    ("storage", "ms-settings:storagesense"),
    ("apps", "ms-settings:appsfeatures"),
    ("default_apps", "ms-settings:defaultapps"),
    ("startup_apps", "ms-settings:startupapps"),
    ("colors", "ms-settings:colors"),
    ("background", "ms-settings:personalization-background"),
    ("mouse", "ms-settings:mousetouchpad"),
    ("keyboard", "ms-settings:keyboard"),
    ("printers", "ms-settings:printers"),
    ("updates", "ms-settings:windowsupdate"),
    ("privacy", "ms-settings:privacy"),
    ("accounts", "ms-settings:yourinfo"),
    ("time", "ms-settings:dateandtime"),
    ("language", "ms-settings:regionlanguage"),
    ("about", "ms-settings:about"),
];

pub fn settings_uri(page: &str) -> Option<&'static str> {
    SETTINGS_PAGES
        .iter()
        .find(|(name, _)| *name == page)
        .map(|(_, uri)| *uri)
}

fn level(value: Option<u8>) -> Result<u8, ActionError> {
    value
        .filter(|v| *v <= 100)
        .ok_or_else(|| ActionError::Invalid("pass a level from 0 to 100".into()))
}

/// One everyday switch. All are reversible and need no confirmation.
pub fn control(what: &str, value: Option<u8>, page: Option<&str>) -> Result<Outcome, ActionError> {
    // Volume keys move 2% per press.
    const KEYS: &str = "$s=New-Object -ComObject WScript.Shell;";
    let msg = |m: &str| Ok(Outcome::msg(m));
    match what {
        "volume_up" => {
            powershell(&format!("{KEYS}1..5|%{{$s.SendKeys([char]175)}}"), &[])?;
            msg("Volume up")
        }
        "volume_down" => {
            powershell(&format!("{KEYS}1..5|%{{$s.SendKeys([char]174)}}"), &[])?;
            msg("Volume down")
        }
        "mute" => {
            powershell(&format!("{KEYS}$s.SendKeys([char]173)"), &[])?;
            msg("Mute switched")
        }
        "set_volume" => {
            let v = level(value)?;
            powershell(
                &format!(
                    "{KEYS}1..50|%{{$s.SendKeys([char]174)}};1..{}|%{{$s.SendKeys([char]175)}}",
                    (v / 2).max(1)
                ),
                &[],
            )?;
            if v < 2 {
                powershell(&format!("{KEYS}$s.SendKeys([char]174)"), &[])?;
            }
            Ok(Outcome::msg(format!("Volume at {v}%")))
        }
        "brightness" => {
            let v = level(value)?.to_string();
            let out = powershell(
                "$m=Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightnessMethods -ErrorAction SilentlyContinue; \
                 if($m){$m | Invoke-CimMethod -MethodName WmiSetBrightness -Arguments @{Timeout=1;Brightness=[byte]$env:SIDEKICK_LEVEL} | Out-Null; 'ok'}",
                &[("SIDEKICK_LEVEL", &v)],
            )?;
            if out.trim() != "ok" {
                return Err(ActionError::Failed(
                    "this screen's brightness can't be set from Windows (an external monitor?)"
                        .into(),
                ));
            }
            Ok(Outcome::msg(format!("Brightness at {v}%")))
        }
        "dark_mode_on" | "dark_mode_off" => {
            let light = if what == "dark_mode_on" { "0" } else { "1" };
            powershell(
                "$p='HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize'; \
                 Set-ItemProperty $p AppsUseLightTheme ([int]$env:SIDEKICK_LIGHT); \
                 Set-ItemProperty $p SystemUsesLightTheme ([int]$env:SIDEKICK_LIGHT)",
                &[("SIDEKICK_LIGHT", light)],
            )?;
            msg(if light == "0" {
                "Dark mode on"
            } else {
                "Light mode on"
            })
        }
        "lock" => {
            powershell("rundll32.exe user32.dll,LockWorkStation", &[])?;
            msg("Locked")
        }
        "open_settings" => {
            // No page: the Settings home.
            let page = page.filter(|p| !p.trim().is_empty()).unwrap_or("home");
            let uri = settings_uri(page).ok_or_else(|| {
                ActionError::Invalid(format!(
                    "unknown settings page {page}; one of: {}",
                    SETTINGS_PAGES
                        .iter()
                        .map(|(n, _)| *n)
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?;
            if !cfg!(windows) {
                return Err(ActionError::Failed("Windows settings need Windows".into()));
            }
            open::that_detached(uri).map_err(|e| ActionError::Failed(e.to_string()))?;
            msg("Opened Settings")
        }
        other => Err(ActionError::Invalid(format!("unknown switch {other}"))),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Window {
    pub app: String,
    pub title: String,
}

pub fn parse_windows(text: &str) -> Vec<Window> {
    text.lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(app, title)| Window {
            app: app.trim().to_owned(),
            title: title.trim().to_owned(),
        })
        .filter(|w| !w.title.is_empty())
        .collect()
}

/// Open windows with a title, by app.
pub fn windows() -> Result<Vec<Window>, ActionError> {
    let out = powershell(
        "Get-Process | Where-Object { $_.MainWindowTitle } | ForEach-Object { \"$($_.ProcessName)`t$($_.MainWindowTitle)\" }",
        &[],
    )?;
    Ok(parse_windows(&out))
}

/// Finds the first window whose app or title has `query` in it.
const MATCH_WINDOW: &str = "$q=$env:SIDEKICK_QUERY; $p=Get-Process | Where-Object { $_.MainWindowTitle -and ($_.ProcessName -like \"*$q*\" -or $_.MainWindowTitle -like \"*$q*\") } | Select-Object -First 1;";

fn query(q: &str) -> Result<String, ActionError> {
    // Wildcards would match every window.
    let q: String = q
        .chars()
        .filter(|c| !matches!(c, '*' | '?' | '[' | ']' | '`'))
        .collect();
    let q = q.trim().to_owned();
    if q.is_empty() {
        return Err(ActionError::Invalid("name the app or window".into()));
    }
    Ok(q)
}

/// Brings a window to the front.
pub fn focus_window(q: &str) -> Result<Outcome, ActionError> {
    let q = query(q)?;
    let out = powershell(
        &format!(
            "{MATCH_WINDOW} if($p){{ (New-Object -ComObject WScript.Shell).AppActivate($p.Id) | Out-Null; $p.MainWindowTitle }}"
        ),
        &[("SIDEKICK_QUERY", &q)],
    )?;
    match out.trim() {
        "" => Err(ActionError::Failed(format!("no open window matches {q}"))),
        title => Ok(Outcome::msg(format!("Switched to {title}"))),
    }
}

/// Asks an app's window to close, as if its X was pressed (it can still
/// ask to save).
pub fn close_window(q: &str) -> Result<Outcome, ActionError> {
    let q = query(q)?;
    let out = powershell(
        &format!(
            "{MATCH_WINDOW} if($p){{ $t=$p.MainWindowTitle; $p.CloseMainWindow() | Out-Null; $t }}"
        ),
        &[("SIDEKICK_QUERY", &q)],
    )?;
    match out.trim() {
        "" => Err(ActionError::Failed(format!("no open window matches {q}"))),
        title => Ok(Outcome::msg(format!("Closed {title}"))),
    }
}

/// Starts an app from the Start menu by name; only installed apps start.
pub fn launch_app(q: &str) -> Result<Outcome, ActionError> {
    let q = query(q)?;
    let out = powershell(
        "$a=Get-StartApps | Where-Object { $_.Name -like \"*$env:SIDEKICK_QUERY*\" } | Select-Object -First 1; \
         if($a){ Start-Process \"shell:AppsFolder\\$($a.AppID)\"; $a.Name }",
        &[("SIDEKICK_QUERY", &q)],
    )?;
    match out.trim() {
        "" => Err(ActionError::Failed(format!("no installed app named {q}"))),
        name => Ok(Outcome::msg(format!("Opened {name}"))),
    }
}

pub fn sleep_pc() -> Result<Outcome, ActionError> {
    powershell("rundll32.exe powrprof.dll,SetSuspendState 0,1,0", &[])?;
    Ok(Outcome::msg("Going to sleep"))
}

pub fn empty_recycle_bin() -> Result<Outcome, ActionError> {
    powershell("Clear-RecycleBin -Force -ErrorAction SilentlyContinue", &[])?;
    Ok(Outcome::msg("Recycle Bin emptied"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_is_on() {
        let s = parse_state(
            "night_light_byte=21\ntoasts=0\napps_light=0\nbattery=64\nbattery_status=2\nbrightness=70\nwifi=Home 5G\n",
        );
        assert_eq!(s.night_light, Switch::On);
        assert_eq!(s.do_not_disturb, Switch::On);
        assert_eq!(s.dark_mode, Switch::On);
        assert_eq!(s.battery, Some(64));
        assert_eq!(s.charging, Some(true));
        assert_eq!(s.wifi.as_deref(), Some("Home 5G"));
        let d = describe(&s);
        assert!(d.contains("Night light: on"));
        assert!(d.contains("Battery: 64%, plugged in"));

        let off = parse_state("night_light_byte=19\ntoasts=\napps_light=1\n");
        assert_eq!(off.night_light, Switch::Off);
        assert_eq!(
            off.do_not_disturb,
            Switch::Off,
            "no value means notifications on"
        );
        assert_eq!(off.dark_mode, Switch::Off);
        assert_eq!(off.battery, None);

        let none = parse_state("");
        assert_eq!(none.night_light, Switch::Unknown);
        assert_eq!(none.do_not_disturb, Switch::Unknown);
    }

    #[test]
    fn switches_take_only_known_names() {
        assert!(control("format_c", None, None).is_err());
        assert!(control("set_volume", Some(150), None).is_err());
        assert!(control("open_settings", None, Some("cmd.exe")).is_err());
        assert_eq!(settings_uri("nightlight"), Some("ms-settings:nightlight"));
        assert_eq!(settings_uri("home"), Some("ms-settings:"));
        assert!(query("**").is_err());
        assert_eq!(query(" chr*ome ").unwrap(), "chrome");
    }

    #[test]
    fn reads_windows() {
        let w = parse_windows("chrome\tInbox - Gmail\nCode\tsidekick - main.rs\nexplorer\t\n");
        assert_eq!(w.len(), 2);
        assert_eq!(w[1].app, "Code");
    }
}
