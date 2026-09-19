//! Secret Redaction and Environment Isolation.

use std::collections::HashMap;

pub use agent_protocol::security::SecretRedactor;

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
