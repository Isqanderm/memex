---
name: /arch:subagent-driven-development
description: "Execute plans via delegate_task subagents (2-stage review)."
version: 1.1.1
author: Hermes Agent (adapted from obra/superpowers)
license: MIT
platforms: [linux, macos, windows]
metadata:
  hermes:
    tags: [delegation, subagent, implementation, workflow, parallel]
    related_skills: [/arch:writing-plans, requesting-code-review, test-driven-development]
---

# /arch:subagent-driven-development

## Overview

Execute implementation plans by dispatching fresh subagents per task with systematic two-stage review.

**Core principle:** Fresh subagent per task + two-stage review (spec then quality) = high quality, fast iteration.

## HARD-GATE (Prerequisites)

Before dispatching any subagent, VERIFY that an implementation plan exists:

- [ ] Plan file in `.hermes/plans/` (created by `/arch:writing-plans` skill)

If no plan exists, STOP immediately. Load `/arch:writing-plans` skill first. Do NOT implement without a plan. Do NOT make tasks up as you go.

## When to Use

Use this skill when:
- You have an implementation plan (from /arch:writing-plans skill or user requirements)
- Tasks are mostly independent
- Quality and spec compliance are important
- You want automated review between tasks

**vs. manual execution:**
- Fresh context per task (no confusion from accumulated state)
- Automated review process catches issues early
- Consistent quality checks across all tasks
- Subagents can ask questions before starting work

## The Process

### 1. Read and Parse Plan

Read the plan file. Extract ALL tasks with their full text and context upfront. Create a todo list.

### 2. Per-Task Workflow

For EACH task in the plan:

#### Step 1: Dispatch Implementer Subagent

Use `delegate_task` with complete context: goal, task text, file paths, TDD instructions, project context.

#### Step 2: Dispatch Spec Compliance Reviewer

Verify implementation against original spec. Check all requirements, file paths, function signatures.

**If spec issues found:** Fix gaps, re-review. Continue only when spec-compliant.

#### Step 3: Dispatch Code Quality Reviewer

After spec compliance passes: review code quality, conventions, error handling, test coverage, security.

**If quality issues found:** Fix, re-review. Continue only when approved.

#### Step 4: Mark Complete

### 3. Final Review

After ALL tasks complete, dispatch integration reviewer for consistency.

### 4. Verify and Commit

```bash
pytest tests/ -q
git diff --stat
git add -A && git commit -m "feat: complete [feature name] implementation"
```

## Task Granularity

**Each task = 2-5 minutes of focused work.**

## Red Flags — Never Do These

- Start implementation without a plan
- Skip reviews (spec compliance OR code quality)
- Proceed with unfixed critical/important issues
- Dispatch multiple implementation subagents for tasks that touch the same files
- Make subagent read the plan file (provide full text in context instead)
- Skip scene-setting context (subagent needs to understand where the task fits)
- Ignore subagent questions (answer before letting them proceed)
- Accept "close enough" on spec compliance
- Skip review loops (reviewer found issues → implementer fixes → review again)
- Let implementer self-review replace actual review (both are needed)
- **Start code quality review before spec compliance is PASS** (wrong order)
- Move to next task while either review has open issues

## Handling Issues

### If Subagent Asks Questions

- Answer clearly and completely. Provide additional context if needed.

### If Reviewer Finds Issues

- Implementer subagent (or a new one) fixes them → reviewer reviews again → repeat until approved.

### If Subagent Fails a Task

- Dispatch a new fix subagent with specific instructions about what went wrong.
- Don't try to fix manually in the controller session (context pollution).

## Efficiency Notes

**Why fresh subagent per task:** prevents context pollution, clean focused context.
**Why two-stage review:** spec catches under/over-building early, quality ensures well-built implementation.
**Cost trade-off:** more subagent invocations but catches issues early.

## Terminal State

When ALL tasks are complete, all reviews pass, and the full test suite is green:

The implementation pipeline is complete. The project is ready for the next development cycle — which starts again with `/arch:architecture-documentation`.
