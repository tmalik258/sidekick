//! Office through its COM automation, the way a macro would: an Outlook
//! draft with attachments (shown, never sent), reading and writing Excel
//! ranges, and Word documents or PDFs. Needs the desktop Office apps; the
//! new Outlook has no automation, so drafts need classic Outlook.
//!
//! Like `pc`, each call is one hidden PowerShell script and everything the
//! user names reaches it through environment variables.

use std::path::Path;

use crate::pc::powershell;
use crate::{ActionError, Outcome};

/// Most rows and columns read from a sheet at once.
const MAX_ROWS: usize = 200;
const MAX_COLS: usize = 30;

const DRAFT: &str = r##"$ErrorActionPreference='Stop'
try {
  $o=New-Object -ComObject Outlook.Application
  $m=$o.CreateItem(0)
  $m.To=$env:SK_TO; $m.Subject=$env:SK_SUBJECT; $m.Body=$env:SK_BODY
  $n=0
  foreach($a in ($env:SK_ATTACH -split "`n")){ if($a -and (Test-Path -LiteralPath $a)){ [void]$m.Attachments.Add($a); $n++ } }
  $m.Display()
  "#ok $n"
} catch { "#error $($_.Exception.Message)" }
"##;

const EXCEL_READ: &str = r##"$ErrorActionPreference='Stop'
$x=$null
try {
  $x=New-Object -ComObject Excel.Application; $x.Visible=$false; $x.DisplayAlerts=$false
  $wb=$x.Workbooks.Open($env:SK_PATH,0,$true)
  $ws= if($env:SK_SHEET){ $wb.Worksheets.Item($env:SK_SHEET) } else { $wb.Worksheets.Item(1) }
  $r= if($env:SK_RANGE){ $ws.Range($env:SK_RANGE) } else { $ws.UsedRange }
  $rows=[Math]::Min($r.Rows.Count,[int]$env:SK_MAXR); $cols=[Math]::Min($r.Columns.Count,[int]$env:SK_MAXC)
  $names=@(); foreach($s in $wb.Worksheets){ $names+=$s.Name }
  "#sheet`t$($ws.Name)`t$($r.Address(0,0))`t$($names -join ', ')"
  for($i=1;$i -le $rows;$i++){
    $line=@(); for($j=1;$j -le $cols;$j++){ $line+=([string]$r.Cells.Item($i,$j).Text) -replace "[`t`r`n]",' ' }
    $line -join "`t"
  }
  $wb.Close($false)
} catch { "#error $($_.Exception.Message)" }
finally { if($x){ $x.Quit(); [void][Runtime.InteropServices.Marshal]::ReleaseComObject($x) } }
"##;

const EXCEL_WRITE: &str = r##"$ErrorActionPreference='Stop'
$x=$null
try {
  $x=New-Object -ComObject Excel.Application; $x.Visible=$false; $x.DisplayAlerts=$false
  $wb=$x.Workbooks.Open($env:SK_PATH)
  $ws= if($env:SK_SHEET){ $wb.Worksheets.Item($env:SK_SHEET) } else { $wb.Worksheets.Item(1) }
  $start=$ws.Range($env:SK_RANGE).Cells.Item(1,1)
  $rows=$env:SK_VALUES -split "`n"
  for($i=0;$i -lt $rows.Count;$i++){
    $cells=$rows[$i].TrimEnd("`r") -split "`t"
    for($j=0;$j -lt $cells.Count;$j++){ $start.Offset($i,$j).Value2=$cells[$j] }
  }
  $wb.Save(); $wb.Close($false)
  "#ok $($rows.Count)"
} catch { "#error $($_.Exception.Message)" }
finally { if($x){ $x.Quit(); [void][Runtime.InteropServices.Marshal]::ReleaseComObject($x) } }
"##;

