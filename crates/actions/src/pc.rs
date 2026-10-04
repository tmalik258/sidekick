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

/// Switches Do Not Disturb from Notification Center, the way a person
/// would: open it, flip the bell switch, close it. Windows has no API for
/// it. Prints `#ok on|off`, `#already on|off`, or `#error <why>`.
const DND_SCRIPT: &str = r##"$ErrorActionPreference='Stop'
try {
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$A=[System.Windows.Automation.AutomationElement]
$want=$env:SK_ON -eq '1'
Start-Process 'ms-actioncenter:'
$btn=$null
for($i=0;$i -lt 40 -and -not $btn;$i++){
  Start-Sleep -Milliseconds 150
  foreach($w in $A::RootElement.FindAll('Children',[System.Windows.Automation.Condition]::TrueCondition)){
    if($w.Current.Name -match 'Notification Center|Action center|Notification centre'){
      $btn=$w.FindAll('Descendants',[System.Windows.Automation.Condition]::TrueCondition) |
        Where-Object { $_.Current.Name -match 'Do not disturb|Focus assist' -and $_.Current.IsEnabled } |
        Select-Object -First 1
      if($btn){break}
    }
  }
}
if(-not $btn){ '#error the Do not disturb switch was not found in Notification Center'; exit }
$t=$null
if($btn.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern,[ref]$t)){
  $on=$t.Current.ToggleState -eq 'On'
  if($on -eq $want){ "#already $(if($want){'on'}else{'off'})" }
  else { $t.Toggle(); Start-Sleep -Milliseconds 200; "#ok $(if($t.Current.ToggleState -eq 'On'){'on'}else{'off'})" }
} else {
  $inv=$null
  if(-not $btn.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$inv)){ '#error the switch did not respond'; exit }
  $inv.Invoke(); "#ok $(if($want){'on'}else{'off'})"
}
} catch { "#error $($_.Exception.Message)" }
finally {
  Start-Sleep -Milliseconds 150
  (New-Object -ComObject WScript.Shell).SendKeys('{ESC}')
}
"##;

/// Active playback devices: `{endpoint}\tName (Driver)`, from the registry
/// Windows keeps for them.
const AUDIO_LIST: &str = r##"$ErrorActionPreference='SilentlyContinue'
$base='HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render'
Get-ChildItem $base | ForEach-Object {
  if((Get-ItemProperty $_.PSPath).DeviceState -eq 1){
    $p=Get-ItemProperty (Join-Path $_.PSPath 'Properties')
    "$($_.PSChildName)`t$($p.'{a45c254e-df1c-4efd-8020-67d146a850e0},2') ($($p.'{b3f8fa53-0004-438e-9003-51a46e139bfc},6'))"
  }
}
"##;

