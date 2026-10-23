# Project: AgentMesh — Phase 22 Terminal UI Layout, Rendering & Usability Fixes

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
     │  - AppState & Screen Models (6 screens: Dash, Proj, Plan, Fleet,      │
     │                              Diag, Settings)                          │
     │  - PlanReviewFocus (TaskList vs TaskInspector)                        │
     │  - Dual-Column Responsive Layout (Compact, Standard, Wide)            │
     │  - Content-Adaptive Inspector & Smooth Sliding-Window Task Cards      │
     │  - Synchronized Contextual Footers & Adaptive Headers                 │
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
1. **Responsive Two-Column Plan Screen (R1)**:
   - Dynamic width-aware layout:
     - Compact (<90 cols): `Constraint::Percentage(46)` / `Constraint::Percentage(54)`
     - Standard (90..139 cols): `Constraint::Percentage(44)` / `Constraint::Percentage(56)`
     - Wide (>=140 cols): `Constraint::Length(64)` / `Constraint::Min(76)` to eliminate wasted whitespace.
   - Adaptive block titles based on pane width preventing border clipping.
   - Harmonized 90-column breakpoint across `ui.rs`, `header.rs`, and `footer.rs`.
2. **Content-Adaptive Task Inspector (R2)**:
   - Dynamic section height calculations (metadata, description, dependencies, resources, overlaps) based on actual item counts. Empty sections collapse from 5/6 to 3 lines (`Length(3)`).
   - Paragraph word wrapping with `Wrap { trim: true }` across description, metadata, dependencies, resources, and warnings.
   - Dedicated `AppState::inspector_scroll_offset` enabling vertical scrolling via mouse wheel and keyboard (`Shift+Up/Down`, `Shift+J/K`, or active inspector pane focus).
   - Auto-reset `inspector_scroll_offset = 0` on task selection change.
3. **High-Contrast Task List & Card Presentation (R3)**:
   - Distinct combined visual indicator for focused + multi-selected cards (`▶ [✓] ` prefix with dual-highlight border and badge).
   - Metadata layout formatted into a 2-line structure or adaptive wrapping to ensure status, agent, and progress bar are never clipped.
   - Graceful title truncation with ellipsis `...` when title exceeds card width.
   - Dynamic card height allocation when overlap warnings exist so dependencies preview remains visible.
   - Sliding-window viewport offset calculation preserving smooth upward scrolling without pinning the active item to the bottom row.
4. **Synchronized Keyboard Navigation & Footer Hints (R4)**:
   - `PlanReviewFocus { TaskList, TaskInspector }` with `Tab` / `Left`/`Right` pane switching.
   - Selective bulk approval: `Shift+A` approves `selected_task_indices` when multi-selection is active; otherwise prompts for all pending tasks.
   - Adaptive footer hints: concise format for width < 150 cols, expanded format for >= 150 cols. Primary actions (`[Y]`, `[N]`, `[E]`, `[C]`, `[Shift+A]`) preserved in compact mode (<90 cols).
   - Dynamic status chunk allocation (`Length(34.max(width/4))`) preventing 120-col status toast truncation.
5. **Visual Consistency & State Machine Integrity (R5)**:
   - Cyan border aesthetic, `[▶ FOCUSED]` active pane indicator, and status color codes (green success, yellow pending/warning, red error/rejection).
   - Strict compile-time and runtime validation of coordinator domain task transitions.

---

## Feature Inventory

Every requirement and survey finding mapped to an assigned milestone:

| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | Survey & Technical Mapping | 3 parallel explorers mapped R1, R2, R3, R4, R5, and test gaps | M0 | Survey |
| 2 | Core State & Navigation Extensions | `PlanReviewFocus`, `inspector_scroll_offset`, mouse column routing, selective `Shift+A`, 6-screen tab cycle fix | M1 | R4, R5 |
| 3 | Responsive Plan Layout & Header/Footer Harmonization | Adaptive columns (compact/standard/wide), unified 90-col breakpoint, compact footer hints, dynamic status chunk | M2 | R1, R4 |
| 4 | Content-Adaptive Task Inspector | Dynamic height allocation, `Wrap { trim: true }`, vertical scrolling, scroll reset on task change | M3 | R2 |
| 5 | Task List Rendering & Card Ergonomics | Combined focus+selection badge, 2-line metadata wrapping, title ellipsis, dynamic card height, sliding-window scroll | M4 | R3, R5 |
| 6 | E2E Usability & Ergonomics Test Suite | Comprehensive TestBackend test suite across all 4 tiers verifying R1-R5 deterministically, publish `TEST_READY.md` | M_TEST | R1-R5 |
| 7 | Final Verification & Coverage Hardening | 100% test pass, clippy, gate verification (Reviewers x2, Challengers x2, Forensic Auditor), git commits | M5 | R5, AGENTS.md |

