use serde::Serialize;
use sha2::{Digest, Sha256};

pub fn operation_plan_token(kind: &str, value: &impl Serialize) -> Result<String, String> {
    let payload = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    let mut hash = StableHash::new();
    hash.update(kind.as_bytes());
    hash.update(&[0]);
    hash.update(&payload);
    Ok(format!("{}-sha256-{}", kind, hash.finish()))
}

pub fn verify_operation_plan(expected: &str, provided: Option<&str>) -> Result<(), String> {
    let provided = provided
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "操作计划缺失，请重新预览".to_string())?;
    if provided == expected {
        Ok(())
    } else {
        Err("操作计划已过期或内容已变化，请重新预览".to_string())
    }
}

#[derive(Clone)]
pub struct StableHash(Sha256);

impl StableHash {
    pub fn new() -> Self {
        Self(Sha256::new())
    }

    pub fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    pub fn finish(&self) -> String {
        format!("{:x}", self.0.clone().finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_stable_and_changes_with_payload() {
        let first = operation_plan_token("install", &vec!["a", "b"]).unwrap();
        let second = operation_plan_token("install", &vec!["a", "b"]).unwrap();
        let changed = operation_plan_token("install", &vec!["a", "c"]).unwrap();

        assert_eq!(first, second);
        assert_ne!(first, changed);
        assert!(verify_operation_plan(&first, Some(&second)).is_ok());
        assert!(verify_operation_plan(&first, Some(&changed)).is_err());
        assert!(verify_operation_plan(&first, None).is_err());
    }
}
