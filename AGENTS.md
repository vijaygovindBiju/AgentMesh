# AgentMesh — Agent Rules & Guidelines

## 1. Authoritative Task & State Management
- `TODO.md` is the authoritative source of development state.
- Update `TODO.md` before starting, during, and after every task.
- Never mark a task DONE unless its acceptance criteria and tests are fully met.

## 2. Git Commit Convention (Strict Requirement)
Always commit at every meaningful checkpoint — never batch unrelated changes into one commit.

### When to Commit:
- End of every phase (e.g., Phase 0 done, Phase 8 done, etc.)
- After each stable feature or module is complete and compiles cleanly
- After every passing migration or schema change
- After every set of passing tests
- Before and after any large refactor
- Whenever work reaches a state worth preserving independently

### Commit Message Format:
```text
<type>(<scope>): <short summary>

<body — explain what and why, not how. Wrap at 72 chars.>

<footer — breaking changes, issue refs, etc. if applicable>
```

### Commit Types:
- `feat`: New feature or capability
- `fix`: Bug fix
- `refactor`: Code restructure without behaviour change
- `test`: Tests added or updated
- `docs`: Documentation only
- `chore`: Build, config, tooling, CI changes
- `db`: Database schema or migration changes

### Examples:
```text
feat(agent-agy): implement process lifecycle and protocol runner

Implements AgyAgent adapter contract, AgyProcess subprocess management,
NDJSON stream parsing, and JetStream task assignment execution loop.
All unit and integration tests pass.
```