/// Makes an endpoint the default for every role, through the policy
/// interface the Sound control panel uses.
const AUDIO_SET: &str = r##"$ErrorActionPreference='Stop'
try {
if(-not ('Sk.Audio' -as [type])){ Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
namespace Sk {
[Guid("f8679f50-850a-41cf-9c72-430f290290c8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown), ComImport]
interface IPolicyConfig {
  int A(); int B(); int C(); int D(); int E(); int F(); int G(); int H(); int I(); int J();
  [PreserveSig] int SetDefaultEndpoint([MarshalAs(UnmanagedType.LPWStr)] string id, int role);
}
[ComImport, Guid("870af99c-171d-4f9e-af0d-e63df40c2bc9")] class PolicyConfigClient {}
public static class Audio {
  public static int Set(string id) {
    var p = (IPolicyConfig)new PolicyConfigClient(); int r = 0;
    for (int role = 0; role < 3; role++) { r |= p.SetDefaultEndpoint(id, role); }
    return r;
  }
}
}
'@ }
$r=[Sk.Audio]::Set('{0.0.0.00000000}.' + $env:SK_ID)
if($r -eq 0){ '#ok' } else { "#error code $r" }
} catch { "#error $($_.Exception.Message)" }
"##;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
}

pub fn parse_audio(text: &str) -> Vec<AudioDevice> {
    text.lines()
        .filter_map(|l| l.split_once('\t'))
        .filter(|(id, _)| id.starts_with('{') && id.ends_with('}'))
        .map(|(id, name)| AudioDevice {
            id: id.trim().to_owned(),
            name: name.trim().trim_end_matches(" ()").to_owned(),
        })
        .collect()
}

pub fn audio_outputs() -> Result<Vec<AudioDevice>, ActionError> {
    Ok(parse_audio(&powershell(AUDIO_LIST, &[])?))
}

/// The device whose name best matches what the user said ("headphones",
/// "Realtek", "the TV").
pub fn pick_audio<'a>(devices: &'a [AudioDevice], want: &str) -> Option<&'a AudioDevice> {
    let want = want.trim().to_lowercase();
    if want.is_empty() {
        return None;
    }
    devices
        .iter()
        .find(|d| d.name.to_lowercase() == want)
        .or_else(|| {
            devices
                .iter()
                .find(|d| d.name.to_lowercase().contains(&want))
        })
        .or_else(|| {
            const SKIP: &[&str] = &["the", "my", "to", "on", "a", "use", "play"];
            let words: Vec<&str> = want
                .split_whitespace()
                .filter(|w| w.len() >= 2 && !SKIP.contains(w))
                .collect();
            devices.iter().find(|d| {
                let n = d.name.to_lowercase();
                words.iter().any(|w| n.contains(w))
            })
        })
}

pub fn set_audio_output(want: &str) -> Result<Outcome, ActionError> {
    let devices = audio_outputs()?;
    let Some(d) = pick_audio(&devices, want) else {
        let names: Vec<&str> = devices.iter().map(|d| d.name.as_str()).collect();
        return Err(ActionError::Invalid(if names.is_empty() {
            "no playback devices found".into()
        } else {
            format!("no device like {want}; these are on: {}", names.join(", "))
        }));
    };
    let out = powershell(AUDIO_SET, &[("SK_ID", &d.id)])?;
    if !out.contains("#ok") {
        return Err(ActionError::Failed(
            out.trim().trim_start_matches("#error ").to_owned(),
        ));
    }
    Ok(Outcome::msg(format!("Sound now plays on {}", d.name)))
}

/// Which screens show the desktop: this one only, duplicate, extend, or the
/// second one only.
pub fn display_mode(mode: &str) -> Result<Outcome, ActionError> {
    let (flag, said) = match mode.trim().to_lowercase().as_str() {
        "internal" | "pc" | "pc_only" | "this" => ("/internal", "Only this screen"),
        "clone" | "duplicate" | "mirror" => ("/clone", "Screens duplicated"),
        "extend" => ("/extend", "Screens extended"),
        "external" | "second" | "second_only" | "projector" => {
            ("/external", "Only the second screen")
        }
        other => {
            return Err(ActionError::Invalid(format!(
                "unknown display mode {other}; one of internal, clone, extend, external"
            )));
        }
    };
    if !cfg!(windows) {
        return Err(ActionError::Failed("display modes need Windows".into()));
    }
    std::process::Command::new("DisplaySwitch.exe")
        .arg(flag)
        .spawn()
        .map_err(|e| ActionError::Failed(e.to_string()))?;
    Ok(Outcome::msg(said))
}

/// Reads the DND script's answer: the state it ended in, or why it failed.
pub fn parse_dnd(out: &str) -> Result<(bool, bool), String> {
    let line = out
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with('#'))
        .ok_or_else(|| "no answer from Notification Center".to_owned())?;
    let (tag, rest) = line[1..].split_once(' ').unwrap_or((&line[1..], ""));
    match tag {
        "ok" | "already" => Ok((rest.trim() == "on", tag == "already")),
        _ => Err(rest.trim().to_owned()),
    }
}

