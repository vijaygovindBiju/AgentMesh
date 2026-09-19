//! Agent Role and Permission Boundary Enforcement.

use uuid::Uuid;

use agent_protocol::security::{AgentRole, PermissionBoundary};
use crate::security::SecurityError;

/// Enforces role permissions and filesystem boundary constraints on agents.
pub struct PermissionEnforcer;

impl PermissionEnforcer {
    /// Validates whether an agent can be assigned a task with the given affected resources.
    pub fn validate_task_assignment(
        agent_id: Uuid,
        role: AgentRole,
        boundary: &PermissionBoundary,
        is_revoked: bool,
        affected_resources: &[String],
    ) -> Result<(), SecurityError> {
        // 1. Revocation check
        if is_revoked {
            return Err(SecurityError::AgentRevoked(agent_id));
        }

        // 2. Role-based capability check
        match role {
            AgentRole::ReadOnly => {
                if !affected_resources.is_empty() {
                    return Err(SecurityError::UnauthorizedAction {
                        agent_id,
                        action: "assign_write_task".to_string(),
                        reason: "ReadOnly agent cannot be assigned tasks with affected resources".to_string(),
                    });
                }
            }
            AgentRole::Reviewer => {
                if !boundary.can_modify_code && !affected_resources.is_empty() {
                    return Err(SecurityError::UnauthorizedAction {
                        agent_id,
                        action: "modify_code".to_string(),
                        reason: "Reviewer agent cannot modify repository code files".to_string(),
                    });
                }
            }
            AgentRole::Worker | AgentRole::Admin => {
                // Workers and Admins may modify code within their boundaries
            }
        }

        // 3. Permission boundary check on affected file paths
        if let Err(denied) = boundary.validate_resources(affected_resources) {
            return Err(SecurityError::PermissionBoundaryViolation {
                agent_id,
                paths: denied.into_iter().cloned().collect(),
            });
        }

        Ok(())
    }

    /// Validates whether the actual resources modified by an agent conform to its permission boundary.
    pub fn validate_actual_modifications(
        agent_id: Uuid,
        boundary: &PermissionBoundary,
        actual_modified_resources: &[String],
    ) -> Result<(), SecurityError> {
        if !boundary.can_modify_code && !actual_modified_resources.is_empty() {
            return Err(SecurityError::UnauthorizedAction {
                agent_id,
                action: "modify_code".to_string(),
                reason: "Agent permission boundary prohibits modifying code".to_string(),
            });
        }

        if let Err(denied) = boundary.validate_resources(actual_modified_resources) {
            return Err(SecurityError::PermissionBoundaryViolation {
                agent_id,
                paths: denied.into_iter().cloned().collect(),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_revoked_agent_is_rejected() {
        let agent_id = Uuid::new_v4();
        let boundary = PermissionBoundary::new();
        let res = PermissionEnforcer::validate_task_assignment(
            agent_id,
            AgentRole::Worker,
            &boundary,
            true, // revoked
            &["src/main.rs".to_string()],
        );

        assert!(matches!(res, Err(SecurityError::AgentRevoked(_))));
    }

    #[test]
    fn test_readonly_agent_cannot_take_write_tasks() {
        let agent_id = Uuid::new_v4();
        let boundary = PermissionBoundary::new();
        let res = PermissionEnforcer::validate_task_assignment(
            agent_id,
            AgentRole::ReadOnly,
            &boundary,
            false,
            &["src/main.rs".to_string()],
        );

        assert!(matches!(res, Err(SecurityError::UnauthorizedAction { .. })));
    }

    #[test]
    fn test_boundary_denied_paths_rejected() {
        let agent_id = Uuid::new_v4();
        let boundary = PermissionBoundary::new()
            .with_denied_path("secure_config/*");

        let res = PermissionEnforcer::validate_task_assignment(
            agent_id,
            AgentRole::Worker,
            &boundary,
            false,
            &["src/lib.rs".to_string(), "secure_config/keys.json".to_string()],
        );

        match res {
            Err(SecurityError::PermissionBoundaryViolation { paths, .. }) => {
                assert_eq!(paths, vec!["secure_config/keys.json"]);
            }
            other => panic!("Expected PermissionBoundaryViolation, got {:?}", other),
        }
    }
}
