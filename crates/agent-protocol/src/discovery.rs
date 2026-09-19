use std::process::Command;

use crate::capabilities::{
    AgentCapabilities, LanguageCapability, RuntimeCapability, ToolCapability,
};

/// Automated host environment capability detection.
///
/// Probes system PATH, compilers, and developer tools using standard library calls.
pub struct CapabilityDetector;

impl CapabilityDetector {
    /// Detects host runtime environment details (OS, CPU arch, CPU cores, memory, agy CLI).
    pub fn detect_runtime(adapter_type: &str) -> RuntimeCapability {
        let os = std::env::consts::OS.to_string();
        let arch = std::env::consts::ARCH.to_string();
        let cpu_count = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);

        let agy_version = Self::probe_command_version("agy", &["--version"]);
        let memory_mb = Self::probe_memory_mb();

        RuntimeCapability {
            os,
            arch,
            adapter_type: adapter_type.to_string(),
            agy_version,
            cpu_count,
            memory_mb,
        }
    }

    /// Detects installed programming languages, compilers, and framework toolchains.
    pub fn detect_languages() -> Vec<LanguageCapability> {
        let mut languages = Vec::new();

        // 1. Rust
        if let Some(ver) = Self::probe_command_version("rustc", &["--version"])
            .or_else(|| Self::probe_command_version("cargo", &["--version"]))
        {
            let mut frameworks = Vec::new();
            if Self::has_command("cargo-nextest") {
                frameworks.push("nextest".to_string());
            }
            if Self::has_command("sqlx") {
                frameworks.push("sqlx".to_string());
            }
            languages.push(LanguageCapability::new("rust", Some(ver), frameworks));
        }

        // 2. Python
        if let Some(ver) = Self::probe_command_version("python3", &["--version"])
            .or_else(|| Self::probe_command_version("python", &["--version"]))
        {
            languages.push(LanguageCapability::new("python", Some(ver), vec![]));
        }

        // 3. Node / TypeScript / JavaScript
        if let Some(node_ver) = Self::probe_command_version("node", &["--version"]) {
            let mut frameworks = Vec::new();
            if Self::has_command("tsc") {
                frameworks.push("typescript".to_string());
            }
            if Self::has_command("npm") {
                frameworks.push("npm".to_string());
            }
            if Self::has_command("pnpm") {
                frameworks.push("pnpm".to_string());
            }
            if Self::has_command("yarn") {
                frameworks.push("yarn".to_string());
            }

            languages.push(LanguageCapability::new("javascript", Some(node_ver), frameworks.clone()));
            if Self::has_command("tsc") {
                let tsc_ver = Self::probe_command_version("tsc", &["--version"]);
                languages.push(LanguageCapability::new("typescript", tsc_ver, frameworks));
            }
        }

        // 4. Go
        if let Some(ver) = Self::probe_command_version("go", &["version"]) {
            languages.push(LanguageCapability::new("go", Some(ver), vec![]));
        }

        // 5. Dart / Flutter
        if let Some(ver) = Self::probe_command_version("flutter", &["--version"]) {
            languages.push(LanguageCapability::new("flutter", Some(ver), vec!["dart".to_string()]));
        } else if let Some(ver) = Self::probe_command_version("dart", &["--version"]) {
            languages.push(LanguageCapability::new("dart", Some(ver), vec![]));
        }

        // 6. C / C++
        if let Some(ver) = Self::probe_command_version("clang", &["--version"])
            .or_else(|| Self::probe_command_version("gcc", &["--version"]))
        {
            languages.push(LanguageCapability::new("c", Some(ver.clone()), vec![]));
            languages.push(LanguageCapability::new("cpp", Some(ver), vec![]));
        }

        languages
    }

    /// Detects common developer utilities and CLI tools.
    pub fn detect_tools() -> Vec<ToolCapability> {
        let tools_to_check = [
            "git",
            "docker",
            "docker-compose",
            "cargo",
            "npm",
            "python3",
            "sqlx",
            "agy",
            "make",
            "curl",
            "tar",
        ];

        let mut detected = Vec::new();

        for tool in tools_to_check {
            if let Some(ver) = Self::probe_command_version(tool, &["--version"])
                .or_else(|| Self::probe_command_version(tool, &["-v"]))
                .or_else(|| Self::probe_command_version(tool, &["version"]))
            {
                let path = Self::probe_command_path(tool);
                detected.push(ToolCapability::new(tool, Some(ver), path));
            } else if Self::has_command(tool) {
                let path = Self::probe_command_path(tool);
                detected.push(ToolCapability::new(tool, None, path));
            }
        }

        detected
    }

    /// Synthesizes complete capability profile combining runtime, languages, tools, and extra tags.
    pub fn detect_all(adapter_type: &str, extra_tags: &[String]) -> AgentCapabilities {
        let runtime = Self::detect_runtime(adapter_type);
        let languages = Self::detect_languages();
        let tools = Self::detect_tools();

        let mut tags = Vec::new();
        // Automatically infer tags from detected languages & tools
        for lang in &languages {
            tags.push(lang.name.clone());
            match lang.name.as_str() {
                "rust" => {
                    tags.push("backend".to_string());
                    tags.push("systems".to_string());
                }
                "python" => {
                    tags.push("scripting".to_string());
                    tags.push("backend".to_string());
                }
                "javascript" | "typescript" => {
                    tags.push("frontend".to_string());
                    tags.push("web".to_string());
                }
                "go" => {
                    tags.push("backend".to_string());
                    tags.push("cloud".to_string());
                }
                "flutter" | "dart" => {
                    tags.push("frontend".to_string());
                    tags.push("mobile".to_string());
                }
                _ => {}
            }
        }

        for tool in &tools {
            if tool.name == "docker" || tool.name == "docker-compose" {
                tags.push("devops".to_string());
            }
            if tool.name == "git" {
                tags.push("vcs".to_string());
            }
        }

        for extra in extra_tags {
            tags.push(extra.to_lowercase());
        }

        tags.sort();
        tags.dedup();

        AgentCapabilities {
            runtime,
            languages,
            tools,
            tags,
        }
    }

    // ─── Internal Probing Helpers ─────────────────────────────────────────────

    fn has_command(cmd: &str) -> bool {
        Command::new(cmd)
            .arg("--help")
            .output()
            .map(|out| out.status.success() || !out.stdout.is_empty() || !out.stderr.is_empty())
            .unwrap_or(false)
    }

    fn probe_command_version(cmd: &str, args: &[&str]) -> Option<String> {
        let out = Command::new(cmd).args(args).output().ok()?;
        if out.status.success() || !out.stdout.is_empty() {
            let s = String::from_utf8_lossy(&out.stdout);
            let first_line = s.lines().next()?.trim();
            if !first_line.is_empty() {
                return Some(first_line.to_string());
            }
        }
        let err = String::from_utf8_lossy(&out.stderr);
        let first_line = err.lines().next()?.trim();
        if !first_line.is_empty() {
            return Some(first_line.to_string());
        }
        None
    }

    fn probe_command_path(cmd: &str) -> Option<String> {
        let out = Command::new("which").arg(cmd).output().ok()?;
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
        None
    }

    fn probe_memory_mb() -> Option<u64> {
        #[cfg(target_os = "linux")]
        {
            if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
                for line in content.lines() {
                    if line.starts_with("MemTotal:") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            if let Ok(kb) = parts[1].parse::<u64>() {
                                return Some(kb / 1024);
                            }
                        }
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_runtime() {
        let runtime = CapabilityDetector::detect_runtime("Mock");
        assert!(!runtime.os.is_empty());
        assert!(!runtime.arch.is_empty());
        assert_eq!(runtime.adapter_type, "Mock");
        assert!(runtime.cpu_count >= 1);
    }

    #[test]
    fn test_detect_all() {
        let caps = CapabilityDetector::detect_all("Mock", &["test_tag".to_string()]);
        assert!(caps.has_tag("test_tag"));
        // cargo / rust should be detected on this host
        assert!(caps.has_language("rust") || caps.has_tool("cargo") || caps.has_tool("git"));
    }
}
