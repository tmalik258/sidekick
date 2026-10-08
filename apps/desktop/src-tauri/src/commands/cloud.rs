//! Cloud models reached with an API key: Gemini and Groq (free tiers) and
//! OpenRouter. Keys live in Credential Manager, never in the settings file.

use serde::Serialize;
use serde_json::Value;
use sidekick_ai::OpenAiCompat;
use sidekick_core::{AiSettings, CloudPref, Settings};
use tauri::{AppHandle, Manager};

use super::{CmdResult, apply_settings, off_ui};
use crate::ai::key_name;
use crate::state::{AppState, lock};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudKeys {
    gemini: bool,
    groq: bool,
    openrouter: bool,
}

fn pref_mut<'a>(ai: &'a mut AiSettings, id: &str) -> Option<&'a mut CloudPref> {
    match id {
        "gemini" => Some(&mut ai.gemini),
        "groq" => Some(&mut ai.groq),
        "openrouter" => Some(&mut ai.openrouter),
        _ => None,
    }
}

/// Which cloud providers have a key saved.
#[tauri::command]
pub async fn cloud_keys() -> CmdResult<CloudKeys> {
    let has = |id: &str| crate::secrets::get(&key_name(id)).is_some();
    off_ui(move || CloudKeys {
        gemini: has("gemini"),
        groq: has("groq"),
        openrouter: has("openrouter"),
    })
    .await
}

/// Checks the key with the provider, saves it and switches the provider on.
#[tauri::command]
pub async fn cloud_key_set(app: AppHandle, id: String, key: String) -> CmdResult<Settings> {
    let key = key.trim().to_owned();
    if key.is_empty() {
        return Err("Paste the key first.".into());
    }
    let model = OpenAiCompat::cloud(&id, &key, None).ok_or("Unknown provider.")?;
    if model.models().await.is_none() {
        return Err("That key did not work. Check it was copied in full.".into());
    }
    let name = key_name(&id);
    off_ui(move || crate::secrets::set(&name, &key)).await??;
    let mut next = lock(&app.state::<AppState>().settings).clone();
    if let Some(pref) = pref_mut(&mut next.ai, &id) {
        pref.enabled = true;
    }
    apply_settings(&app, next)
}

/// Forgets the key and switches the provider off.
#[tauri::command]
pub async fn cloud_key_clear(app: AppHandle, id: String) -> CmdResult<Settings> {
    let name = key_name(&id);
    off_ui(move || crate::secrets::delete(&name)).await?;
    let mut next = lock(&app.state::<AppState>().settings).clone();
    if let Some(pref) = pref_mut(&mut next.ai, &id) {
        pref.enabled = false;
    }
    apply_settings(&app, next)
}

/// One model OpenRouter offers, with its price in US dollars per million
/// tokens (0 for free ones).
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RouterModel {
    id: String,
    name: String,
    free: bool,
    input: f64,
    output: f64,
    context: u64,
    tools: bool,
}

fn router_models(v: &Value) -> Vec<RouterModel> {
    let per_million = |p: &Value| {
        p.as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .or_else(|| p.as_f64())
            .unwrap_or(0.0)
            * 1_000_000.0
    };
    let mut out: Vec<RouterModel> = v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m["id"].as_str()?.to_owned();
            let input = per_million(&m["pricing"]["prompt"]);
            let output = per_million(&m["pricing"]["completion"]);
            Some(RouterModel {
                name: m["name"].as_str().unwrap_or(&id).to_owned(),
                // Negative prices mean "depends on the model it picks".
                free: id.ends_with(":free") || (input == 0.0 && output == 0.0),
                input,
                output,
                context: m["context_length"].as_u64().unwrap_or(0),
                tools: m["supported_parameters"]
                    .as_array()
                    .is_some_and(|p| p.iter().any(|x| x == "tools")),
                id,
            })
        })
        .collect();
    // Models that can use Sidekick's tools first, then by name.
    out.sort_by(|a, b| b.tools.cmp(&a.tools).then_with(|| a.name.cmp(&b.name)));
    out
}

/// Every model OpenRouter offers (no key needed to list them).
#[tauri::command]
pub async fn openrouter_models() -> CmdResult<Vec<RouterModel>> {
    let v: Value = reqwest::Client::new()
        .get("https://openrouter.ai/api/v1/models")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("could not reach OpenRouter: {e}"))?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(router_models(&v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_openrouter_models_and_prices() {
        let v = serde_json::json!({ "data": [
            { "id": "meta-llama/llama-3.3-70b-instruct:free", "name": "Llama 3.3 70B (free)",
              "pricing": { "prompt": "0", "completion": "0" }, "context_length": 131072,
              "supported_parameters": ["tools", "temperature"] },
            { "id": "anthropic/claude-sonnet", "name": "Claude Sonnet",
              "pricing": { "prompt": "0.000003", "completion": "0.000015" }, "context_length": 200000,
              "supported_parameters": ["tools"] },
            { "id": "x/no-tools", "name": "A no tools", "pricing": { "prompt": "0.000001", "completion": "0.000001" } }
        ]});
        let m = router_models(&v);
        assert_eq!(m.len(), 3);
        assert_eq!(m[2].id, "x/no-tools");
        let claude = m
            .iter()
            .find(|x| x.id == "anthropic/claude-sonnet")
            .unwrap();
        assert!(!claude.free);
        assert!((claude.input - 3.0).abs() < 1e-9 && (claude.output - 15.0).abs() < 1e-9);
        assert!(m.iter().any(|x| x.free && x.tools));
    }
}
