//! Command-line argument parsing and execution modes for the AgentMesh Coordinator.

use std::env;

/// Parsed command-line arguments for the coordinator binary.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CliArgs {
    /// Force non-interactive headless daemon mode (even when stdout is a TTY).
    pub headless: bool,
    /// Force interactive TUI mode (when TTY attached).
    pub tui: bool,
    /// Optional target project UUID or name.
    pub project_id: Option<String>,
    /// Automatically approve proposed tasks without critical overlap warnings during headless runs.
    pub auto_approve: bool,
    /// Print system metrics and fleet summary to stdout, then exit.
    pub status: bool,
    /// Run safe environment diagnostics and exit.
    pub doctor: bool,
    /// Display help information and exit.
    pub show_help: bool,
    /// Display version information and exit.
    pub show_version: bool,
}

impl CliArgs {
    /// Parses arguments from standard environment (`std::env::args()`).
    pub fn parse() -> Result<Self, String> {
        Self::from_args(env::args())
    }

    /// Parses arguments from any iterator of string-like values.
    ///
    /// The first argument is treated as the executable name and skipped.
    pub fn from_args<I, T>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = T>,
        T: Into<String>,
    {
        let raw_args: Vec<String> = args.into_iter().map(Into::into).collect();
        let mut cli = CliArgs::default();

        // Skip binary name if present
        let mut iter = raw_args.into_iter().skip(1).peekable();

        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "-d" | "--headless" => {
                    cli.headless = true;
                }
                "--tui" => {
                    cli.tui = true;
                }
                "-p" | "--project" => {
                    if let Some(next_val) = iter.next() {
                        if next_val.starts_with('-') {
                            return Err(format!(
                                "Option '{arg}' requires a project ID or name, found flag '{next_val}'"
                            ));
                        }
                        cli.project_id = Some(next_val);
                    } else {
                        return Err(format!("Option '{arg}' requires an argument"));
                    }
                }
                arg if arg.starts_with("--project=") => {
                    let val = arg.trim_start_matches("--project=").trim();
                    if val.is_empty() {
                        return Err("Option '--project' requires a non-empty argument".to_string());
                    }
                    cli.project_id = Some(val.to_string());
                }
                arg if arg.starts_with("-p=") => {
                    let val = arg.trim_start_matches("-p=").trim();
                    if val.is_empty() {
                        return Err("Option '-p' requires a non-empty argument".to_string());
                    }
                    cli.project_id = Some(val.to_string());
                }
                "--auto-approve" => {
                    cli.auto_approve = true;
                }
                "-s" | "--status" => {
                    cli.status = true;
                }
                "--doctor" => {
                    cli.doctor = true;
                }
                "-h" | "--help" => {
                    cli.show_help = true;
                }
                "-V" | "-v" | "--version" => {
                    cli.show_version = true;
                }
                unknown if unknown.starts_with('-') => {
                    return Err(format!(
                        "Unrecognized option '{unknown}'. Use --help for usage."
                    ));
                }
                positional => {
                    return Err(format!(
                        "Unexpected positional argument '{positional}'. Use --help for usage."
                    ));
                }
            }
        }

        // Mutual exclusion: cannot specify both --headless and --tui
        if cli.headless && cli.tui {
            return Err("Cannot specify both --headless and --tui simultaneously".to_string());
        }

        Ok(cli)
    }

    /// Determines whether the coordinator should execute in non-interactive headless mode.
    ///
    /// - If `--headless` is passed, returns `true`.
    /// - If `--tui` is passed, returns `false`.
    /// - Otherwise, returns `!stdout_is_tty`.
    pub fn is_headless(&self, stdout_is_tty: bool) -> bool {
        if self.headless {
            true
        } else if self.tui {
            false
        } else {
            !stdout_is_tty
        }
    }

    /// Formatted help text for display when `--help` / `-h` is passed.
    pub fn help_text() -> &'static str {
        concat!(
            "AgentMesh Coordinator v",
            env!("CARGO_PKG_VERSION"),
            "\n",
            "Autonomous multi-agent task coordination platform with interactive TUI and headless daemon.\n\n",
            "USAGE:\n",
            "    coordinator [OPTIONS]\n\n",
            "OPTIONS:\n",
            "    -d, --headless             Force non-interactive daemon mode (no TUI)\n",
            "        --tui                  Force interactive TUI mode (when TTY attached)\n",
            "    -p, --project <ID_OR_NAME> Target specific project by UUID or name\n",
            "        --auto-approve         Automatically approve proposed tasks without critical overlap warnings (headless mode)\n",
            "    -s, --status               Print system metrics and fleet summary, then exit\n",
            "        --doctor               Run safe environment diagnostics, then exit\n",
            "    -h, --help                 Print help information\n",
            "    -V, --version              Print version information\n"
        )
    }

    /// Formatted version string for display when `--version` / `-V` is passed.
    pub fn version_text() -> &'static str {
        concat!("AgentMesh Coordinator v", env!("CARGO_PKG_VERSION"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_empty_args() {
        let args = CliArgs::from_args(vec!["coordinator"]).unwrap();
        assert!(!args.headless);
        assert!(!args.tui);
        assert!(args.project_id.is_none());
        assert!(!args.auto_approve);
        assert!(!args.status);
        assert!(!args.show_help);
        assert!(!args.show_version);
    }

    #[test]
    fn test_headless_flags() {
        let args = CliArgs::from_args(vec!["coordinator", "--headless"]).unwrap();
        assert!(args.headless);
        assert!(!args.tui);

        let args_short = CliArgs::from_args(vec!["coordinator", "-d"]).unwrap();
        assert!(args_short.headless);
    }

    #[test]
    fn test_tui_flag() {
        let args = CliArgs::from_args(vec!["coordinator", "--tui"]).unwrap();
        assert!(args.tui);
        assert!(!args.headless);
    }

    #[test]
    fn test_headless_and_tui_conflict() {
        let res = CliArgs::from_args(vec!["coordinator", "--headless", "--tui"]);
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .contains("Cannot specify both --headless and --tui"));

        let res_short = CliArgs::from_args(vec!["coordinator", "-d", "--tui"]);
        assert!(res_short.is_err());
    }

    #[test]
    fn test_project_flag_space() {
        let args = CliArgs::from_args(vec!["coordinator", "--project", "test-proj-123"]).unwrap();
        assert_eq!(args.project_id.as_deref(), Some("test-proj-123"));

        let args_short = CliArgs::from_args(vec!["coordinator", "-p", "alpha"]).unwrap();
        assert_eq!(args_short.project_id.as_deref(), Some("alpha"));
    }

    #[test]
    fn test_project_flag_equals() {
        let args = CliArgs::from_args(vec!["coordinator", "--project=my-project"]).unwrap();
        assert_eq!(args.project_id.as_deref(), Some("my-project"));

        let args_short = CliArgs::from_args(vec!["coordinator", "-p=beta"]).unwrap();
        assert_eq!(args_short.project_id.as_deref(), Some("beta"));
    }

    #[test]
    fn test_project_flag_missing_arg() {
        let res = CliArgs::from_args(vec!["coordinator", "--project"]);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("requires an argument"));

        let res_flag_next = CliArgs::from_args(vec!["coordinator", "--project", "--tui"]);
        assert!(res_flag_next.is_err());
        assert!(res_flag_next
            .unwrap_err()
            .contains("requires a project ID or name"));
    }

    #[test]
    fn test_auto_approve_flag() {
        let args = CliArgs::from_args(vec!["coordinator", "--auto-approve"]).unwrap();
        assert!(args.auto_approve);
    }

    #[test]
    fn test_status_flag() {
        let args = CliArgs::from_args(vec!["coordinator", "--status"]).unwrap();
        assert!(args.status);

        let args_short = CliArgs::from_args(vec!["coordinator", "-s"]).unwrap();
        assert!(args_short.status);
    }

    #[test]
    fn test_help_and_version_flags() {
        let args_h = CliArgs::from_args(vec!["coordinator", "-h"]).unwrap();
        assert!(args_h.show_help);

        let args_help = CliArgs::from_args(vec!["coordinator", "--help"]).unwrap();
        assert!(args_help.show_help);

        let args_v = CliArgs::from_args(vec!["coordinator", "-V"]).unwrap();
        assert!(args_v.show_version);

        let args_ver = CliArgs::from_args(vec!["coordinator", "--version"]).unwrap();
        assert!(args_ver.show_version);
    }

    #[test]
    fn test_unknown_option() {
        let res = CliArgs::from_args(vec!["coordinator", "--bogus"]);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Unrecognized option '--bogus'"));
    }

    #[test]
    fn test_positional_arg_rejected() {
        let res = CliArgs::from_args(vec!["coordinator", "extra_arg"]);
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .contains("Unexpected positional argument 'extra_arg'"));
    }

    #[test]
    fn test_is_headless_evaluation() {
        let default_args = CliArgs::default();
        // Default on TTY is interactive (not headless)
        assert!(!default_args.is_headless(true));
        // Default on non-TTY is headless
        assert!(default_args.is_headless(false));

        let headless_args = CliArgs {
            headless: true,
            ..Default::default()
        };
        // Explicit headless is always headless
        assert!(headless_args.is_headless(true));
        assert!(headless_args.is_headless(false));

        let tui_args = CliArgs {
            tui: true,
            ..Default::default()
        };
        // Explicit TUI is never headless
        assert!(!tui_args.is_headless(true));
        assert!(!tui_args.is_headless(false));
    }

    #[test]
    fn test_help_and_version_text_contents() {
        let help = CliArgs::help_text();
        assert!(help.contains("--headless"));
        assert!(help.contains("--tui"));
        assert!(help.contains("--project"));
        assert!(help.contains("--auto-approve"));
        assert!(help.contains("--status"));
        assert!(help.contains("--doctor"));

        let version = CliArgs::version_text();
        assert!(version.contains("AgentMesh Coordinator v"));
    }
}