---

## Milestones

| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| M0 | Survey & Technical Mapping | Complete technical survey across all 5 requirements and test suites | none | DONE |
| M1 | Core State & Navigation Extensions | `state.rs`: `PlanReviewFocus`, `inspector_scroll_offset`, selective `Shift+A`, mouse column routing, 6-screen test fix | M0 | DONE |
| M2 | Responsive Plan Layout & Header/Footer Harmonization | `plan_review.rs`, `header.rs`, `footer.rs`: column constraints, 90-col breakpoint, compact hints, toast width | M1 | DONE |
| M3 | Content-Adaptive Task Inspector | `details_pane.rs`: dynamic section heights, `Wrap { trim: true }`, vertical scrolling with offset | M1 | DONE |
| M4 | Task List Rendering & Card Ergonomics | `task_card.rs`, `plan_review.rs`: combined focus+select indicator, 2-line metadata, title ellipsis, sliding-window scroll | M1 | DONE |
| M_TEST | E2E Testing Track: Usability & Ergonomics Suite | `phase22_plan_review_ergonomics.rs`: Tiers 1-4 TestBackend verification, `TEST_READY.md` (25/25 passed) | M0 | DONE |
| M5 | Final Milestone: 100% Pass & Coverage Hardening | All tests pass, clippy clean, Reviewers, Challengers, Forensic Auditor, git commits | M1, M2, M3, M4, M_TEST | DONE |

---

## Interface Contracts

### `AppState` ↔ `PlanReviewScreen` & `DetailsPaneWidget`
- `state.plan_review_focus: PlanReviewFocus`:
  - Enum variants: `TaskList`, `TaskInspector`.
  - Determines which pane receives active `[▶ FOCUSED]` border styling and keyboard scroll events.
- `state.inspector_scroll_offset: usize`:
  - Vertical line scroll offset applied to inspector contents.
  - Reset to 0 whenever `selected_task_index` changes.
- `state.inspector_scroll_down()`, `state.inspector_scroll_up()`:
  - Increment/decrement helpers clamping offset to maximum content lines.
- `state.selected_task_indices: HashSet<usize>`:
  - If non-empty, `Shift+A` targets only `selected_task_indices`.
  - If empty, `Shift+A` targets all pending tasks (`HumanReview` or `Proposed`).

---

## Code Layout

- `crates/coordinator/src/tui/state.rs`: `AppState`, `PlanReviewFocus`, mouse handling, keyboard actions, selective bulk approval
- `crates/coordinator/src/tui/screens/plan_review.rs`: Responsive two-column layout, adaptive block titles, sliding-window card scrolling
- `crates/coordinator/src/tui/widgets/task_card.rs`: Combined focus+selection badge, 2-line metadata formatting, title ellipsis, dynamic height
- `crates/coordinator/src/tui/widgets/details_pane.rs`: Content-adaptive height allocation, `Wrap { trim: true }`, vertical scrolling
- `crates/coordinator/src/tui/widgets/header.rs`: Unified 90-col breakpoint, right-sized tabs chunk (`Length(48)`), compact pill formatting
- `crates/coordinator/src/tui/widgets/footer.rs`: Unified 90-col breakpoint, concise/expanded hints, compact action hints, dynamic status chunk
- `crates/coordinator/src/tui/ui.rs`: Root layout and extreme size guard
- `crates/coordinator/tests/phase22_tui_usability.rs`: Usability and ergonomics TestBackend test suite
- `crates/coordinator/tests/phase20_tui_verification.rs`: Backward-compatible TUI verification suite
- `crates/coordinator/tests/phase21_tui_enhancements.rs`: Backward-compatible TUI enhancements suite
