use serde::{Deserialize, Serialize};

/// Parsed event emitted by `agy --output-format stream-json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AgyStreamEvent {
    Init {
        #[serde(default)]
        conversation_id: Option<String>,
        #[serde(default)]
        init: Option<AgyInitPayload>,
    },
    StepUpdate {
        step_update: AgyStepUpdatePayload,
    },
    Result {
        result: AgyResultPayload,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AgyInitPayload {
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub permission_mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AgyStepUpdatePayload {
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub step_index: Option<u64>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub step_type: Option<String>,
    #[serde(default)]
    pub text_delta: Option<String>,
    #[serde(default)]
    pub duration_seconds: Option<f64>,
}

impl AgyStepUpdatePayload {
    /// Returns true if this step update indicates that the agent is blocked or waiting for input/permission.
    pub fn is_blocked(&self) -> bool {
        let state_blocked = self
            .state
            .as_deref()
            .map(|s| {
                let lower = s.to_lowercase();
                lower == "blocked" || lower == "waiting_for_input" || lower == "waiting_for_message"
            })
            .unwrap_or(false);

        let step_type_blocked = self
            .step_type
            .as_deref()
            .map(|st| {
                let lower = st.to_lowercase();
                lower == "ask_question" || lower == "ask_permission" || lower == "blocked"
            })
            .unwrap_or(false);

        state_blocked || step_type_blocked
    }

    /// Extracts the blocking reason or question if available.
    pub fn blocked_reason(&self) -> String {
        if let Some(ref text) = self.text_delta {
            if !text.trim().is_empty() {
                return format!("Agent waiting for response: {}", text.trim());
            }
        }
        if let Some(ref st) = self.step_type {
            return format!("Agent entered blocked state on step type: {st}");
        }
        "Agent is waiting for external input or permission".to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AgyResultPayload {
    #[serde(default)]
    pub conversation_id: Option<String>,
    pub status: String,
    #[serde(default)]
    pub response: Option<String>,
    #[serde(default)]
    pub duration_seconds: Option<f64>,
    #[serde(default)]
    pub num_turns: Option<u64>,
}

/// Helper function to parse a single line of NDJSON from `agy`.
/// If the line cannot be parsed as a known `AgyStreamEvent`, returns `None`.
pub fn parse_stream_line(line: &str) -> Option<AgyStreamEvent> {
    let trimmed = line.trim();
    if trimmed.is_empty() || !trimmed.starts_with('{') {
        return None;
    }
    serde_json::from_str::<AgyStreamEvent>(trimmed).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_init_event() {
        let raw = r#"{"event":"init","conversation_id":"8a6e3b0f-2ee1-4c48-a19b-52e0f71db1c2","init":{"cwd":"/media/test","tools":["read_file","run_command"],"permission_mode":"always-proceed"}}"#;
        let event = parse_stream_line(raw).expect("Failed to parse init event");
        match event {
            AgyStreamEvent::Init { conversation_id, init } => {
                assert_eq!(conversation_id.as_deref(), Some("8a6e3b0f-2ee1-4c48-a19b-52e0f71db1c2"));
                let init = init.expect("Init payload missing");
                assert_eq!(init.permission_mode.as_deref(), Some("always-proceed"));
                assert_eq!(init.tools.len(), 2);
            }
            other => panic!("Expected Init, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_step_update_event() {
        let raw = r#"{"event":"step_update","step_update":{"conversation_id":"8a6e3b0f-2ee1-4c48-a19b-52e0f71db1c2","step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":"Compiling..."}}"#;
        let event = parse_stream_line(raw).expect("Failed to parse step update");
        match event {
            AgyStreamEvent::StepUpdate { step_update } => {
                assert_eq!(step_update.step_index, Some(1));
                assert_eq!(step_update.state.as_deref(), Some("ACTIVE"));
                assert_eq!(step_update.text_delta.as_deref(), Some("Compiling..."));
            }
            other => panic!("Expected StepUpdate, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_result_success_event() {
        let raw = r#"{"event":"result","result":{"conversation_id":"8a6e3b0f-2ee1-4c48-a19b-52e0f71db1c2","status":"SUCCESS","response":"All tasks complete.\n","duration_seconds":6.43,"num_turns":1}}"#;
        let event = parse_stream_line(raw).expect("Failed to parse result");
        match event {
            AgyStreamEvent::Result { result } => {
                assert_eq!(result.status, "SUCCESS");
                assert_eq!(result.response.as_deref(), Some("All tasks complete.\n"));
            }
            other => panic!("Expected Result, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_blocked_step_update() {
        let raw = r#"{"event":"step_update","step_update":{"conversation_id":"8a6e3b0f-2ee1-4c48-a19b-52e0f71db1c2","step_index":2,"state":"waiting_for_input","step_type":"ask_question","text_delta":"Which database migration version should I apply?"}}"#;
        let event = parse_stream_line(raw).expect("Failed to parse blocked step update");
        match event {
            AgyStreamEvent::StepUpdate { step_update } => {
                assert!(step_update.is_blocked());
                assert!(step_update.blocked_reason().contains("Which database migration version"));
            }
            other => panic!("Expected StepUpdate, got {other:?}"),
        }
    }
}