/// Turns Do Not Disturb on or off, and says so.
pub fn set_dnd(on: bool) -> Result<Outcome, ActionError> {
    state_changed();
    if !cfg!(windows) {
        return Err(ActionError::Failed("Do Not Disturb needs Windows".into()));
    }
    let out = powershell(DND_SCRIPT, &[("SK_ON", if on { "1" } else { "0" })])?;
    let (now, already) = parse_dnd(&out).map_err(ActionError::Failed)?;
    if now != on {
        return Err(ActionError::Failed(
            "Windows did not switch Do Not Disturb".into(),
        ));
    }
    Ok(Outcome::msg(match (on, already) {
        (true, true) => "Do Not Disturb was already on",
        (true, false) => "Done. Do Not Disturb is on",
        (false, true) => "Do Not Disturb was already off",
        (false, false) => "Done. Do Not Disturb is off",
    }))
}

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
        // The registry only shows whether banners are allowed, not the Do Not
        // Disturb switch itself; dnd_on and dnd_off check that live.
        format!(
            "Notification banners: {} (Do Not Disturb is not shown here; dnd_on or dnd_off \
             checks it and says if it was already so)",
            match s.do_not_disturb {
                Switch::On => "off",
                Switch::Off => "on",
                Switch::Unknown => "unknown",
            }
        ),
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
pub(crate) fn powershell(script: &str, env: &[(&str, &str)]) -> Result<String, ActionError> {
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
pub(crate) fn powershell(_script: &str, _env: &[(&str, &str)]) -> Result<String, ActionError> {
    Err(ActionError::Failed("this works on Windows only".into()))
}

/// What is on right now. Everything reads as unknown off Windows.
/// The last reading and when it was taken. Several moments can ask within
/// seconds; each reading starts a PowerShell.
static STATE_CACHE: std::sync::Mutex<Option<(std::time::Instant, PcState)>> =
    std::sync::Mutex::new(None);
const STATE_FRESH: std::time::Duration = std::time::Duration::from_secs(20);

pub fn read_state() -> PcState {
    if let Ok(cache) = STATE_CACHE.lock()
        && let Some((at, state)) = cache.as_ref()
        && at.elapsed() < STATE_FRESH
    {
        return state.clone();
    }
    let state = powershell(STATUS_SCRIPT, &[])
        .map(|t| parse_state(&t))
        .unwrap_or_default();
    if let Ok(mut cache) = STATE_CACHE.lock() {
        *cache = Some((std::time::Instant::now(), state.clone()));
    }
    state
}

/// Forgets the last reading (Sidekick just changed something).
fn state_changed() {
    if let Ok(mut cache) = STATE_CACHE.lock() {
        *cache = None;
    }
}

/// Switches a radio (Bluetooth or Wi-Fi) through Windows' Radio API.
const RADIO: &str = r#"$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$asTask=([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
function Await($op,$type){ $t=$asTask.MakeGenericMethod($type).Invoke($null,@($op)); $t.Wait(-1) | Out-Null; $t.Result }
[Windows.Devices.Radios.Radio,Windows.System.Devices,ContentType=WindowsRuntime] | Out-Null
[Windows.Devices.Radios.RadioAccessStatus,Windows.System.Devices,ContentType=WindowsRuntime] | Out-Null
[Windows.Devices.Radios.RadioState,Windows.System.Devices,ContentType=WindowsRuntime] | Out-Null
Await ([Windows.Devices.Radios.Radio]::RequestAccessAsync()) ([Windows.Devices.Radios.RadioAccessStatus]) | Out-Null
$radios=Await ([Windows.Devices.Radios.Radio]::GetRadiosAsync()) ([System.Collections.Generic.IReadOnlyList[Windows.Devices.Radios.Radio]])
$r=$radios | Where-Object { $_.Kind -eq $env:SK_KIND } | Select-Object -First 1
if(-not $r){ Write-Output "none"; exit }
Await ($r.SetStateAsync($env:SK_STATE)) ([Windows.Devices.Radios.RadioAccessStatus])
"#;

/// Wi-Fi networks in range, strongest first.
pub fn wifi_networks() -> Result<Vec<String>, ActionError> {
    let out = powershell("netsh wlan show networks mode=bssid", &[])?;
    Ok(parse_networks(&out))
}

pub fn parse_networks(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        // "SSID 1 : Home"; BSSID lines start with a B.
        if t.starts_with("SSID ")
            && let Some((_, name)) = t.split_once(':')
        {
            let name = name.trim();
            if !name.is_empty() && !out.iter().any(|n| n == name) {
                out.push(name.to_owned());
            }
        }
    }
    out
}

