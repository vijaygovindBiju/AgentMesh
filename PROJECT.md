# Project: AgentMesh — Phase 21 Operator TUI Enhancement & Production Console

## Architecture

AgentMesh is an autonomous agent coordination platform built in Rust. It utilizes PostgreSQL 16 as persistent state storage, NATS JetStream for durable message delivery and real-time streaming, and Ratatui + Crossterm for the terminal user interface.

```
                      ┌────────────────────────────────────────┐
                      │        Operator Terminal / CLI         │
                      └──────────────────┬─────────────────────┘
                                         │
                 ┌───────────────────────┴───────────────────────┐
                 │                                               │
                 ▼                                               ▼
     ┌───────────────────────┐                       ┌───────────────────────┐
     │   Interactive TUI     │                       │    Headless Daemon    │
     │  (Ratatui+Crossterm)  │                       │   (--headless / CI)   │
     └───────────┬───────────┘                       └───────────┬───────────┘
                 │ (TuiAction / TuiUpdateEvent)                  │
                 ▼                                               ▼
     ┌───────────────────────────────────────────────────────────────────────┐
     │                     Coordinator Core Runtime                          │
     │  - State Machine (CoordinatorState, transitions)                      │
     │  - Command Handler (Human Approval Gates, Overlap Ack, Retries)       │
     │  - Assignment Service (Atomic FOR UPDATE SKIP LOCKED)                 │
     │  - Background Sweepers (StaleTaskSweeper, Reconciler)                 │
     └───────────────────┬───────────────────────────────┬───────────────────┘
                         │                               │
                         ▼                               ▼
     ┌───────────────────────────────────────┐   ┌───────────────────────────┐
     │              PostgreSQL 16            │   │      NATS JetStream       │
     │  - Tasks, Projects, Proposals         │   │  - TASK_ASSIGNMENTS       │
     │  - TaskApprovals, TaskDeliveries      │   │  - AGENT_EVENTS           │
     │  - Agents, AgentEvents, Overlaps      │   │  - Heartbeat & Discovery  │
     └───────────────────────────────────────┘   └───────────────────────────┘
```

### Key Architectural Boundaries & Principles
1. **Screen Navigation Hierarchy (R1)**: 5 first-class screens:
   - Screen 1: `Dashboard` (System overview, active tasks, overlap alerts, KPI summary)
   - Screen 2: `ProjectManagement` (Project list/loader, project spec & Git metadata, new project definition)
   - Screen 3: `PlanReview` (Interactive visual DAG dependency tree, task cards, human approval gating)
   - Screen 4: `FleetInspector` (Agent fleet monitoring, liveness, heartbeats, runtime profiles, telemetry cards)
   - Screen 5: `Diagnostics` (Live event stream, multi-axis filters, bounded ring buffer, real task timeline drill-down)
   *(With backward-compatible aliases for existing tests: `ProjectInput` -> `ProjectManagement`)*
2. **Global Status Header & Contextual Footer (R1)**:
   - Dynamic 3-column / 2-line adaptive header rendering PostgreSQL connectivity, NATS connectivity, active agent ratio (`active/total`), and pending approval count.
   - Pane-aware contextual shortcut hints with compact terminal truncation resilience.
3. **Command Palette & Safety Confirmation Modals (R1 & R2)**:
   - Global command palette modal (`:`) with real-time text query filtering, categorized execution, and dynamic bounding.
   - Safety confirmation dialog modal for destructive actions (`CancelTask`, `RejectTask`) with keyboard safety (`Enter`/`y` to confirm, `Esc`/`n` to abort).
4. **Project Lifecycle & Intuitive Recursive DAG (R2)**:
   - Full interactive project selection, loading (`TuiAction::LoadProject`), active project spec inspection, and state synchronization in `main.rs`.
   - Recursive DAG tree rendering eliminating the 1-hop traversal bug, grouped by topological execution tiers, with blocker (`[`) and dependent (`]`) navigation jumps.
   - Task retry command handler (`execute_retry_task`) allowing transition from `Cancelled`/`Failed` to `Approved`/`HumanReview`.
