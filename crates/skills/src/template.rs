//! `{{field}}` templates rendered against an event payload plus derived
//! variables (human sizes, file stems, regex captures).

use std::collections::BTreeMap;

use serde_json::Value;

/// Variables available to a template.
pub type Vars = BTreeMap<String, String>;

/// Flattens a payload into template variables and adds derived ones.
pub fn vars_from(payload: &Value) -> Vars {
    let mut vars = Vars::new();
    if let Value::Object(map) = payload {
        for (k, v) in map {
            let text = match v {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                other => other.to_string(),
            };
            vars.insert(k.clone(), text);
        }
    }
    if let Some(size) = payload.get("size").and_then(Value::as_u64) {
        vars.insert("size_human".into(), human_size(size));
    }
    if let Some(name) = payload.get("name").and_then(Value::as_str) {
        let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
        vars.insert("stem".into(), stem.to_string());
    }
    vars
}

pub fn render(template: &str, vars: &Vars) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                let key = after[..end].trim();
                if let Some(v) = vars.get(key) {
                    out.push_str(v);
                }
                rest = &after[end + 2..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_known_and_derived_vars() {
        let vars =
            vars_from(&serde_json::json!({"name": "photo.jpeg", "size": 2_621_440, "port": 3000}));
        assert_eq!(
            render("{{stem}} ({{size_human}}) on {{ port }}", &vars),
            "photo (2.5 MB) on 3000"
        );
        assert_eq!(render("missing: [{{nope}}]", &vars), "missing: []");
        assert_eq!(render("open {{", &vars), "open {{");
    }

    #[test]
    fn human_sizes() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.5 KB");
    }
}
