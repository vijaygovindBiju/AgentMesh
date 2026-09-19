use serde::{Deserialize, Serialize};

/// Detailed task complexity analysis and sizing estimate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComplexityEstimate {
    /// Sizing category: "XS", "S", "M", "L", "XL"
    pub estimated_size: String,
    /// Numerical complexity score (1 to 100)
    pub score: u32,
    /// Detected risk or high-effort factors
    pub risk_factors: Vec<String>,
}

/// Evaluates task scope, affected resources, and technical keywords to estimate complexity.
pub struct ComplexityEstimator;

impl ComplexityEstimator {
    /// Evaluates a task and returns a `ComplexityEstimate`.
    pub fn estimate(
        title: &str,
        description: &str,
        affected_resources: &[String],
        dependency_count: usize,
    ) -> ComplexityEstimate {
        let mut score: u32 = 10;
        let mut risk_factors = Vec::new();

        // 1. Evaluate affected resources footprint
        let resource_count = affected_resources.len();
        if resource_count == 0 {
            score += 5;
        } else if resource_count == 1 {
            score += 10;
        } else if resource_count <= 3 {
            score += 25;
        } else if resource_count <= 6 {
            score += 45;
            risk_factors.push(format!("Modifies {resource_count} separate resource files"));
        } else {
            score += 65;
            risk_factors.push(format!(
                "Broad architectural footprint: {resource_count} resources"
            ));
        }

        // 2. Check for directory-wide impacts
        for r in affected_resources {
            if r.ends_with('/') || r.contains('*') {
                score += 15;
                risk_factors.push(format!("Touches wildcard or directory scope: '{r}'"));
                break;
            }
        }

        // 3. Keyword and domain risk factors
        let full_text = format!("{} {}", title, description).to_lowercase();

        if full_text.contains("migration")
            || full_text.contains("schema")
            || full_text.contains("database")
        {
            score += 20;
            risk_factors.push("Involves database schema or migration alterations".to_string());
        }
        if full_text.contains("refactor")
            || full_text.contains("rewrite")
            || full_text.contains("redesign")
        {
            score += 25;
            risk_factors
                .push("Contains major code refactoring or architectural redesign".to_string());
        }
        if full_text.contains("security")
            || full_text.contains("auth")
            || full_text.contains("crypto")
            || full_text.contains("token")
        {
            score += 15;
            risk_factors
                .push("Security-critical component (authentication / cryptography)".to_string());
        }
        if full_text.contains("protocol")
            || full_text.contains("concurrency")
            || full_text.contains("lock")
            || full_text.contains("async")
        {
            score += 15;
            risk_factors.push("Complex concurrency or messaging protocol interaction".to_string());
        }

        // 4. Dependency graph density
        if dependency_count >= 3 {
            score += 15;
            risk_factors.push(format!(
                "High dependency gating: {dependency_count} prerequisites"
            ));
        }

        // Normalize score between 1 and 100
        let final_score = score.min(100);

        // Map to T-shirt sizing
        let estimated_size = match final_score {
            0..=15 => "XS",
            16..=30 => "S",
            31..=60 => "M",
            61..=80 => "L",
            _ => "XL",
        }
        .to_string();

        ComplexityEstimate {
            estimated_size,
            score: final_score,
            risk_factors,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_complexity_estimation() {
        // Small documentation or typo fix
        let est_small = ComplexityEstimator::estimate(
            "Fix typo in README",
            "Correct misspelled word in docs",
            &["README.md".to_string()],
            0,
        );
        assert!(est_small.estimated_size == "XS" || est_small.estimated_size == "S");
        assert!(est_small.risk_factors.is_empty());

        // Large database refactor
        let est_large = ComplexityEstimator::estimate(
            "Database Schema Migration and Security Refactor",
            "Refactor auth schema and rewrite PostgreSQL migrations across multiple tables",
            &[
                "migrations/003.sql".to_string(),
                "src/auth.rs".to_string(),
                "src/db/users.rs".to_string(),
                "src/db/tokens.rs".to_string(),
            ],
            3,
        );
        assert!(est_large.estimated_size == "L" || est_large.estimated_size == "XL");
        assert!(!est_large.risk_factors.is_empty());
    }
}
