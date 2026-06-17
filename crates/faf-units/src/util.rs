use serde::{Deserialize, Deserializer};

/// Deserializer that accepts either a JSON boolean or an integer (0/1) as bool.
pub fn deserialize_bool_or_int<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Bool(b) => Ok(Some(b)),
        serde_json::Value::Number(n) => {
            let b = n.as_i64().map(|i| i != 0).unwrap_or(false);
            Ok(Some(b))
        }
        serde_json::Value::Null => Ok(None),
        _ => Err(serde::de::Error::custom("expected bool or integer")),
    }
}
