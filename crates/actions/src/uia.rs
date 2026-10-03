//! Acting inside desktop apps through Windows UI Automation, the layer
//! screen readers use: a window's buttons, fields, menus and lists come
//! back with their names, so "click Send" never guesses pixels.
//!
//! Each call is one short PowerShell script using .NET's UIAutomationClient.
//! Elements are numbered in document order; an action walks the same order
//! again and checks the name still matches before touching anything.
//!
//! Also here: reading the selected text and typing into the focused field
//! of any app ("rewrite this politely"), and keys as a last resort.

use crate::pc::powershell;
use crate::{ActionError, Outcome};

/// Which window: the app the user was in (by process id), or one named.
#[derive(Debug, Clone, Default)]
pub struct Target {
    pub pid: Option<u32>,
    pub app: Option<String>,
}

impl Target {
    fn env(&self) -> Vec<(&'static str, String)> {
        let mut v = Vec::new();
        if let Some(p) = self.pid {
            v.push(("SK_PID", p.to_string()));
        }
        if let Some(a) = self.app.as_deref().filter(|a| !a.trim().is_empty()) {
            v.push(("SK_APP", clean_query(a)));
        }
        v
    }
}

fn clean_query(q: &str) -> String {
    q.chars()
        .filter(|c| !matches!(c, '*' | '?' | '[' | ']' | '`'))
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Finds the window, then lists its controls the same way every time.
const FIND: &str = r##"$ErrorActionPreference='SilentlyContinue'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$A=[System.Windows.Automation.AutomationElement]
$S=[System.Windows.Automation.TreeScope]
$win=$null
if($env:SK_APP){
  $q=$env:SK_APP
  $p=Get-Process | Where-Object { $_.MainWindowHandle -ne 0 -and ($_.ProcessName -like "*$q*" -or $_.MainWindowTitle -like "*$q*") } | Select-Object -First 1
  if($p){ $win=$A::FromHandle($p.MainWindowHandle) }
}
if(-not $win -and $env:SK_PID){
  $p=Get-Process -Id ([int]$env:SK_PID)
  if($p -and $p.MainWindowHandle -ne 0){ $win=$A::FromHandle($p.MainWindowHandle) }
}
if(-not $win){ Write-Output "#error`tNo window found"; exit }
$kinds=@('Button','Edit','Document','CheckBox','RadioButton','ComboBox','ListItem','MenuItem','TabItem','Hyperlink','TreeItem','SplitButton','DataItem')
$all=$win.FindAll($S::Descendants,[System.Windows.Automation.Condition]::TrueCondition)
$els=New-Object System.Collections.ArrayList
foreach($e in $all){
  $c=$e.Current
  $k=$c.ControlType.ProgrammaticName -replace '^ControlType\.',''
  if($kinds -notcontains $k){ continue }
  if($c.IsOffscreen -and $k -ne 'MenuItem'){ continue }
  [void]$els.Add($e)
  if($els.Count -ge 200){ break }
}
function Val($e){
  $p=$null
  if($e.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern,[ref]$p)){ return $p.Current.Value }
  return ''
}
function Line($i,$e){
  $c=$e.Current
  $k=$c.ControlType.ProgrammaticName -replace '^ControlType\.',''
  $n=($c.Name -replace "[`t`r`n]+",' ')
  if($n.Length -gt 80){ $n=$n.Substring(0,80) }
  $v=''
  if($k -eq 'Edit' -or $k -eq 'ComboBox' -or $k -eq 'Document'){ $v=((Val $e) -replace "[`t`r`n]+",' '); if($v.Length -gt 80){ $v=$v.Substring(0,80) } }
  if($c.IsPassword){ $k='Password'; $v='' }
  $en= if($c.IsEnabled){'1'}else{'0'}
  return "$i`t$k`t$n`t$v`t$en"
}
"##;

const SNAPSHOT: &str = r##"
Write-Output ("#window`t" + $win.Current.Name)
$i=0
foreach($e in $els){ $i++; Write-Output (Line $i $e) }
"##;

