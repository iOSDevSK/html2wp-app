//! The immutable Linux runtime selected for this desktop processor.
use serde_json::Value;

pub fn selected(release: &Value) -> Result<&Value, String> {
    let arch = std::env::consts::ARCH;
    let entry = release["platforms"][arch].as_object()
        .ok_or_else(|| format!("No published Linux runtime for {arch}; update the app before preparing the environment."))?;
    if entry.get("architecture").and_then(Value::as_str) != Some(arch) {
        return Err("Runtime manifest architecture does not match this processor".into());
    }
    Ok(&release["platforms"][arch])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn requires_the_exact_processor_entry() {
        let arch = std::env::consts::ARCH;
        assert!(selected(&json!({"platforms":{arch:{"architecture":arch}}})).is_ok());
        assert!(selected(&json!({"platforms":{arch:{"architecture":"other"}}})).is_err());
        assert!(selected(&json!({"platforms":{}})).is_err());
    }
}
