//! Secret Redaction and Environment Isolation.

use std::collections::HashMap;

/// Redacts sensitive information such as API keys, tokens, and credentials from logs and strings.
pub struct SecretRedactor;

impl SecretRedactor {
    /// Scans a text string and replaces discovered credentials, API keys, and connection strings with `[REDACTED]`.
    pub fn redact(text: &str) -> String {
        let mut result = text.to_string();

        // 1. Redact AgentMesh API keys: am_ak_<hex>
        let mut start_idx = 0;
        while let Some(pos) = result[start_idx..].find("am_ak_") {
            let actual_pos = start_idx + pos;
            let end_pos = result[actual_pos..]
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .map(|i| actual_pos + i)
                .unwrap_or(result.len());

            let key_slice = &result[actual_pos..end_pos];
            if key_slice.len() > 10 {
                result.replace_range(actual_pos..end_pos, "[REDACTED_API_KEY]");
                start_idx = actual_pos + "[REDACTED_API_KEY]".len();
            } else {
                start_idx = end_pos;
            }
        }

        // 2. Redact passwords in connection URLs: postgres://user:password@host
        while let Some(proto_pos) = result.find("://") {
            let user_start = proto_pos + 3;
            if let Some(at_pos) = result[user_start..].find('@') {
                let user_slice = &result[user_start..user_start + at_pos];
                if let Some(colon_pos) = user_slice.find(':') {
                    let pw_start = user_start + colon_pos + 1;
                    let pw_end = user_start + at_pos;
                    result.replace_range(pw_start..pw_end, "[REDACTED]");
                }
                break;
            } else {
                break;
            }
        }

        // 3. Redact private key blocks
        if let Some(priv_start) = result.find("-----BEGIN") {
            if let Some(priv_end) = result.find("KEY-----") {
                let actual_end = priv_end + 8;
                if actual_end > priv_start {
                    result.replace_range(priv_start..actual_end, "[REDACTED_PRIVATE_KEY]");
                }
            }
        }

        result
    }

    /// Recursively redacts sensitive values in JSON payloads.
    pub fn redact_json(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::String(s) => serde_json::Value::String(Self::redact(s)),
            serde_json::Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(Self::redact_json).collect())
            }
            serde_json::Value::Object(map) => {
                let mut redacted_map = serde_json::Map::new();
                for (k, v) in map {
                    let k_lower = k.to_lowercase();
                    if k_lower.contains("password")
                        || k_lower.contains("secret")
                        || k_lower.contains("api_key")
                        || k_lower.contains("token")
                    {
                        redacted_map.insert(k.clone(), serde_json::Value::String("[REDACTED]".to_string()));
                    } else {
                        redacted_map.insert(k.clone(), Self::redact_json(v));
                    }
                }
                serde_json::Value::Object(redacted_map)
            }
            other => other.clone(),
        }
    }
}

/// Isolates execution environments for agent processes to prevent credential leakage.
pub struct SecretScoper;

impl SecretScoper {
    /// Strips coordinator internal database passwords, NATS credentials, and secrets from environment variables.
    pub fn sanitize_env_for_agent(
        source_env: &HashMap<String, String>,
        custom_task_vars: &HashMap<String, String>,
    ) -> HashMap<String, String> {
        let blocked_prefixes = [
            "DATABASE_",
            "POSTGRES_",
            "NATS_ADMIN_",
            "COORDINATOR_",
            "SECRET_",
            "PRIVATE_KEY",
        ];

        let mut safe_env = HashMap::new();

        for (k, v) in source_env {
            let k_upper = k.to_uppercase();
            let is_blocked = blocked_prefixes.iter().any(|prefix| k_upper.starts_with(prefix))
                || k_upper == "DATABASE_URL"
                || k_upper.contains("API_KEY")
                || k_upper.contains("SECRET");

            if !is_blocked {
                safe_env.insert(k.clone(), v.clone());
            }
        }

        // Insert explicit task-scoped variables
        for (k, v) in custom_task_vars {
            safe_env.insert(k.clone(), v.clone());
        }

        safe_env
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_redactor_api_key() {
        let text = "Agent connected with key am_ak_9f83a4c5d6e7f80192837465abcde1234 to coordinator";
        let redacted = SecretRedactor::redact(text);
        assert!(!redacted.contains("9f83a4c5d6e7f80192837465abcde1234"));
        assert!(redacted.contains("[REDACTED_API_KEY]"));
    }

    #[test]
    fn test_secret_redactor_database_url() {
        let text = "Connecting to postgres://agentmesh:super_secret_pw@localhost:5432/agentmesh";
        let redacted = SecretRedactor::redact(text);
        assert!(!redacted.contains("super_secret_pw"));
        assert!(redacted.contains("postgres://agentmesh:[REDACTED]@localhost:5432/agentmesh"));
    }

    #[test]
    fn test_secret_scoper_filters_internal_credentials() {
        let mut host_env = HashMap::new();
        host_env.insert("PATH".to_string(), "/usr/bin".to_string());
        host_env.insert("HOME".to_string(), "/home/user".to_string());
        host_env.insert("DATABASE_URL".to_string(), "postgres://...".to_string());
        host_env.insert("POSTGRES_PASSWORD".to_string(), "secret123".to_string());
        host_env.insert("COORDINATOR_SECRET".to_string(), "xyz".to_string());

        let mut task_vars = HashMap::new();
        task_vars.insert("TASK_ID".to_string(), "123".to_string());

        let sanitized = SecretScoper::sanitize_env_for_agent(&host_env, &task_vars);

        assert!(sanitized.contains_key("PATH"));
        assert!(sanitized.contains_key("HOME"));
        assert!(sanitized.contains_key("TASK_ID"));
        assert!(!sanitized.contains_key("DATABASE_URL"));
        assert!(!sanitized.contains_key("POSTGRES_PASSWORD"));
        assert!(!sanitized.contains_key("COORDINATOR_SECRET"));
    }
}