const ACT: &str = r##"
$n=[int]$env:SK_REF
$e=$null
if($n -ge 1 -and $n -le $els.Count){ $e=$els[$n-1] }
# The window changed since it was read: find the same name instead.
if($env:SK_NAME -and (-not $e -or $e.Current.Name -ne $env:SK_NAME)){
  $e=$null
  foreach($x in $els){ if($x.Current.Name -eq $env:SK_NAME){ $e=$x; break } }
}
if(-not $e){ Write-Output "#error`tThat control is not there now; read the window again"; exit }
$c=$e.Current
if($c.IsPassword -and $env:SK_DO -eq 'type'){ Write-Output "#error`tSidekick never types into password fields"; exit }
$p=$null
$sh=New-Object -ComObject WScript.Shell
switch($env:SK_DO){
  'click' {
    if($e.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$p)){ $p.Invoke(); Write-Output "#ok`tClicked $($c.Name)"; break }
    if($e.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern,[ref]$p)){ $p.Toggle(); Write-Output "#ok`tSwitched $($c.Name)"; break }
    if($e.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern,[ref]$p)){ $p.Select(); Write-Output "#ok`tSelected $($c.Name)"; break }
    if($e.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern,[ref]$p)){ $p.Expand(); Write-Output "#ok`tOpened $($c.Name)"; break }
    $e.SetFocus(); $sh.SendKeys(' '); Write-Output "#ok`tPressed $($c.Name)"
  }
  'type' {
    if($e.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern,[ref]$p) -and -not $p.Current.IsReadOnly){ $p.SetValue($env:SK_TEXT); Write-Output "#ok`tTyped into $($c.Name)"; break }
    $e.SetFocus(); Start-Sleep -Milliseconds 100
    Set-Clipboard -Value $env:SK_TEXT; $sh.SendKeys('^a'); $sh.SendKeys('^v')
    Write-Output "#ok`tTyped into $($c.Name)"
  }
  'select' {
    if($e.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern,[ref]$p)){ $p.Expand(); Start-Sleep -Milliseconds 300 }
    $want=$env:SK_TEXT
    $items=$e.FindAll($S::Descendants,[System.Windows.Automation.Condition]::TrueCondition)
    $hit=$null
    foreach($x in $items){ if($x.Current.Name -like "*$want*"){ $hit=$x; break } }
    if($hit -and $hit.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern,[ref]$p)){ $p.Select(); Write-Output "#ok`tPicked $($hit.Current.Name)"; break }
    Write-Output "#error`tNo option like $want"
  }
  'focus' { $e.SetFocus(); Write-Output "#ok`tFocused $($c.Name)" }
  default { Write-Output "#error`tUnknown action $($env:SK_DO)" }
}
"##;

#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    pub n: usize,
    pub kind: String,
    pub name: String,
    pub value: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Snapshot {
    pub window: String,
    pub controls: Vec<Control>,
}

impl Snapshot {
    /// Lines a model reads: `[12] Button "Send"`.
    pub fn describe(&self) -> String {
        let mut out = format!("Window: {}\n", self.window);
        for c in &self.controls {
            if c.name.is_empty()
                && c.value.is_empty()
                && !matches!(c.kind.as_str(), "Edit" | "Document")
            {
                continue;
            }
            out.push_str(&format!("[{}] {} \"{}\"", c.n, c.kind, c.name));
            if !c.value.is_empty() {
                out.push_str(&format!(" = \"{}\"", c.value));
            }
            if !c.enabled {
                out.push_str(" (disabled)");
            }
            out.push('\n');
        }
        out
    }
}

fn error_of(text: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.strip_prefix("#error\t"))
        .map(str::to_owned)
}

