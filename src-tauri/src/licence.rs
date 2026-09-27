use crate::{model::*, store::Store};
use serde_json::{json, Value};
use std::time::Duration;

pub const DEFAULT_API: &str = "https://api.html2wp.dev";

/// Developer override for testing against a local html2wp server: set
/// H2WP_API before starting the app (for example http://127.0.0.1:8080).
/// Only https, or plain http to this computer, is accepted; anything else
/// keeps the production service. Released builds are used without it.
pub fn api_base() -> String {
    api_from(std::env::var("H2WP_API").ok().as_deref())
}

fn api_from(value: Option<&str>) -> String {
    let Some(value) = value.map(|v| v.trim().trim_end_matches('/')).filter(|v| !v.is_empty()) else { return DEFAULT_API.into() };
    match url::Url::parse(value) {
        Ok(u) if u.username().is_empty() && u.password().is_none() && u.query().is_none() && u.fragment().is_none()
            && (u.scheme() == "https" && u.host_str().is_some()
                || u.scheme() == "http" && matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]" | "host.docker.internal"))) => value.into(),
        _ => {
            eprintln!("Ignoring H2WP_API={value}: use https, or http to this computer");
            DEFAULT_API.into()
        }
    }
}

/// The overridden service as the service container is given it. A loopback
/// URL stays unchanged: the local server hands out upload and download URLs
/// on that same origin, so service.py forwards the container's loopback port
/// to the host. Other plain http needs H2WP_ALLOW_INSECURE_API=1 there.
/// Returns (url, insecure_http), or None for the production service.
pub fn container_api() -> Option<(String, bool)> {
    container_api_from(&api_base())
}

fn container_api_from(api: &str) -> Option<(String, bool)> {
    if api == DEFAULT_API { return None; }
    let u = url::Url::parse(api).ok()?;
    let loopback = matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    Some((api.trim_end_matches('/').to_string(), u.scheme() == "http" && !loopback))
}

// Only display fields cross the native boundary. Never return the raw response,
// request headers, or the saved key to JavaScript or a model.
pub fn display(payload: &Value, has_key: bool) -> Value {
    let licence = payload
        .get("licence")
        .or_else(|| payload.get("license"))
        .unwrap_or(&Value::Null);
    let text = |v: &Value| v.as_str().map(|s| s.chars().take(2000).collect::<String>());
    let expiry = text(&licence["expiresAt"]).or_else(|| text(&payload["expiresAt"]));
    let status = text(&licence["status"]).or_else(|| text(&payload["status"]));
    let valid = licence["valid"]
        .as_bool()
        .or_else(|| payload["valid"].as_bool());
    let state = if !has_key {
        "free"
    } else if valid == Some(false) || status.as_deref() == Some("invalid") {
        "invalid"
    } else if status.as_deref() == Some("expired") {
        "expired"
    } else if valid == Some(true) || status.as_deref() == Some("active") {
        "active"
    } else {
        "unconfirmed"
    };
    json!({"mode":if has_key{"licensed"}else{"free"},"state":state,"checkedAt":now(),
        "expiresAt":expiry,"neverExpires":licence["neverExpires"]==true,
        "plan":text(&licence["plan"]),"creditLine":text(&payload["credit"]["line"]),"note":text(&payload["note"]),
        "message":if has_key&&state=="unconfirmed"{"The service returned allowance information without an explicit licence validity status."}else{"Allowance checked with html2wp."}})
}

pub async fn check(store: &Store) -> Result<Value> {
    let path = store.root.join("private/licence");
    let key = std::fs::read_to_string(path).unwrap_or_default();
    let has_key = !key.trim().is_empty();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(err)?;
    let mut request = client
        .get(format!("{}/v1/allowance", api_base()))
        .header("x-html2wp-host", "codex");
    if has_key {
        request = request.header("x-html2wp-key", key.trim());
    }
    let result = async {
        let response = request.send().await.map_err(|_| "Cannot reach html2wp. Your saved licence has been kept; try again when online.".to_string())?;
        let status=response.status();
        if status.as_u16()==401 || status.as_u16()==403 {
            return Ok(json!({"mode":if has_key{"licensed"}else{"free"},"state":"rejected","checkedAt":now(),"message":"The service rejected this allowance request. Check your licence key or contact html2wp.","expiresAt":null}));
        }
        if !status.is_success(){return Err(format!("html2wp allowance is unavailable (HTTP {}). Try again later.",status.as_u16()));}
        let payload:Value=response.json().await.map_err(|_|"html2wp returned an unreadable allowance response".to_string())?;
        Ok(display(&payload,has_key))
    }.await;
    match result {
        Ok(v) => {
            store.set("licence-status", &v.to_string())?;
            Ok(v)
        }
        Err(e) => Ok(
            json!({"mode":if has_key{"licensed"}else{"free"},"state":"unavailable","checkedAt":now(),"message":e,"expiresAt":null,
            "lastKnown":store.setting("licence-status").and_then(|s|serde_json::from_str::<Value>(&s).ok())}),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn does_not_invent_validity() {
        let v = display(
            &json!({"credit":{"line":"Your current allowance"},"secret":"never-return"}),
            true,
        );
        assert_eq!(v["state"], "unconfirmed");
        assert!(v["expiresAt"].is_null());
        assert!(v.get("secret").is_none());
    }
    #[test]
    fn service_override_is_local_or_https_only_and_reaches_the_host_from_containers() {
        assert_eq!(api_from(None), DEFAULT_API);
        assert_eq!(api_from(Some("")), DEFAULT_API);
        assert_eq!(api_from(Some("http://127.0.0.1:8080/")), "http://127.0.0.1:8080");
        assert_eq!(api_from(Some("https://staging.html2wp.dev")), "https://staging.html2wp.dev");
        for rejected in ["http://staging.example.com", "ftp://127.0.0.1", "https://user:pw@api.example", "https://api.example/?k=1", "not a url"] {
            assert_eq!(api_from(Some(rejected)), DEFAULT_API, "{rejected}");
        }
        assert_eq!(container_api_from(DEFAULT_API), None);
        assert_eq!(container_api_from("http://127.0.0.1:8080"), Some(("http://127.0.0.1:8080".into(), false)));
        assert_eq!(container_api_from("http://localhost:8080"), Some(("http://localhost:8080".into(), false)));
        assert_eq!(container_api_from("http://host.docker.internal:8080"), Some(("http://host.docker.internal:8080".into(), true)));
        assert_eq!(container_api_from("https://staging.html2wp.dev"), Some(("https://staging.html2wp.dev".into(), false)));
    }
    #[test]
    fn free_and_expiry_are_explicit() {
        assert_eq!(display(&json!({}), false)["state"], "free");
        let v = display(
            &json!({"licence":{"status":"active","expiresAt":"2026-10-20"}}),
            true,
        );
        assert_eq!(v["expiresAt"], "2026-10-20");
        assert_eq!(v["state"], "active");
    }
}
