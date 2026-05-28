---
name: /arch:architecture-documentation
description: "ADR, C4, AGENTS.md — architectural artifacts that humans write and LLMs consume. No DSL required."
version: 1.0.1
author: Hermes Agent
license: MIT
platforms: [linux, macos, windows]
metadata:
  hermes:
    tags: [architecture, adr, c4, design, documentation, planning]
    related_skills: [/arch:writing-plans, design-md, excalidraw, golden-query-evaluation, /arch:subagent-driven-development]
---

# /arch:architecture-documentation

Create architectural artifacts before writing code. The key insight: LLMs understand structured natural language — you don't need a DSL. Write for humans first; LLMs will follow.

## When to Use

- Starting a new project (before `/arch:writing-plans`)
- Making a non-obvious architectural decision
- Need to communicate design intent to LLM coding tools (Claude Code, Cursor, Copilot)
- User asks to "think like an architect" or "create architectural artifacts"

## Core Principle: Natural Language Over DSL

LLMs are **intention compilers**, not syntax compilers. They understand structured prose better than formal grammars.

| For compilers | For LLMs |
|---|---|
| `container "API" { technology "FastAPI" }` | "API layer on FastAPI, only routing and DI, business logic in domain" |
| Strict grammar, parse errors break | Conversation, can ask clarifying questions |
| Unnatural for architects | Natural for architects |

**User preference:** DSL is inconvenient for humans. Use Markdown with conventions, not formal languages. The user explicitly rejects DSL-first approaches — always default to structured prose.

## Directory Structure

```
docs/architecture/
├── AGENTS.md              ← LLM constraints and architectural principles
├── adr/
│   ├── README.md          ← ADR process and statuses
│   ├── 0000-template.md   ← blank template
│   └── 0001-*.md          ← actual decisions
└── c4/
    ├── 01-system-context.md  ← C4 Level 1: who and what around the system
    └── 02-containers.md      ← C4 Level 2: services, databases, connections
```

## ADR (Architecture Decision Records)

Each ADR captures **one** architectural decision: context, options considered, choice, rationale, and consequences.

### Template

Use `templates/adr-template.md` (copy to `docs/architecture/adr/0001-short-name.md`).

### Statuses

- `proposed` — under consideration
- `accepted` — adopted
- `superseded` — replaced by a newer ADR (note which one)
- `deprecated` — no longer relevant

### Writing Style

- Write the **context** as a problem statement, not a solution
- List 2-4 **options** with pros, cons, and risks
- State the **decision** in one paragraph with concrete rationale (not "because it's popular")
- Fill in **consequences** after a week of implementation — this is the most valuable section

### ADR Quality Gate

A good ADR should answer: "Why did I choose X over Y given my specific constraints?" If the rationale boils down to "it's the industry standard," the ADR is incomplete.

## C4 Model (Text-Based)

Two levels are sufficient for project start:

### Level 1: System Context
Who (users, external systems) interacts with the system? Describe relationships in prose or ASCII diagram.

### Level 2: Container Diagram
What are the main deployable units (API, database, file storage, background workers, external APIs)? How do they communicate (REST, gRPC, shared DB, message queue)?

No graphical tool required — ASCII boxes and arrows in Markdown are sufficient. The goal is understanding, not aesthetics.

## AGENTS.md

A Markdown file in the project root (or `docs/architecture/`) that LLM coding tools read at session start.

### What to Include

- **Architectural principles** — module boundaries, dependency direction, architectural style
- **Technical constraints** — framework, database, key libraries, async/sync
- **Explicit non-goals** — what NOT to build (prevents LLM over-engineering)
- **ADR references** — pointer to `docs/architecture/adr/` so LLM knows to check decisions

### Why It Works

AGENTS.md is essentially the architectural contract for LLM. When an LLM proposes code, it respects the constraints you've declared. This is the closest thing to "code generation from architectural artifacts" that works today.