/// Writes `SK_TEXT` into a new document, or opens `SK_SRC`; saves as
/// `SK_PATH` (docx 16, pdf 17).
const WORD: &str = r##"$ErrorActionPreference='Stop'
$w=$null
try {
  $w=New-Object -ComObject Word.Application; $w.Visible=$false; $w.DisplayAlerts=0
  if($env:SK_SRC){ $d=$w.Documents.Open($env:SK_SRC,$false,$true) } else { $d=$w.Documents.Add(); $d.Content.Text=$env:SK_TEXT }
  $fmt= if($env:SK_PATH -like '*.pdf'){ 17 } else { 16 }
  $d.SaveAs2($env:SK_PATH,$fmt); $d.Close(0)
  '#ok'
} catch { "#error $($_.Exception.Message)" }
finally { if($w){ $w.Quit(); [void][Runtime.InteropServices.Marshal]::ReleaseComObject($w) } }
"##;

fn need_windows() -> Result<(), ActionError> {
    if cfg!(windows) {
        Ok(())
    } else {
        Err(ActionError::Failed("Office needs Windows".into()))
    }
}

/// The script's `#error` line as an error, with a friendlier word for the
/// usual cause (the app is not installed).
fn check(out: &str, app: &str) -> Result<(), ActionError> {
    let Some(err) = out.lines().find_map(|l| l.trim().strip_prefix("#error")) else {
        return Ok(());
    };
    let err = err.trim();
    if err.contains("80040154") || err.contains("Class not registered") {
        return Err(ActionError::Failed(format!(
            "{app} (the desktop app) is not installed"
        )));
    }
    Err(ActionError::Failed(err.to_owned()))
}

fn existing(path: &str) -> Result<&str, ActionError> {
    if !Path::new(path).is_absolute() || !Path::new(path).exists() {
        return Err(ActionError::Invalid(format!("no file at {path}")));
    }
    Ok(path)
}

