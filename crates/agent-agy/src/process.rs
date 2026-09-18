use std::process::Stdio;
use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::adapter::AgyAgent;
use crate::parser::{parse_stream_line, AgyResultPayload, AgyStreamEvent};

/// Events emitted during the execution lifecycle of an `agy` subprocess.
#[derive(Debug, Clone)]
pub enum AgyProcessEvent {
    /// Parsed NDJSON stream event from agy stdout (init, step_update, result).
    Stream(AgyStreamEvent),
    /// Unparsed stderr or diagnostic line.
    Stderr(String),
    /// Subprocess terminated successfully with exit code 0 and optional result payload.
    Completed {
        exit_code: i32,
        result: Option<AgyResultPayload>,
    },
    /// Subprocess failed, crashed, timed out, or exited with non-zero code.
    Failed {
        reason: String,
        exit_code: Option<i32>,
        stderr: String,
    },
}

pub struct AgyProcess;

impl AgyProcess {
    /// Spawns the `agy` CLI subprocess with the provided prompt and streams lifecycle events.
    ///
    /// The function emits events through the returned channel receiver and enforces
    /// the configured execution timeout.
    pub async fn run(
        agent: &AgyAgent,
        prompt: &str,
    ) -> Result<mpsc::Receiver<AgyProcessEvent>> {
        let (tx, rx) = mpsc::channel(100);

        let mut cmd = Command::new(&agent.agy_path);
        cmd.arg("-p")
            .arg(prompt)
            .arg("--output-format")
            .arg("stream-json");

        if agent.dangerously_skip_permissions {
            cmd.arg("--dangerously-skip-permissions");
        }

        if let Some(ref model) = agent.model {
            cmd.arg("--model").arg(model);
        }

        if let Some(ref effort) = agent.effort {
            cmd.arg("--effort").arg(effort);
        }

        if let Some(ref dir) = agent.workspace_dir {
            cmd.arg("--add-dir").arg(dir);
            cmd.current_dir(dir);
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        info!(
            binary = %agent.agy_path.display(),
            timeout_secs = agent.timeout.as_secs(),
            "Spawning agy CLI subprocess"
        );

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn agy process: {}", agent.agy_path.display()))?;

        let stdout = child
            .stdout
            .take()
            .context("Failed to capture agy child stdout")?;
        let stderr = child
            .stderr
            .take()
            .context("Failed to capture agy child stderr")?;

        let timeout_duration = agent.timeout;

        tokio::spawn(async move {
            let mut stdout_lines = BufReader::new(stdout).lines();
            let mut stderr_lines = BufReader::new(stderr).lines();

            let mut collected_stderr = Vec::new();
            let mut final_result: Option<AgyResultPayload> = None;

            let execution_future = async {
                loop {
                    tokio::select! {
                        stdout_res = stdout_lines.next_line() => {
                            match stdout_res {
                                Ok(Some(line)) => {
                                    debug!(%line, "agy stdout line");
                                    if let Some(stream_event) = parse_stream_line(&line) {
                                        if let AgyStreamEvent::Result { ref result } = stream_event {
                                            final_result = Some(result.clone());
                                        }
                                        let _ = tx.send(AgyProcessEvent::Stream(stream_event)).await;
                                    }
                                }
                                Ok(None) => break, // EOF on stdout
                                Err(e) => {
                                    warn!(error = %e, "Error reading agy stdout line");
                                    break;
                                }
                            }
                        }
                        stderr_res = stderr_lines.next_line() => {
                            match stderr_res {
                                Ok(Some(line)) => {
                                    debug!(%line, "agy stderr line");
                                    collected_stderr.push(line.clone());
                                    let _ = tx.send(AgyProcessEvent::Stderr(line)).await;
                                }
                                Ok(None) => {}, // EOF on stderr
                                Err(e) => {
                                    warn!(error = %e, "Error reading agy stderr line");
                                }
                            }
                        }
                    }
                }

                // Wait for process exit status
                child.wait().await
            };

            match tokio::time::timeout(timeout_duration, execution_future).await {
                Ok(wait_result) => match wait_result {
                    Ok(exit_status) => {
                        let code = exit_status.code().unwrap_or(-1);
                        if exit_status.success() {
                            info!(exit_code = code, "agy process finished successfully");
                            let _ = tx
                                .send(AgyProcessEvent::Completed {
                                    exit_code: code,
                                    result: final_result,
                                })
                                .await;
                        } else {
                            let stderr_joined = collected_stderr.join("\n");
                            error!(exit_code = code, stderr = %stderr_joined, "agy process exited with non-zero status");
                            let _ = tx
                                .send(AgyProcessEvent::Failed {
                                    reason: format!("agy process exited with status code {code}"),
                                    exit_code: Some(code),
                                    stderr: stderr_joined,
                                })
                                .await;
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "Failed waiting for agy process");
                        let _ = tx
                            .send(AgyProcessEvent::Failed {
                                reason: format!("Failed waiting for agy process: {e}"),
                                exit_code: None,
                                stderr: collected_stderr.join("\n"),
                            })
                            .await;
                    }
                },
                Err(_) => {
                    // Timeout elapsed: kill the child process
                    warn!(
                        timeout_secs = timeout_duration.as_secs(),
                        "agy process timed out, sending kill signal"
                    );
                    let _ = child.kill().await;
                    let _ = tx
                        .send(AgyProcessEvent::Failed {
                            reason: format!(
                                "Execution timed out after {} seconds",
                                timeout_duration.as_secs()
                            ),
                            exit_code: None,
                            stderr: collected_stderr.join("\n"),
                        })
                        .await;
                }
            }
        });

        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::Duration;
    use uuid::Uuid;

    struct TempScript {
        path: std::path::PathBuf,
    }

    impl TempScript {
        fn new(content: &str) -> Self {
            let path = std::env::temp_dir().join(format!("mock_agy_test_{}.sh", Uuid::new_v4()));
            let mut file = std::fs::File::create(&path).unwrap();
            file.write_all(content.as_bytes()).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            Self { path }
        }
    }

    impl Drop for TempScript {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    #[tokio::test]
    async fn test_process_streams_ndjson_and_completes() {
        let script = TempScript::new(
            "#!/bin/bash\n\
             echo '{\"event\":\"init\",\"conversation_id\":\"test-123\"}'\n\
             echo '{\"event\":\"step_update\",\"step_update\":{\"step_index\":1,\"text_delta\":\"working\"}}'\n\
             echo '{\"event\":\"result\",\"result\":{\"status\":\"SUCCESS\",\"response\":\"done\"}}'\n\
             exit 0",
        );

        let agent = AgyAgent::new("TestBot", "key")
            .with_agy_path(script.path.clone())
            .with_timeout(Duration::from_secs(5));

        let mut rx = AgyProcess::run(&agent, "dummy prompt").await.unwrap();

        let mut received_init = false;
        let mut received_step = false;
        let mut received_completed = false;

        while let Some(evt) = rx.recv().await {
            match evt {
                AgyProcessEvent::Stream(AgyStreamEvent::Init { conversation_id, .. }) => {
                    assert_eq!(conversation_id.as_deref(), Some("test-123"));
                    received_init = true;
                }
                AgyProcessEvent::Stream(AgyStreamEvent::StepUpdate { step_update }) => {
                    assert_eq!(step_update.step_index, Some(1));
                    received_step = true;
                }
                AgyProcessEvent::Completed { exit_code, result } => {
                    assert_eq!(exit_code, 0);
                    assert_eq!(result.map(|r| r.status), Some("SUCCESS".to_string()));
                    received_completed = true;
                }
                _ => {}
            }
        }

        assert!(received_init, "Must receive Init event");
        assert!(received_step, "Must receive StepUpdate event");
        assert!(received_completed, "Must receive Completed event");
    }

    #[tokio::test]
    async fn test_process_failure_exit_code() {
        let script = TempScript::new(
            "#!/bin/bash\n\
             >&2 echo 'Fatal CLI error'\n\
             exit 42",
        );

        let agent = AgyAgent::new("TestBot", "key")
            .with_agy_path(script.path.clone())
            .with_timeout(Duration::from_secs(5));

        let mut rx = AgyProcess::run(&agent, "dummy prompt").await.unwrap();

        let mut received_failed = false;
        while let Some(evt) = rx.recv().await {
            if let AgyProcessEvent::Failed { reason, exit_code, stderr } = evt {
                assert_eq!(exit_code, Some(42));
                assert!(reason.contains("42"));
                assert!(stderr.contains("Fatal CLI error"));
                received_failed = true;
            }
        }

        assert!(received_failed, "Must receive Failed event on non-zero exit");
    }

    #[tokio::test]
    async fn test_process_timeout_kills_child() {
        let script = TempScript::new(
            "#!/bin/bash\n\
             sleep 10\n\
             exit 0",
        );

        let agent = AgyAgent::new("TestBot", "key")
            .with_agy_path(script.path.clone())
            .with_timeout(Duration::from_millis(200));

        let mut rx = AgyProcess::run(&agent, "dummy prompt").await.unwrap();

        let mut received_timeout = false;
        while let Some(evt) = rx.recv().await {
            if let AgyProcessEvent::Failed { reason, .. } = evt {
                if reason.contains("timed out") {
                    received_timeout = true;
                }
            }
        }

        assert!(received_timeout, "Must report timeout failure");
    }
}