## 30-Minute Startup Workflow

1. **C4 Level 1** (10 min) — external users/systems as a list or simple diagram
2. **First ADR** (10 min) — pick the decision you're struggling with most, fill the template
3. **C4 Level 2** (10 min) — containers that follow from your ADR decision

After these three: write AGENTS.md (5 min) and you have a complete architectural baseline.

## Visualization Tools

Two complementary tools for different phases:

| Phase | Tool | Purpose |
|---|---|---|
| **Discuss** (ADR) | Excalidraw (see `excalidraw` skill) | Hand-drawn comparison diagrams — options side-by-side with pros/cons. Informal, invites editing. |
| **Final** (C4) | Mermaid (in markdown) | C4 Level 1-2 diagrams. Structured, version-controlled, LLM-readable. |

**Workflow:** discuss with LLM → sketch in Excalidraw → agree → write ADR.md (markdown, no diagrams needed inside ADR) + Mermaid C4 diagram (separate file).

Load `excalidraw` skill for hand-drawn ADR discussion diagrams. Use Mermaid code blocks in markdown for C4.

## Full Pipeline (Superpowers Sequence)

```
Discuss → Excalidraw → ADR.md → Mermaid C4 → AGENTS.md → Golden Queries → Implementation Plan → Subagent-Driven Execution
```

1. **Discuss** — LLM presents options with pros/cons given user's constraints
2. **Excalidraw** — visual decision matrix (informal, hand-drawn)
3. **ADR.md** — formal decision record (context, options, decision, consequences)
4. **Mermaid C4** — system context + container diagram in markdown
5. **AGENTS.md** — architectural contract for LLM coding tools
6. **Golden Queries** — acceptance criteria before any code
7. **Implementation Plan** — task-by-task via `/arch:writing-plans`
8. **Subagent-Driven** — execute with two-stage review per task

## Pitfalls

- **Don't start with code.** If architectural decisions aren't documented, the code drifts from intent.
- **Don't jump straight to ADR markdown without discussion.** Use Excalidraw for the conversation phase — it's faster to iterate visually, then crystallise into ADR.md.
- **Don't confuse discussion artifacts with final artifacts.** Excalidraw = temporary, for the conversation. ADR.md + Mermaid C4 = permanent, in the repo.
- **Don't write all ADRs upfront.** One ADR per non-obvious decision. If it's obvious, it's not an ADR.
- **Don't use DSL (Structurizr, HCL, etc.) unless the user explicitly prefers it.** The user finds DSL inconvenient. Natural language with Markdown conventions is the default.
- **Don't write the plan for the user unless asked.** Some users want the toolkit/templates to do architectural thinking themselves. Offer the choice: "Want me to write the plan, or want templates so you can think through it yourself?"
- **C4 and ADR answer different questions.** C4 = what and where. ADR = why. Don't confuse them.

## HARD-GATE (Start Here)

This is a **terminal state** for the architecture phase. When ALL architectural artifacts are complete:

- [ ] ADR decisions recorded in `docs/architecture/adr/`
- [ ] C4 Level 1-2 diagrams in `docs/architecture/c4/`
- [ ] AGENTS.md in project root or `docs/architecture/`

**The ONLY skill you invoke after /arch:architecture-documentation is `/arch:writing-plans`.**

Do NOT load `/arch:subagent-driven-development`, `golden-query-evaluation`, or any implementation skill directly. Architecture → Plan → Execute. In that order. No shortcuts.

These artifact paths are the CONTRACT between architecture and planning:
- `docs/architecture/adr/` — ADR markdown files (consumed by `/arch:writing-plans`)
- `docs/architecture/c4/` — C4 diagrams in Mermaid (consumed by `/arch:writing-plans`)
- `AGENTS.md` or `docs/architecture/AGENTS.md` — architectural contract (consumed by `/arch:writing-plans` and `/arch:subagent-driven-development`)