/// An Outlook email draft, opened for the user to check and send.
pub fn email_draft(
    to: &str,
    subject: &str,
    body: &str,
    attach: &[String],
) -> Result<Outcome, ActionError> {
    need_windows()?;
    for a in attach {
        existing(a)?;
    }
    let files = attach.join("\n");
    let out = powershell(
        DRAFT,
        &[
            ("SK_TO", to),
            ("SK_SUBJECT", subject),
            ("SK_BODY", body),
            ("SK_ATTACH", &files),
        ],
    )?;
    check(&out, "Classic Outlook")?;
    Ok(Outcome::msg(if attach.is_empty() {
        "The draft is open in Outlook; send it when it looks right".to_owned()
    } else {
        format!(
            "The draft is open in Outlook with {} attached; send it when it looks right",
            attach.len()
        )
    }))
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sheet {
    pub name: String,
    pub address: String,
    pub sheets: String,
    pub rows: Vec<Vec<String>>,
}

impl Sheet {
    pub fn describe(&self) -> String {
        let mut out = format!(
            "Sheet {} ({}), sheets: {}\n",
            self.name, self.address, self.sheets
        );
        for r in &self.rows {
            out.push_str(&r.join("\t"));
            out.push('\n');
        }
        out
    }
}

pub fn parse_sheet(out: &str) -> Sheet {
    let mut sheet = Sheet::default();
    for line in out.lines() {
        if let Some(head) = line.strip_prefix("#sheet\t") {
            let mut parts = head.split('\t');
            sheet.name = parts.next().unwrap_or_default().to_owned();
            sheet.address = parts.next().unwrap_or_default().to_owned();
            sheet.sheets = parts.next().unwrap_or_default().to_owned();
        } else if !sheet.name.is_empty() && !line.starts_with('#') {
            sheet
                .rows
                .push(line.split('\t').map(|c| c.trim().to_owned()).collect());
        }
    }
    // Trailing empty rows say nothing.
    while sheet
        .rows
        .last()
        .is_some_and(|r| r.iter().all(String::is_empty))
    {
        sheet.rows.pop();
    }
    sheet
}

/// Cells as shown in Excel, read-only (the file is not changed).
pub fn excel_read(path: &str, sheet: &str, range: &str) -> Result<Sheet, ActionError> {
    need_windows()?;
    let out = powershell(
        EXCEL_READ,
        &[
            ("SK_PATH", existing(path)?),
            ("SK_SHEET", sheet),
            ("SK_RANGE", range),
            ("SK_MAXR", &MAX_ROWS.to_string()),
            ("SK_MAXC", &MAX_COLS.to_string()),
        ],
    )?;
    check(&out, "Excel")?;
    Ok(parse_sheet(&out))
}

/// Fills cells from `values` (rows by line, cells by tab) starting at the
/// first cell of `range`, then saves the workbook.
pub fn excel_write(
    path: &str,
    sheet: &str,
    range: &str,
    values: &str,
) -> Result<Outcome, ActionError> {
    need_windows()?;
    if range.trim().is_empty() || values.is_empty() {
        return Err(ActionError::Invalid(
            "say where (a cell like B2) and what to write".into(),
        ));
    }
    let out = powershell(
        EXCEL_WRITE,
        &[
            ("SK_PATH", existing(path)?),
            ("SK_SHEET", sheet),
            ("SK_RANGE", range),
            ("SK_VALUES", values),
        ],
    )?;
    check(&out, "Excel")?;
    Ok(Outcome {
        message: format!("Wrote from {range} and saved"),
        path: Some(path.to_owned()),
    })
}

/// A new Word document (or PDF, by the extension) with `text`. Never
/// replaces a file that is already there.
pub fn word_create(path: &str, text: &str) -> Result<Outcome, ActionError> {
    need_windows()?;
    let p = Path::new(path);
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_lowercase();
    if !p.is_absolute() || !matches!(ext.as_str(), "docx" | "pdf") {
        return Err(ActionError::Invalid(
            "give a full path ending in .docx or .pdf".into(),
        ));
    }
    if p.exists() {
        return Err(ActionError::Invalid(format!("{path} already exists")));
    }
    let out = powershell(
        WORD,
        &[("SK_PATH", path), ("SK_TEXT", text), ("SK_SRC", "")],
    )?;
    check(&out, "Word")?;
    Ok(Outcome {
        message: format!(
            "Made {}",
            p.file_name().unwrap_or_default().to_string_lossy()
        ),
        path: Some(path.to_owned()),
    })
}

/// A PDF of a Word document, next to it.
pub fn to_pdf(src: &str) -> Result<Outcome, ActionError> {
    need_windows()?;
    let dest = Path::new(existing(src)?).with_extension("pdf");
    if dest.exists() {
        return Err(ActionError::Invalid(format!(
            "{} already exists",
            dest.display()
        )));
    }
    let dest = dest.display().to_string();
    let out = powershell(
        WORD,
        &[("SK_PATH", &dest), ("SK_TEXT", ""), ("SK_SRC", src)],
    )?;
    check(&out, "Word")?;
    Ok(Outcome {
        message: "Saved as PDF".into(),
        path: Some(dest),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_sheet_output() {
        let s = parse_sheet(
            "noise\n#sheet\tInvoices\tA1:C3\tInvoices, Notes\nClient\tAmount\tPaid\nAcme\t1,200\tyes\n\t\t\n",
        );
        assert_eq!(s.name, "Invoices");
        assert_eq!(s.address, "A1:C3");
        assert_eq!(s.rows.len(), 2, "the empty last row is dropped");
        assert_eq!(s.rows[1], ["Acme", "1,200", "yes"]);
        assert!(
            s.describe()
                .starts_with("Sheet Invoices (A1:C3), sheets: Invoices, Notes\n")
        );
    }

    #[test]
    fn explains_missing_office() {
        assert!(check("#ok", "Excel").is_ok());
        let e = check(
            "#error Retrieving the COM class factory failed 80040154",
            "Excel",
        );
        assert!(e.unwrap_err().to_string().contains("not installed"));
    }
}