pub fn parse_snapshot(text: &str) -> Result<Snapshot, String> {
    if let Some(e) = error_of(text) {
        return Err(e);
    }
    let mut s = Snapshot::default();
    for line in text.lines() {
        if let Some(w) = line.strip_prefix("#window\t") {
            s.window = w.trim().to_owned();
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 5 {
            continue;
        }
        let Ok(n) = f[0].trim().parse() else { continue };
        s.controls.push(Control {
            n,
            kind: f[1].trim().to_owned(),
            name: f[2].trim().to_owned(),
            value: f[3].trim().to_owned(),
            enabled: f[4].trim() == "1",
        });
    }
    Ok(s)
}

fn fail(e: String) -> ActionError {
    ActionError::Failed(e)
}

fn run(script: &str, env: &[(&'static str, String)]) -> Result<String, ActionError> {
    let pairs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    powershell(script, &pairs)
}

/// The window's controls, numbered.
pub fn snapshot(target: &Target) -> Result<Snapshot, ActionError> {
    let out = run(&format!("{FIND}{SNAPSHOT}"), &target.env())?;
    parse_snapshot(&out).map_err(fail)
}

/// Click, type, select or focus control `n` (checked against `name`).
pub fn act(
    target: &Target,
    n: usize,
    name: &str,
    what: &str,
    text: &str,
) -> Result<Outcome, ActionError> {
    if !matches!(what, "click" | "type" | "select" | "focus") {
        return Err(ActionError::Invalid(format!("unknown action {what}")));
    }
    let mut env = target.env();
    env.push(("SK_REF", n.to_string()));
    env.push(("SK_NAME", name.to_owned()));
    env.push(("SK_DO", what.to_owned()));
    env.push(("SK_TEXT", text.to_owned()));
    let out = run(&format!("{FIND}{ACT}"), &env)?;
    if let Some(e) = error_of(&out) {
        return Err(fail(e));
    }
    let note = out
        .lines()
        .find_map(|l| l.strip_prefix("#ok\t"))
        .unwrap_or("Done")
        .to_owned();
    Ok(Outcome::msg(note))
}

/// Keys that may send something (Enter) or are risky.
pub fn keys_send(keys: &str) -> bool {
    let k = keys.to_uppercase();
    k.contains("{ENTER}") || k.contains('~') || k.contains("{DEL") || k.contains("%{F4}")
}

/// Keys in SendKeys form ("^s", "{TAB}", "%f") to the app, after bringing
/// it to the front. The fallback for apps that show no controls.
pub fn keys(target: &Target, keys: &str) -> Result<Outcome, ActionError> {
    if keys.trim().is_empty() {
        return Err(ActionError::Invalid("which keys?".into()));
    }
    let mut env = target.env();
    env.push(("SK_KEYS", keys.to_owned()));
    let out = run(
        &format!(
            "{FIND}\n$h=$win.Current.NativeWindowHandle\n$pr=Get-Process | Where-Object {{ $_.MainWindowHandle -eq $h }} | Select-Object -First 1\n$sh=New-Object -ComObject WScript.Shell\nif($pr){{ [void]$sh.AppActivate($pr.Id) }}\nStart-Sleep -Milliseconds 200\n$sh.SendKeys($env:SK_KEYS)\nWrite-Output \"#ok`tSent keys\""
        ),
        &env,
    )?;
    if let Some(e) = error_of(&out) {
        return Err(fail(e));
    }
    Ok(Outcome::msg(format!("Pressed {keys}")))
}

/// Brings the app forward and runs one clipboard round trip, keeping what
/// was on the clipboard before.
const CLIP: &str = r##"$ErrorActionPreference='SilentlyContinue'
Add-Type -AssemblyName System.Windows.Forms
$sh=New-Object -ComObject WScript.Shell
if($env:SK_PID){ [void]$sh.AppActivate([int]$env:SK_PID) }
Start-Sleep -Milliseconds 250
$old=Get-Clipboard -Raw
"##;

/// The text selected in the app the user was in.
pub fn selection(target: &Target) -> Result<String, ActionError> {
    let out = run(
        &format!(
            "{CLIP}Set-Clipboard -Value ' '\n$sh.SendKeys('^c')\nStart-Sleep -Milliseconds 300\n$sel=Get-Clipboard -Raw\nif($old){{ Set-Clipboard -Value $old }}\nif($sel -and $sel -ne ' '){{ Write-Output $sel }}"
        ),
        &target.env(),
    )?;
    let text = out.trim_end().to_owned();
    if text.is_empty() {
        return Err(ActionError::Failed("nothing is selected there".into()));
    }
    Ok(text)
}

/// Puts text into the field the user was in (replacing a selection).
pub fn type_here(target: &Target, text: &str) -> Result<Outcome, ActionError> {
    let mut env = target.env();
    env.push(("SK_TEXT", text.to_owned()));
    run(
        &format!(
            "{CLIP}Set-Clipboard -Value $env:SK_TEXT\n$sh.SendKeys('^v')\nStart-Sleep -Milliseconds 300\nif($old){{ Set-Clipboard -Value $old }}\nWrite-Output \"#ok\""
        ),
        &env,
    )?;
    Ok(Outcome::msg("Typed it in"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_window() {
        let s = parse_snapshot(
            "#window\tInbox - Outlook\n1\tButton\tNew mail\t\t1\n2\tEdit\tTo\tali@x.com\t1\n3\tButton\tSend\t\t0\n4\tPassword\tPIN\t\t1\njunk\n",
        )
        .unwrap();
        assert_eq!(s.window, "Inbox - Outlook");
        assert_eq!(s.controls.len(), 4);
        let d = s.describe();
        assert!(d.contains("[2] Edit \"To\" = \"ali@x.com\""));
        assert!(d.contains("[3] Button \"Send\" (disabled)"));
        assert_eq!(
            parse_snapshot("#error\tNo window found").unwrap_err(),
            "No window found"
        );
    }

    #[test]
    fn enter_counts_as_sending() {
        assert!(keys_send("{ENTER}"));
        assert!(keys_send("hello~"));
        assert!(!keys_send("^s"));
        assert!(!keys_send("{TAB}{TAB}"));
        assert!(act(&Target::default(), 1, "", "drag", "").is_err());
        assert_eq!(clean_query(" sla*ck "), "slack");
    }
}
