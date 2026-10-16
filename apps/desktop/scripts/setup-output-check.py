"""Exercise the generated capture wrapper with harmless commands, never installs."""
import json
from pathlib import Path
import re
import subprocess
import tempfile
import time

root = Path(__file__).resolve().parents[3]
source = (root / "apps/desktop/src-tauri/src/setup_progress.rs").read_text(encoding="utf-8")
literal = re.search(r'let script = format!\(\s*("(?:[^"\\]|\\.)*")', source).group(1)
template = json.loads(literal).replace("{{", "{").replace("}}", "}").replace("{id}", "setup-test")
cases = [
    ("cmd.exe /c 'echo ordinary warning 1>&2 & echo useful progress & exit /b 0'", 0),
    ("cmd.exe /c 'echo explicit failure 1>&2 & exit /b 9'", 9),
    ("throw 'explicit PowerShell failure'", 1),
    ("Write-Error 'nonterminating PowerShell failure'", 1),
    ("Write-Output '{\"status\":\"pulling layer\",\"digest\":\"a\",\"completed\":10,\"total\":20}'", 0),
]
with tempfile.TemporaryDirectory(prefix="sidekick-output-test-") as directory:
    task_directory = Path(directory).resolve()
    assert task_directory.parent == Path(tempfile.gettempdir()).resolve()
    assert task_directory.name.startswith("sidekick-output-test-")
    log = Path(directory) / "capture.log"
    script = Path(directory) / "capture.ps1"
    for command, code in cases:
        log.write_text("", encoding="utf-8")
        rendered = template.replace("{quoted_path}", str(log).replace("'", "''")).replace("{command}", command)
        script.write_text(rendered, encoding="utf-8-sig")
        result = subprocess.run(
            ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(script)],
            capture_output=True, text=True, creationflags=subprocess.CREATE_NO_WINDOW,
        )
        assert result.returncode == 0, result.stderr
        output = log.read_text(encoding="utf-8")
        assert output.startswith("DUSKIE_START"), output
        assert output.rstrip().endswith(f"DUSKIE_RESULT:setup-test:{code}"), output
        if "ordinary warning" in command:
            assert "ordinary warning" in output and "useful progress" in output
        if "digest" in command:
            assert '"completed":10' in output
    log.write_text("", encoding="utf-8")
    command = "Write-Output 'DUSKIE_RESULT:0'; Write-Output 'Downloading test bytes'; Start-Sleep -Milliseconds 2500; Write-Output 'Test complete'"
    script.write_text(template.replace("{quoted_path}", str(log).replace("'", "''")).replace("{command}", command), encoding="utf-8-sig")
    with subprocess.Popen(
        ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(script)],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, creationflags=subprocess.CREATE_NO_WINDOW,
    ) as process:
        deadline = time.monotonic() + 5
        while "Downloading test bytes" not in log.read_text(encoding="utf-8") and time.monotonic() < deadline:
            time.sleep(0.02)
        assert "Downloading test bytes" in log.read_text(encoding="utf-8")
        assert process.poll() is None, "progress must be captured while the command is still running"
        assert "DUSKIE_RESULT:setup-test:" not in log.read_text(encoding="utf-8")
        process.communicate(timeout=8)
        assert log.read_text(encoding="utf-8").rstrip().endswith("DUSKIE_RESULT:setup-test:0")
print("PASS: setup capture streams progress before completion, preserves warnings, distinguishes errors, and captures pull JSON")
