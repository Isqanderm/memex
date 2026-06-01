# Memory Categories & Context Design

**Date:** 2026-06-02  
**Status:** Approved  
**Branch:** feat/better-context-prompt

---

## Problem

User's personal notes, research findings, and reminders are stored as undifferentiated memory facts. Two concrete pain points:

1. **All in one pile** — research notes, reminders, personal thoughts, decisions, preferences are mixed together with no way to distinguish or filter them
2. **No context in retrieval** — search results show bare facts (`User lives in Moscow`) with no indication of when the fact was stored or what project it belongs to

---

## Solution

Three changes that work together:

### 1. Add `category` and `project` fields to Memory

New fields on the `Memory` model:

| Field | Type | Values |
|---|---|---|
| `category` | enum (nullable) | `research` / `reminder` / `thought` / `decision` / `preference` |
| `project` | string (nullable) | free-form project tag, e.g. "Memex", "work", "personal" |

Both are extracted automatically by LLM during `remember()` — user never sets them manually.

**Category definitions:**
- `research` — notes from investigation, reading, experiments ("Found that e5-small has 384 dims")
- `reminder` — tasks, appointments, things to do/check ("Need to review PR by Friday")
- `thought` — personal ideas, reflections, observations ("I think we should switch to async queue")
- `decision` — concluded choices ("Decided to use PostgreSQL over MongoDB")
- `preference` — stable user settings/preferences ("Prefers dark mode", "Uses Python for backend")

### 2. Show category + date in retrieval context

Current format: `[memory] User lives in Moscow`

New format: `[memory | personal | 2026-01-15] User lives in Moscow`

Full format when project is set: `[memory | research | Memex | 2026-05-20] multilingual-e5-small has 384 dimensions`

This gives the LLM temporal and categorical context to reason about relevance, recency, and scope.

### 3. Filtering in `recall()` and `/api/memory/*`

New optional parameter:
- `recall(query, category="research")` — MCP tool
- `GET /api/memory/list?category=reminder` — REST API
- `POST /api/query` body: `{"query": "...", "memory_category": "research"}` — filters memory search

---

## Data Model Changes

### Migration 0006: add category and project columns

```sql
ALTER TABLE memories ADD COLUMN category VARCHAR(20);
ALTER TABLE memories ADD COLUMN project VARCHAR(100);
```

No default — NULL means "not categorized" (legacy memories). No NOT NULL constraint — category detection can fail silently.

### Updated Memory model

```python
category: Mapped[str | None] = mapped_column(String(20), nullable=True)
project: Mapped[str | None] = mapped_column(String(100), nullable=True)
```

---

## LLM Extraction Changes

### Updated `EXTRACT_PROMPT` in `src/memory/extractor.py`

Add to extraction output:
```json
{
  "facts": [
    {
      "content": "User decided to use PostgreSQL",
      "category": "decision",
      "project": "Memex",
      "forget_after": "optional ISO datetime"
    }
  ]
}
```

Category and project are optional in the JSON — if LLM omits them, stored as NULL.

### Updated `ExtractedFact` dataclass

```python
@dataclass
class ExtractedFact:
    content: str
    forget_after: datetime | None = None
    category: str | None = None
    project: str | None = None
```

---

## Retrieval Changes

### ContextBuilder — memory display format

```python
if hit.category or hit.project or hit.created_at:
    parts = ["memory"]
    if hit.category: parts.append(hit.category)
    if hit.project: parts.append(hit.project)
    if hit.created_at: parts.append(hit.created_at.strftime("%Y-%m-%d"))
    tag = " | ".join(parts)
    sources_text += f"  [{tag}] {hit.content}\n"
else:
    sources_text += f"  [memory] {hit.content}\n"
```

### MemorySearch — optional category filter

`MemorySearch.search()` accepts optional `category: str | None = None`. When set, adds `WHERE category = :category` to the SQL query.

### API changes

- `GET /api/memory/list?category=research` — filter by category
- `POST /api/query` — add optional `memory_category` field
- `GET /api/memory/context` — profile groups memories by category in summary

---

## MCP Changes

### `recall` tool — add optional `category` parameter

```python
"category": {
    "type": "string",
    "enum": ["research", "reminder", "thought", "decision", "preference"],
    "description": "Filter memories by category (optional)"
}
```

---

## Out of Scope

- UI for browsing memories by category (existing `/api/memory/list` handles it)
- Manual category override by user
- Changing the ContextBuilder prompt (already improved in this branch as v2)

---

## Migrations Summary

| Migration | What |
|---|---|
| 0006 | Add `category VARCHAR(20)` and `project VARCHAR(100)` to memories |