/// Connects to a Wi-Fi network this PC already knows.
pub fn wifi_connect(name: &str) -> Result<Outcome, ActionError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ActionError::Invalid("which network?".into()));
    }
    let out = powershell(
        "netsh wlan connect name=\"$env:SK_NET\"",
        &[("SK_NET", name)],
    )?;
    if out.to_lowercase().contains("success") {
        Ok(Outcome::msg(format!("Connecting to {name}")))
    } else {
        Err(ActionError::Failed(format!(
            "{name} is not a saved network; connect once from the Wi-Fi menu ({})",
            out.trim()
        )))
    }
}

/// Apps winget can install, by name: "Name<TAB>Id<TAB>Version".
pub fn app_search(q: &str) -> Result<String, ActionError> {
    let q = q.trim();
    if q.is_empty() {
        return Err(ActionError::Invalid("search for what?".into()));
    }
    let out = powershell(
        "winget search --name \"$env:SK_Q\" --accept-source-agreements --count 8 | Out-String -Width 200",
        &[("SK_Q", q)],
    )?;
    Ok(out.trim().to_owned())
}

/// A winget id is letters, digits, dots, dashes and plus signs.
pub fn valid_app_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 120
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
}

/// Installs or updates an app with winget (after the user's tap).
pub fn app_install(id: &str, upgrade: bool) -> Result<Outcome, ActionError> {
    if !valid_app_id(id) {
        return Err(ActionError::Invalid(format!("{id} is not a winget id")));
    }
    let verb = if upgrade { "upgrade" } else { "install" };
    let out = powershell(
        &format!(
            "winget {verb} --id \"$env:SK_ID\" -e --silent --accept-source-agreements --accept-package-agreements | Out-String -Width 200"
        ),
        &[("SK_ID", id)],
    )?;
    let lower = out.to_lowercase();
    if lower.contains("successfully")
        || lower.contains("no applicable upgrade")
        || lower.contains("already installed")
    {
        Ok(Outcome::msg(format!(
            "{} {id}",
            if upgrade { "Updated" } else { "Installed" }
        )))
    } else {
        Err(ActionError::Failed(
            out.lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("winget failed")
                .trim()
                .to_owned(),
        ))
    }
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
    state_changed();
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
        "bluetooth_on" | "bluetooth_off" | "wifi_on" | "wifi_off" => {
            let (kind, on) = what.split_once('_').unwrap_or_default();
            let kind = if kind == "wifi" { "WiFi" } else { "Bluetooth" };
            let state = if on == "on" { "On" } else { "Off" };
            let out = powershell(RADIO, &[("SK_KIND", kind), ("SK_STATE", state)])?;
            if !out.contains("Allowed") {
                return Err(ActionError::Failed(format!(
                    "Windows did not allow switching {kind} ({})",
                    out.trim()
                )));
            }
            Ok(Outcome::msg(format!(
                "{} {}",
                if kind == "WiFi" { "Wi-Fi" } else { kind },
                on
            )))
        }
        "dnd_on" | "dnd_off" => set_dnd(what == "dnd_on"),
        "audio_outputs" => {
            let list = audio_outputs()?;
            Ok(Outcome::msg(if list.is_empty() {
                "No playback devices found".to_owned()
            } else {
                format!(
                    "Playback devices: {}",
                    list.iter()
                        .map(|d| d.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }))
        }
        "audio_output" => set_audio_output(page.unwrap_or_default()),
        "display" => display_mode(page.unwrap_or_default()),
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

/// Installed apps from the Start menu, as (name, AppID).
fn start_apps() -> Result<Vec<(String, String)>, ActionError> {
    let out = powershell(
        "Get-StartApps | ForEach-Object { \"$($_.Name)`t$($_.AppID)\" }",
        &[],
    )?;
    Ok(out
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(n, id)| (n.trim().to_owned(), id.trim().to_owned()))
        .filter(|(n, id)| !n.is_empty() && !id.is_empty())
        .collect())
}

/// Letters and digits only, lowercased: "WIND HAWK" and "Windhawk" agree.
fn squash(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Installed app names that match `q`, closest first: the same name, then
/// names that start with it, then names that contain it or all its words.
pub fn match_apps<'a>(apps: &'a [(String, String)], q: &str) -> Vec<&'a (String, String)> {
    let want = squash(q);
    let words: Vec<String> = q
        .split_whitespace()
        .map(squash)
        .filter(|w| !w.is_empty())
        .collect();
    if want.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<(u8, &(String, String))> = apps
        .iter()
        .filter_map(|a| {
            let name = squash(&a.0);
            let rank = if name == want {
                0
            } else if name.starts_with(&want) {
                1
            } else if name.contains(&want) {
                2
            } else if words.len() > 1 && words.iter().all(|w| name.contains(w.as_str())) {
                3
            } else {
                return None;
            };
            Some((rank, a))
        })
        .collect();
    hits.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.0.len().cmp(&b.1.0.len())));
    hits.into_iter().map(|(_, a)| a).collect()
}

