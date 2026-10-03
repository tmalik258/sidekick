//! PowerShell pieces the desktop scripts share, so each one loads UI
//! Automation, finds an app's window and outlines a control the same way.
//! Prepend the ones a script needs; each is self-contained.

/// Loads UI Automation. `$A` is AutomationElement, `$S` TreeScope and
/// `$All` the condition that matches every element.
pub(crate) const UIA: &str = r##"Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$A=[System.Windows.Automation.AutomationElement]
$S=[System.Windows.Automation.TreeScope]
$All=[System.Windows.Automation.Condition]::TrueCondition
"##;

/// `Find-App $q`: the first app with a window whose process name or title
/// has `$q` in it, preferring windows that have a title.
pub(crate) const FIND_APP: &str = r##"function Find-App($q){
  $ps=@(Get-Process | Where-Object { $_.MainWindowHandle -ne 0 -and ($_.ProcessName -like "*$q*" -or $_.MainWindowTitle -like "*$q*") })
  $t=$ps | Where-Object { $_.MainWindowTitle } | Select-Object -First 1
  if($t){ return $t }
  return $ps | Select-Object -First 1
}
"##;

/// `Show-Outline $rect`: a short blue outline around a screen rectangle
/// (a control's BoundingRectangle), without taking focus.
pub(crate) const OUTLINE: &str = r##"function Show-Outline($r){
  Add-Type -AssemblyName System.Windows.Forms,System.Drawing
  if(-not ('Sk.Hi' -as [type])){ Add-Type -Namespace Sk -Name Hi -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetProcessDPIAware(); [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int c); [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h,IntPtr a,int x,int y,int w,int ht,uint f);' }
  [void][Sk.Hi]::SetProcessDPIAware()
  if($r.IsEmpty -or $r.Width -le 2){ return }
  $b=3; $x=[int]$r.X-$b; $y=[int]$r.Y-$b; $w=[int]$r.Width+2*$b; $h=[int]$r.Height+2*$b
  $f=New-Object System.Windows.Forms.Form
  $f.FormBorderStyle='None'; $f.ShowInTaskbar=$false; $f.TopMost=$true
  $f.BackColor=[System.Drawing.Color]::FromArgb(10,132,255)
  $f.StartPosition='Manual'; $f.Bounds=New-Object System.Drawing.Rectangle($x,$y,$w,$h)
  $g=New-Object System.Drawing.Region(New-Object System.Drawing.Rectangle(0,0,$w,$h))
  $g.Exclude((New-Object System.Drawing.Rectangle($b,$b,($w-2*$b),($h-2*$b))))
  $f.Region=$g
  [void][Sk.Hi]::ShowWindow($f.Handle,4)
  [void][Sk.Hi]::SetWindowPos($f.Handle,[IntPtr](-1),$x,$y,$w,$h,0x10)
  for($i=0;$i -lt 6;$i++){ [System.Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 70 }
  $f.Close(); $f.Dispose()
}
"##;

/// Removes wildcard characters from a name the user gave, which would
/// otherwise match every window.
pub(crate) fn clean_query(q: &str) -> String {
    q.chars()
        .filter(|c| !matches!(c, '*' | '?' | '[' | ']' | '`'))
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_define_what_they_promise() {
        assert!(UIA.contains("$A=") && UIA.contains("$S=") && UIA.contains("$All="));
        assert!(FIND_APP.starts_with("function Find-App"));
        assert!(OUTLINE.starts_with("function Show-Outline"));
        assert_eq!(clean_query(" sla*ck "), "slack");
    }
}
