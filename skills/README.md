# Skills

Built-in skills, one YAML file each (SRS 9.1). They are compiled into the app; skills in `%APPDATA%/dev.sidekick.app/skills` load on top and override built-ins with the same id.

```yaml
id: dev.open-in-browser        # unique
name: Open dev servers         # shown in settings
priority: 60                   # higher wins the title when skills merge
cooldown_secs: 45              # same dedupe key stays quiet this long
dedupe: "{{port}}"             # cooldown key template
remember: "dev:{{port}}"       # learn which option you pick here
trust: suggest                 # or auto: run the first safe option at once
enabled_by_default: true
trigger:
  event: port.listening        # event kind from a sensor
  where:                       # every test must pass
    process: { one_of: [node, python] }
    port: { range: [1024, 65535] }
    text: { regex: "(?P<port>\\d+)" }   # named groups become {{port}}
suggestion:
  title: "Server on port {{port}}"
  detail: "{{process}} is serving {{url}}"
  options:
    - label: Chrome
      action: open_url
      args: { url: "{{url}}", browser: chrome }
      requires: ["browser:chrome"]    # hidden when Chrome is not installed
      when: { port: { not_one_of: [9229] } }
```

Actions: `open_path`, `reveal_path`, `copy_file`, `copy_text`, `open_url`, `convert`, `extract_archive`, `run_installer`, `kill_port`, `clear_clipboard_later`, `format_json_clipboard`.

Capabilities for `requires`: `browser:chrome|edge|firefox|zen|brave`, `tool:ffmpeg`, `tool:image` (ImageMagick or ffmpeg), `tool:soffice`, `tool:pandoc`, `tool:tar`.