/// Pairs of letters in `s`, for comparing names that are spelled a bit off.
fn pairs(s: &str) -> Vec<(char, char)> {
    let c: Vec<char> = s.chars().collect();
    c.windows(2).map(|w| (w[0], w[1])).collect()
}

/// How alike two names are, 0 to 1, by the letter pairs they share.
fn likeness(a: &str, b: &str) -> f32 {
    let (a, b) = (pairs(&squash(a)), pairs(&squash(b)));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut rest = b.clone();
    let shared = a
        .iter()
        .filter(|p| {
            rest.iter()
                .position(|r| r == *p)
                .map(|i| rest.swap_remove(i))
                .is_some()
        })
        .count();
    2.0 * shared as f32 / (a.len() + b.len()) as f32
}

/// When nothing matches (a typo like "win halt"), the installed names that
/// look closest, best first.
pub fn closest_apps<'a>(apps: &'a [(String, String)], q: &str, n: usize) -> Vec<&'a str> {
    let mut scored: Vec<(f32, &str)> = apps
        .iter()
        .map(|a| (likeness(&a.0, q), a.0.as_str()))
        .filter(|(s, _)| *s >= 0.3)
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.into_iter().take(n).map(|(_, name)| name).collect()
}

/// What an app search found: names that match, or, when none do, the
/// closest-looking ones to ask about.
pub enum Found {
    Matches(Vec<String>),
    Closest(Vec<String>),
}

/// Installed apps whose name is like `q`, for "do I have ...".
pub fn find_apps(q: &str) -> Result<Found, ActionError> {
    let q = query(q)?;
    let apps = start_apps()?;
    let hits: Vec<String> = match_apps(&apps, &q)
        .into_iter()
        .take(10)
        .map(|a| a.0.clone())
        .collect();
    if !hits.is_empty() {
        return Ok(Found::Matches(hits));
    }
    Ok(Found::Closest(
        closest_apps(&apps, &q, 3)
            .into_iter()
            .map(str::to_owned)
            .collect(),
    ))
}

/// Starts an installed app by name (spelling and spaces forgiven); only
/// apps in the Start menu start.
pub fn launch_app(q: &str) -> Result<Outcome, ActionError> {
    let q = query(q)?;
    let apps = start_apps()?;
    let Some((name, id)) = match_apps(&apps, &q)
        .first()
        .map(|a| (a.0.clone(), a.1.clone()))
    else {
        // A typo never starts the wrong app; it names the likely ones.
        let close = closest_apps(&apps, &q, 3);
        return Err(ActionError::Failed(if close.is_empty() {
            format!("no installed app named {q}")
        } else {
            format!(
                "no installed app named {q}; did you mean {}?",
                close.join(" or ")
            )
        }));
    };
    powershell(
        "Start-Process \"shell:AppsFolder\\$env:SIDEKICK_APP_ID\"",
        &[("SIDEKICK_APP_ID", &id)],
    )?;
    Ok(Outcome::msg(format!("Opened {name}")))
}