5. **Deep Fleet Telemetry & Bounded Event Streaming (R3)**:
   - `AgentInspectorWidget` enriched with liveness timestamps, heartbeat latency, `HealthStatus` badges, consecutive failures, and error banners.
   - Diagnostics live event stream powered by a bounded `VecDeque` ring buffer (limit 300) with $O(1)$ push/pop, eliminating memory leaks, deduplicating SQL poll bursts, clamping scroll indices, and providing severity & entity filters.
   - Elimination of synthetic mock timeline timestamps in `diagnostics.rs` in favor of real `TimelineService::build_task_timeline`.
6. **Resize Resilience & CLI Compatibility (R4)**:
   - Adaptive rendering across 60x18, 70x20, 80x24, 100x30, and 160x50 layouts without panics or visual truncation.
   - Preservation of all existing CLI flags (`--headless`, `--tui`, `--status`, `--project`, `--auto-approve`, `--help`, `--version`).
   - Headless daemon channel drop preventing unbounded memory growth.

---

## Feature Inventory

Every feature and survey finding mapped to an assigned milestone:

| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | Survey & Architecture Inspection | 3 parallel explorers surveying navigation, lifecycle/DAG, fleet/diagnostics, and test suites | M0 | R1-R4, Survey |
| 2 | State Models & Protocol Channel Updates | Update `AppState` (5 screens, connectivity, ring buffer, project list, filter, confirmation modal, clamp helpers) and `TuiAction`/`TuiUpdateEvent` | M1 | R1, R2, R3 |
| 3 | Task Retry State Machine & Command Handler | Allow `(Cancelled, Approved)` in `TaskStatus::can_transition_to` and implement `execute_retry_task` in `commands.rs` | M1 | R2 |
| 4 | Headless Channel Memory Leak Prevention | Drop unpolled `tui_rx` in daemon mode in `main.rs` to prevent memory accumulation | M1 | R3 |
| 5 | Responsive Global Status Header | Adaptive 3-column / 2-line layout displaying PG/NATS connectivity, active agent ratio, and pending approvals | M2 | R1, R4 |
| 6 | Dynamic Contextual Footer Hints | Contextual hotkey hints per active view/pane with compact mode (<85 cols) prioritization | M2 | R1, R4 |
| 7 | Global Command Palette Modal | Modal (`:`) with search filtering, categorized action execution, and dynamic relative bounding | M2 | R1 |
| 8 | Terminal Resize Resilience (60x18 to 160x50) | Responsive layout adaptation, stacked layouts in compact mode, details pane collapse, and extreme size guard | M2 | R4 |
| 9 | Dual-Purpose Project Management Screen | Existing project list/selector, active project spec & Git metadata inspector, and new project input form | M3 | R2 |
| 10 | Recursive DAG Dependency Tree & Navigation | Recursive tree rendering (fixing 1-hop bug), topological execution tiers, blocker/dependent navigation keys (`[`/`]`) | M3 | R2 |
| 11 | Task Approval Gating & Safety Confirmation Dialogs | Gated `y`/`n`/`a`/`e`/`c`/`r` actions and modal confirmation dialog for destructive actions (`c`, `n`) | M3 | R2 |
| 12 | Coordinator Project Loading & Action Handling | Wire `LoadProject`, `RetryTask`, and `SubmitProject` active project state updates in `main.rs` | M3 | R2 |
| 13 | Deep Agent Fleet Telemetry Cards | Enriched `AgentInspectorWidget` with liveness (`last_seen`, latency), health badges, error banner, and task deliveries | M4 | R3 |
| 14 | Bounded Event Ring Buffer & Filtering | `VecDeque` bounded buffer (limit 300), $O(1)$ push/pop, deduplicated SQL polling, index clamping, and multi-axis filters | M4 | R3 |
| 15 | Real Task Timeline Drill-down (No Synthetic Mocks) | Purge fake timestamps in `diagnostics.rs` and wire `TuiAction::InspectTaskTimeline` to `TimelineService` | M4 | R3, R4 |
| 16 | E2E Testing Track: Comprehensive Test Suites | Test infra, Tier 1-4 TestBackend suites (86 existing tests + new modal, action, buffer, resize suites) & `TEST_READY.md` | M_TEST | R4, AC |
| 17 | Final Verification & Adversarial Coverage Hardening | Phase 1 E2E test pass (100%), Phase 2 Tier 5 adversarial hardening, `cargo check --workspace`, clean git commits | M5 | R4, AGENTS.md |

