//! Native desktop notifications use this app's bundle identity and icon.
//! Conversion code can request one through its existing host event callback;
//! a notification failure never changes the delivered result.
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Listener};
use tauri_plugin_notification::NotificationExt;

const EVENT: &str = "host-native-notification";

#[derive(Deserialize)]
struct Notice {
    title: String,
    body: String,
}

pub fn queue(emit: &(impl Fn(&str, Value) + Send + Sync), title: &str, body: &str) {
    emit(EVENT, json!({"title": title, "body": body}));
}

pub fn show(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}

pub fn listen(app: &AppHandle) {
    let handle = app.clone();
    app.listen(EVENT, move |event| {
        if let Ok(notice) = serde_json::from_str::<Notice>(event.payload()) {
            show(&handle, &notice.title, &notice.body);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queues_a_notification_without_shell_escaping_or_conversion_side_effects() {
        let events = std::sync::Mutex::new(Vec::new());
        queue(&|name, value| events.lock().unwrap().push((name.to_string(), value)), "Theme \"ready\"", "site: ZIP in Exports");
        let queued = events.lock().unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].0, EVENT);
        assert_eq!(queued[0].1["title"], "Theme \"ready\"");
        assert_eq!(queued[0].1["body"], "site: ZIP in Exports");
    }
}
