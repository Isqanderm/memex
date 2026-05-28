---
name: superpowers-pipeline
description: "Entry point for the design-to-implementation pipeline: architecture → plan → subagent execution. Load this one skill to start the full flow."
version: 1.0.0
author: Hermes Agent
license: MIT
platforms: [linux, macos, windows]
metadata:
  hermes:
    tags: [pipeline, architecture, planning, implementation, workflow, orchestration]
    related_skills: [architecture-documentation, writing-plans, subagent-driven-development]
---

# Superpowers Pipeline

## Overview

A three-phase design-to-implementation pipeline modelled after [obra/superpowers](https://github.com/obra/superpowers). Each phase has a strict terminal state that points to the next — no central orchestrator, just explicit handoffs.

```
Phase 1                    Phase 2                    Phase 3
architecture-              writing-                   subagent-driven-
documentation         →    plans                 →    development
─────────────────         ─────────                  ──────────────────
ADR + C4 + AGENTS.md      Task-by-task plan           delegate_task × N
                                                      + 2-stage review
```

**You are at Phase 1. Load `architecture-documentation` now.**

## Core Principle

Each skill explicitly declares:
1. **Prerequisite gate** — what artifacts must exist before starting
2. **Terminal state** — what artifacts it produces and where
3. **Next skill** — the ONLY skill to load after completion

No guessing. No hoping the agent figures it out. The contract is in the text.

## Artifact Convention (The Contract)

| Phase | Produces | Path | Consumed By |
|---|---|---|---|
| Architecture | ADR decisions | `docs/architecture/adr/` | writing-plans |
| Architecture | C4 diagrams | `docs/architecture/c4/` | writing-plans |
| Architecture | AGENTS.md | Project root or `docs/architecture/` | writing-plans, subagent-driven-development |
| Planning | Implementation plan | `.hermes/plans/<name>.md` | subagent-driven-development |
| Execution | Working code | Project source | (terminal — pipeline ends here) |

## Phase 1: Architecture Documentation

**Skill to load: `architecture-documentation`**

Produces:
- **ADR** — one per non-obvious architectural decision (context, options, decision, consequences)
- **C4 Level 1-2** — system context + container diagrams (Mermaid in markdown)
- **AGENTS.md** — architectural contract for LLM coding tools

Visual workflow for ADR discussion:
1. LLM presents options with pros/cons
2. Excalidraw sketch (hand-drawn, temporary)
3. Formalise into ADR.md (permanent, in repo)

**Gate to proceed:** All three artifacts exist and user has approved the architectural direction.

**Terminal state:** Load `writing-plans`. Do NOT load `subagent-driven-development` directly.

## Phase 2: Implementation Planning

**Skill to load: `writing-plans`**

**Prerequisite:** ADR + C4 + AGENTS.md exist. If missing, STOP and go back to Phase 1.

Produces:
- Implementation plan in `.hermes/plans/` — bite-sized tasks (2-5 min each), exact file paths, complete code, verification steps

**Gate to proceed:** Plan saved and user has approved the scope.

**Terminal state:** Load `subagent-driven-development`. Provide the plan file path.

## Phase 3: Subagent-Driven Execution

**Skill to load: `subagent-driven-development`**

**Prerequisite:** Implementation plan exists in `.hermes/plans/`. If missing, STOP and go back to Phase 2.

Produces:
- Working code, committed task-by-task
- Two-stage review per task (spec compliance → code quality)
- Final integration review

**Terminal state:** Pipeline complete. No further skills loaded automatically.

## Full Flow (Checklist)

- [ ] Phase 1: `architecture-documentation` loaded
- [ ] ADR-001+ written, approved
- [ ] C4 Level 1-2 diagrams written
- [ ] AGENTS.md written
- [ ] → Load `writing-plans`
- [ ] Phase 2: Plan written, tasks are bite-sized
- [ ] Plan saved to `.hermes/plans/`
- [ ] → Load `subagent-driven-development`
- [ ] Phase 3: Tasks dispatched, reviews passed
- [ ] Full test suite green
- [ ] Pipeline complete

## Why This Works (Superpowers Mechanism)

1. **Explicit directives beat metadata.** `related_skills` in frontmatter is a hint. "The ONLY skill you invoke after me is X" in the body is a directive. Agents follow directives.
2. **Known paths beat discovery.** Each skill knows exactly where to read artifacts. No file system scanning needed.
3. **Gates prevent skipping.** Each skill refuses to run without prerequisite artifacts.
4. **No central state.** Each skill is self-contained. The pipeline emerges from the handoffs, not from an orchestrator.

## Pitfalls

- **Don't skip Phase 1.** Architecture without a plan is directionless. A plan without architecture is guesswork.
- **Don't load multiple phases at once.** One skill at a time, in order.
- **Don't implement without a plan.** Phase 3 refuses to run without Phase 2 artifacts.
- **Don't blend phases.** Architecture decisions are NOT made during planning. Planning is NOT done during implementation.
