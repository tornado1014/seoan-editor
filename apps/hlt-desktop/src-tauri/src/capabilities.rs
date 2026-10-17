use std::collections::BTreeSet;

const DEFAULT_CAPABILITY: &str = include_str!("../capabilities/default.json");
const REQUIRED: [&str; 2] = ["core:default", "dialog:allow-open"];
const FORBIDDEN_FRAGMENTS: [&str; 5] = ["shell", "process", "http", "websocket", "fs:"];

pub fn validate_default_capability() -> Result<(), String> {
    let value: serde_json::Value =
        serde_json::from_str(DEFAULT_CAPABILITY).map_err(|error| error.to_string())?;
    let permissions = value
        .get("permissions")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "default capability permissions must be an array".to_string())?;
    let actual: BTreeSet<&str> = permissions
        .iter()
        .map(|permission| {
            permission
                .as_str()
                .ok_or_else(|| "capability permission must be a string".to_string())
        })
        .collect::<Result<_, _>>()?;
    let required: BTreeSet<&str> = REQUIRED.into_iter().collect();
    if actual != required {
        return Err(format!(
            "default capability must contain only {required:?}; found {actual:?}"
        ));
    }
    if actual.iter().any(|permission| {
        FORBIDDEN_FRAGMENTS
            .iter()
            .any(|fragment| permission.contains(fragment))
    }) {
        return Err("default capability contains a forbidden permission".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn checked_in_default_capability_is_minimal() {
        super::validate_default_capability().unwrap();
    }
}