---

## Milestones

| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| M0 | Survey & Architecture Inspection | 3 parallel explorers surveying codebase and formulating technical blueprint | none | DONE |
| M1 | Core State, Channel & State Machine Extensions | `AppState`, `TuiAction`, `TuiUpdateEvent`, `TaskStatus::can_transition_to`, `execute_retry_task`, headless channel drop | M0 | IN_PROGRESS |
| M2 | Responsive Header, Contextual Footer, Command Palette & Resize Resilience | `HeaderWidget`, `FooterWidget`, `CommandPaletteWidget`, `help_modal.rs` bounds, compact layout adaptation | M1 | PLANNED |
| M3 | Project Lifecycle, Recursive DAG & Action Gating | Screen 2 `ProjectManagement`, `dag_view.rs` recursion, confirmation dialog widget, `main.rs` action wiring | M1 | PLANNED |
| M4 | Deep Fleet Telemetry, Bounded Event Stream & Real Timelines | `AgentInspectorWidget`, `FleetInspector` screen, `VecDeque` ring buffer, event filters, real `TimelineService` wiring | M1 | PLANNED |
| M_TEST | E2E Testing Track: Comprehensive TestBackend Suite | Parallel test track: new TestBackend test suites, `TEST_INFRA.md`, publish `TEST_READY.md` | M0 | IN_PROGRESS |
| M5 | Final Milestone: 100% E2E Test Pass & Coverage Hardening | Phase 1 (100% pass Tiers 1-4), Phase 2 (Adversarial coverage hardening Tier 5), `cargo check`, git commits | M1, M2, M3, M4, M_TEST | PLANNED |

---

## Code Layout

- `crates/coordinator/src/tui/state.rs`: App state, screens, focus enums, `TuiAction`, `TuiUpdateEvent`, index clamping
- `crates/coordinator/src/tui/ui.rs`: Root layout orchestration, modal rendering layers, extreme size guard
- `crates/coordinator/src/tui/widgets/header.rs`: Responsive global status header with connectivity pills
- `crates/coordinator/src/tui/widgets/footer.rs`: Dynamic contextual footer with compact prioritization
- `crates/coordinator/src/tui/widgets/command_palette.rs`: Global searchable command palette modal
- `crates/coordinator/src/tui/widgets/confirmation_modal.rs`: Destructive action safety confirmation modal
- `crates/coordinator/src/tui/widgets/help_modal.rs`: Global help cheatsheet with responsive bounds
- `crates/coordinator/src/tui/widgets/dag_view.rs`: Recursive topological execution tree widget
- `crates/coordinator/src/tui/widgets/agent_inspector.rs`: Enriched agent telemetry cards widget
- `crates/coordinator/src/tui/screens/project_input.rs`: Dual-purpose project management & input screen
- `crates/coordinator/src/tui/screens/plan_review.rs`: Interactive DAG review & task approval gating
- `crates/coordinator/src/tui/screens/fleet_inspector.rs`: Dedicated agent fleet management screen
- `crates/coordinator/src/tui/screens/dashboard.rs`: Live dashboard with health badges and active task table
- `crates/coordinator/src/tui/screens/diagnostics.rs`: Live event stream, filters, and real timeline drilldown
- `crates/coordinator/src/domain/task.rs`: Task status transition rules (retry support)
- `crates/coordinator/src/coordinator/commands.rs`: Retry task command handler
- `crates/coordinator/src/main.rs`: Channel wiring, action handling, headless drop
- `crates/coordinator/tests/phase20_tui_verification.rs`: Existing 86 tests (strictly preserved)
- `crates/coordinator/tests/phase21_tui_enhancements.rs`: New TestBackend suites for Phase 21 features