/// Brings an app to the front, or starts it when it is not open.
pub fn open_app(q: &str) -> Result<Outcome, ActionError> {
    focus_window(q).or_else(|_| launch_app(q))
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
    #[test]
    fn picks_audio_devices() {
        let list = parse_audio(
            "{0001}\tSpeakers (Realtek(R) Audio)\n{0002}\tHeadphones (WH-1000XM4)\nnoise\n{0003}\tLG TV (NVIDIA High Definition Audio)\n",
        );
        assert_eq!(list.len(), 3);
        assert_eq!(pick_audio(&list, "headphones").unwrap().id, "{0002}");
        assert_eq!(pick_audio(&list, "the tv").unwrap().id, "{0003}");
        assert_eq!(pick_audio(&list, "realtek").unwrap().id, "{0001}");
        assert!(pick_audio(&list, "projector xyz").is_none());
        assert!(pick_audio(&list, "").is_none());
        assert!(display_mode("sideways").is_err());
    }

    #[test]
    fn reads_dnd_answers() {
        assert_eq!(parse_dnd("noise\n#ok on\n"), Ok((true, false)));
        assert_eq!(parse_dnd("#already off"), Ok((false, true)));
        assert_eq!(parse_dnd("#error not found"), Err("not found".to_owned()));
        assert!(parse_dnd("").is_err());
    }

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
        assert!(d.contains("Notification banners: off"));
        assert!(!d.contains("Do Not Disturb: "));

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
        let apps: Vec<(String, String)> = [
            ("Windhawk", "Windhawk.App"),
            ("Windows Terminal", "Microsoft.WindowsTerminal"),
            ("Hawk Viewer", "Hawk"),
            ("Spotify", "Spotify.App"),
        ]
        .iter()
        .map(|(n, id)| ((*n).to_owned(), (*id).to_owned()))
        .collect();
        let names = |q: &str| -> Vec<String> {
            match_apps(&apps, q)
                .into_iter()
                .map(|a| a.0.clone())
                .collect()
        };
        assert_eq!(names("WIND HAWK"), ["Windhawk"]);
        assert_eq!(names("spotify"), ["Spotify"]);
        assert_eq!(names("win"), ["Windhawk", "Windows Terminal"]);
        assert_eq!(names("terminal windows"), ["Windows Terminal"]);
        assert!(names("photoshop").is_empty());
        assert_eq!(names("win hawk"), ["Windhawk"]);
        // Typos match nothing but are offered as the closest names.
        assert!(names("win halt").is_empty());
        assert_eq!(
            closest_apps(&apps, "win halt", 3).first(),
            Some(&"Windhawk")
        );
        assert_eq!(closest_apps(&apps, "spotfy", 3).first(), Some(&"Spotify"));
        assert!(closest_apps(&apps, "photoshop", 3).is_empty());
        assert_eq!(query(" chr*ome ").unwrap(), "chrome");
    }

    #[test]
    fn reads_wifi_networks() {
        let text = "Interface name : Wi-Fi\nThere are 2 networks currently visible.\n\nSSID 1 : Home 5G\n    Network type : Infrastructure\n    BSSID 1 : aa:bb\nSSID 2 : Cafe\n";
        assert_eq!(parse_networks(text), ["Home 5G", "Cafe"]);
        assert!(valid_app_id("Spotify.Spotify"));
        assert!(!valid_app_id("x; rm -rf"));
        assert!(control("bluetooth_maybe", None, None).is_err());
    }

    #[test]
    fn reads_windows() {
        let w = parse_windows("chrome\tInbox - Gmail\nCode\tsidekick - main.rs\nexplorer\t\n");
        assert_eq!(w.len(), 2);
        assert_eq!(w[1].app, "Code");
    }
}
