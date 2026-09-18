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
    fn test_parse_invalid_or_non_json_returns_none() {
        assert!(parse_stream_line("").is_none());
        assert!(parse_stream_line("   ").is_none());
        assert!(parse_stream_line("Some random stdout message").is_none());
        assert!(parse_stream_line("{not valid json").is_none());
    }
}
