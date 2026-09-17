use serde::{Deserialize, Serialize};

/// High-level Coordinator operational state machine.
///
/// Tracks the lifecycle of the coordinator coordinating a project:
/// Idle -> ProjectInput -> Planning -> HumanReview -> Assigning -> Executing -> Done
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinatorState {
    /// Idle, waiting for project input or new instructions.
    Idle,
    /// Receiving or accepting project input.
    ProjectInput,
    /// AI planning service is decomposing project requirements into proposed tasks.
    Planning,
    /// Tasks are queued for human review and approval in the TUI review interface.
    HumanReview,
    /// Matching and assigning approved, unblocked tasks to available agents.
    Assigning,
    /// Agents are actively executing tasks.
    Executing,
    /// All tasks for the project have successfully completed.
    Done,
}

impl CoordinatorState {
    /// Validates if transitioning from `self` to `next` is allowed.
    pub fn can_transition_to(&self, next: &CoordinatorState) -> bool {
        use CoordinatorState::*;
        matches!(
            (self, next),
            (Idle, ProjectInput)
                | (ProjectInput, Planning)
                | (Planning, HumanReview)
                | (HumanReview, Assigning)
                | (Assigning, Executing)
                | (Executing, Assigning) // More tasks ready for assignment while executing
                | (Executing, Done)
                | (Done, Idle)
                // Recovery / reset / cancellation paths
                | (Planning, Idle)
                | (HumanReview, Idle)
                | (Assigning, Idle)
                | (Executing, Idle)
        )
    }

    /// Returns `true` if the coordinator is actively orchestrating tasks (Assigning or Executing).
    pub fn is_active(&self) -> bool {
        matches!(self, CoordinatorState::Assigning | CoordinatorState::Executing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinator_state_transitions() {
        assert!(CoordinatorState::Idle.can_transition_to(&CoordinatorState::ProjectInput));
        assert!(CoordinatorState::ProjectInput.can_transition_to(&CoordinatorState::Planning));
        assert!(CoordinatorState::Planning.can_transition_to(&CoordinatorState::HumanReview));
        assert!(CoordinatorState::HumanReview.can_transition_to(&CoordinatorState::Assigning));
        assert!(CoordinatorState::Assigning.can_transition_to(&CoordinatorState::Executing));
        assert!(CoordinatorState::Executing.can_transition_to(&CoordinatorState::Assigning));
        assert!(CoordinatorState::Executing.can_transition_to(&CoordinatorState::Done));
        assert!(CoordinatorState::Done.can_transition_to(&CoordinatorState::Idle));

        // Invalid transitions
        assert!(!CoordinatorState::Idle.can_transition_to(&CoordinatorState::Executing));
        assert!(!CoordinatorState::Planning.can_transition_to(&CoordinatorState::Assigning));
        assert!(!CoordinatorState::ProjectInput.can_transition_to(&CoordinatorState::Done));
    }

    #[test]
    fn test_coordinator_state_serde() {
        for state in [
            CoordinatorState::Idle,
            CoordinatorState::ProjectInput,
            CoordinatorState::Planning,
            CoordinatorState::HumanReview,
            CoordinatorState::Assigning,
            CoordinatorState::Executing,
            CoordinatorState::Done,
        ] {
            let json = serde_json::to_string(&state).unwrap();
            let recovered: CoordinatorState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, recovered);
        }
    }
}
