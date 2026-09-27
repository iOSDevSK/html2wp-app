use crate::{codex::Rpc, model::*};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexModel {
    pub id: String,
    pub model: String,
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub supported_reasoning_efforts: Vec<ReasoningEffortOption>,
    #[serde(default)]
    pub default_reasoning_effort: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningEffortOption {
    pub reasoning_effort: String,
    pub description: String,
}

pub fn effort_key(model: &str) -> String {
    format!("model-effort:{model}")
}

/// The model and effort a project runs with: its own choice when it has
/// one, otherwise the default from Settings. The effort saved on the project
/// only counts for the model it was chosen with.
pub fn project_choice(store: &crate::store::Store, p: &Project) -> (String, String) {
    let model = p
        .model
        .clone()
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| store.setting("selected-model").unwrap_or_default());
    let effort = p
        .effort
        .clone()
        .filter(|e| !e.is_empty() && p.model.as_deref() == Some(model.as_str()))
        .or_else(|| store.setting(&effort_key(&model)))
        .unwrap_or_default();
    (model, effort)
}

pub fn resolve_effort(models: &[CodexModel], model: &str, selected: &str) -> Result<String> {
    let details = models
        .iter()
        .find(|m| m.model == model)
        .ok_or("Choose an available model first")?;
    let effort = if selected.is_empty() {
        &details.default_reasoning_effort
    } else {
        selected
    };
    if effort.is_empty()
        || !details
            .supported_reasoning_efforts
            .iter()
            .any(|option| option.reasoning_effort == effort)
    {
        return Err("The reasoning effort is unavailable for this model. Choose a supported effort in Settings.".into());
    }
    Ok(effort.to_string())
}

pub fn apply_effort(params: &mut Value, effort: &str, turn: bool) {
    if turn {
        params["effort"] = json!(effort);
    } else {
        if !params["config"].is_object() {
            params["config"] = json!({});
        }
        params["config"]["model_reasoning_effort"] = json!(effort);
    }
}

pub fn visible_models(page: &Value) -> Result<Vec<CodexModel>> {
    let models: Vec<CodexModel> = serde_json::from_value(
        page.get("data")
            .ok_or("Codex returned no model catalog")?
            .clone(),
    )
    .map_err(err)?;
    Ok(models
        .into_iter()
        .filter(|m| !m.hidden && !m.model.trim().is_empty())
        .collect())
}

pub async fn catalog(rpc: &Rpc) -> Result<Vec<CodexModel>> {
    let mut models = Vec::new();
    let mut cursor: Option<String> = None;
    let mut cursors = HashSet::new();
    let mut seen = HashSet::new();
    for _ in 0..20 {
        let page = rpc
            .request(
                "model/list",
                json!({"limit":100,"includeHidden":false,"cursor":cursor}),
            )
            .await?;
        for model in visible_models(&page)? {
            if seen.insert(model.model.clone()) {
                models.push(model);
            }
        }
        cursor = page["nextCursor"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        match &cursor {
            None => return Ok(models),
            Some(value) => {
                if !cursors.insert(value.clone()) {
                    return Err(
                        "Codex repeated a model catalog page. Refresh the models and try again."
                            .into(),
                    );
                }
            }
        }
    }
    Err("Codex model catalog exceeded the page limit".into())
}

pub fn resolve(models: &[CodexModel], selected: &str) -> Result<String> {
    let model = if selected.is_empty() {
        models.iter().find(|m| m.is_default)
    } else {
        models.iter().find(|m| m.model == selected)
    };
    model.map(|m|m.model.clone()).ok_or_else(||if selected.is_empty(){"Codex did not report a default model. Choose a model in Settings.".into()}else{"Your selected model is no longer in the Codex catalog. Choose another model in Settings.".into()})
}

pub fn apply_selection(params: &mut Value, model: &str) {
    params["model"] = json!(model);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<CodexModel> {
        visible_models(&json!({"data":[{"id":"catalog-a","model":"model-a","displayName":"A","isDefault":true},{"id":"catalog-b","model":"model-b","displayName":"B"},{"id":"internal","model":"hidden","displayName":"Hidden","hidden":true}]})).unwrap()
    }
    #[test]
    fn catalog_preserves_server_names_and_hides_internal_models() {
        let m = fixture();
        assert_eq!(m.len(), 2);
        assert_eq!(resolve(&m, "model-b").unwrap(), "model-b");
        assert!(resolve(&m, "catalog-b").is_err());
        assert!(resolve(&m, "hidden").is_err());
    }
    #[test]
    fn default_is_resolved_so_resume_cannot_keep_an_old_selection() {
        let m = fixture();
        let mut params = json!({"threadId":"saved-thread","model":"model-b"});
        apply_selection(&mut params, &resolve(&m, "").unwrap());
        assert_eq!(params["model"], "model-a");
        assert_eq!(params["threadId"], "saved-thread");
    }
    #[test]
    fn missing_model_never_silently_falls_back() {
        assert!(resolve(&fixture(), "removed-model").is_err());
        assert!(resolve(&[], "").is_err());
    }

    fn effort_fixture() -> Vec<CodexModel> {
        visible_models(&json!({"data":[
            {"id":"a","model":"model-a","displayName":"A","defaultReasoningEffort":"low","supportedReasoningEfforts":[{"reasoningEffort":"low","description":"Quick"},{"reasoningEffort":"high","description":"Thorough"}]},
            {"id":"b","model":"model-b","displayName":"B","defaultReasoningEffort":"medium","supportedReasoningEfforts":[{"reasoningEffort":"medium","description":"Balanced"}]}
        ]})).unwrap()
    }

    #[test]
    fn effort_uses_the_catalog_and_rejects_incompatible_choices() {
        let catalog = effort_fixture();
        assert_eq!(resolve_effort(&catalog, "model-a", "").unwrap(), "low");
        assert_eq!(resolve_effort(&catalog, "model-a", "high").unwrap(), "high");
        assert_eq!(resolve_effort(&catalog, "model-b", "").unwrap(), "medium");
        assert!(resolve_effort(&catalog, "model-b", "high").is_err());
        assert!(resolve_effort(&catalog, "removed", "").is_err());
    }

    #[test]
    fn default_effort_resets_both_resumed_thread_and_next_turn() {
        let effort = resolve_effort(&effort_fixture(), "model-a", "").unwrap();
        let mut thread = json!({"threadId":"saved","config":{"model_reasoning_effort":"high","other":"preserved"}});
        apply_effort(&mut thread, &effort, false);
        assert_eq!(thread["config"]["model_reasoning_effort"], "low");
        assert_eq!(thread["config"]["other"], "preserved");
        let mut turn = json!({"threadId":"saved","effort":"high"});
        apply_effort(&mut turn, &effort, true);
        assert_eq!(turn["effort"], "low");
        assert_eq!(turn["threadId"], "saved");
    }

    #[test]
    fn saved_efforts_survive_switching_models_independently() {
        let root = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(root.path().into()).unwrap();
        store.set(&effort_key("model-a"), "high").unwrap();
        store.set(&effort_key("model-b"), "medium").unwrap();
        store.set(&effort_key("model-b"), "").unwrap();
        assert_eq!(
            store.setting(&effort_key("model-a")).as_deref(),
            Some("high")
        );
        assert_eq!(store.setting(&effort_key("model-b")).as_deref(), Some(""));
    }
}
