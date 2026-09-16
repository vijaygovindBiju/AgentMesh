# ADR 004 — TUI: ratatui

**Date:** 2026-09-16  
**Status:** Accepted

---

## Context

The coordinator needs a terminal UI for:
1. **Project input screen** — human provides a project description
2. **Plan review screen** — per-task approval with Y/N/E/↑↓/Enter
3. **Live dashboard** — real-time view of agent status and task lifecycle

The UI must update in real time as agent events arrive via NATS, without blocking the event processing loop.

---

## Options

| Technology | Pros | Cons |
|---|---|---|
| tui-rs | Original Rust TUI library | Archived/deprecated; replaced by ratatui |
| ratatui | Active fork of tui-rs; composable widgets; good docs | Complex layout code |
| cursive | Higher-level abstractions | Less active; fewer widgets |
| egui (terminal) | Immediate mode | Not suited for terminal rendering |
| Web UI | Rich UI possible | Requires browser; separate process; more complex setup |

---

## Decision

**ratatui** with the `crossterm` backend.

---

## Reasons

1. **Active, maintained fork.** ratatui is the direct successor to tui-rs with active releases and a growing contributor base.

2. **Rust-native.** No FFI, no JavaScript, no browser. Runs in the same process as the coordinator.

3. **Composable widget model.** Custom widgets (task card, overlap warning banner, agent status row) can be composed from primitive ratatui blocks and text.

4. **crossterm backend.** Works on Linux, macOS, and Windows without external dependencies. The coordinator targets Linux for deployment but developers may use macOS.

5. **Keyboard-only interaction.** The approval flow (Y/N/E/↑↓/Enter) is naturally keyboard-driven, which matches ratatui's model.

6. **Async-friendly.** ratatui does not own the event loop; the coordinator's tokio runtime drives it. Real-time NATS event updates can trigger TUI re-renders via `mpsc` channels.

---

## Approval UX Design

```
┌──────────────────────────────────────────────────────────────────┐
│ AgentMesh  ●  PLAN REVIEW  ●  Task 3 of 8            [ESC: abort]│
├──────────────────────────────────────────────────────────────────┤
│                                                                  │
│  TASK-003  Set up authentication middleware            [PENDING] │
│  ─────────────────────────────────────────────────────────────── │
│  Assigned to:  Agent A  (owner: Alice)                           │
│  Depends on:   TASK-001 (DB schema) ✓ approved                   │
│  Resources:    src/auth/, src/middleware/auth.rs                  │
│                                                                  │
│  Description:                                                    │
│  Implement JWT validation middleware. Register routes for        │
│  /auth/login and /auth/refresh. Connect to user table from       │
│  TASK-001.                                                       │
│                                                                  │
│  ⚠ OVERLAP: src/middleware/ also touched by TASK-007             │
│                                                                  │
│  [Enter] shows full dependency list + all overlap warnings       │
│  ─────────────────────────────────────────────────────────────── │
│  [Y] Approve  [N] Reject  [E] Edit  [↑/↓] Navigate              │
└──────────────────────────────────────────────────────────────────┘
```

**Keybindings:**
- `Y` — Approve current task
- `N` — Reject current task
- `E` — Enter inline edit mode for description; `Enter` to confirm + approve
- `↑/↓` — Navigate between tasks (does not approve/reject)
- `Enter` — Open details pane: full dependencies + all overlap warnings
- `ESC` — Abort plan review (requires confirmation; no changes applied)

---

## Consequences

- Complex layout management in Rust (explicit constraint-based sizing).
- Inline text editing in ratatui requires careful cursor and input handling.
- Mouse input not supported in v0.1 (keyboard only).
- TUI rendering must be driven from the main thread; NATS events update shared state via `Arc<Mutex<AppState>>` or `tokio::sync::mpsc` channels.
