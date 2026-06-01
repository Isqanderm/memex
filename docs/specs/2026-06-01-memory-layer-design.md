# Memory Layer Design

**Date:** 2026-06-01  
**Status:** Approved  
**Scope:** Add per-user evolving memory to Memex (Approach A — separate Memory layer)

---

## Problem

Memex is a document archive. When `remember("I now work at company Y")` is called, it stores the text as a document. If a contradicting fact was stored earlier ("I work at company X"), both will surface in search results. Memex has no concept of fact evolution, temporal context, or user identity.

The goal is to turn Memex into a system where personal facts evolve over time: new facts supersede old ones, information from conversations and documents is automatically extracted, and the user's current profile is always queryable.

---

## Architecture Overview

```
Sources:         remember()    observe()    upload(file)
                     ↓            ↓              ↓
Fact extraction:   [LLM: extract atomic facts + resolve relations]
                                  ↓
Storage:            memories table (is_active, parent_id, relation)
                                  ↓
Retrieval:   recall()  →  Memory search + Chunk search → RRF → LLM
             context() →  Active memories → LLM profile summary
```

The existing `documents`/`chunks` pipeline is unchanged. Memory facts live in a separate table and are merged at retrieval time.

---

## Data Model

### Table: `memories`

| Column | Type | Description |
|--------|------|-------------|
| `id` | UUID PK | |
| `content` | Text | Atomic fact: "User works at company Y" |
| `raw_input` | Text | Original text the fact was extracted from |
| `source` | String | `"explicit"` / `"conversation"` / `"document"` |
| `is_active` | Bool | False when superseded or expired |
| `forget_after` | DateTime? | Auto-expiry for time-bound facts |
| `relation` | String? | `"updates"` / `"extends"` / `"derives"` — how this relates to parent |
| `parent_id` | UUID? | FK → memories.id (the fact this one updates/extends/derives) |
| `content_vector` | Vector(1536) | For semantic search |
| `created_at` | DateTime | |

### Table: `memory_extraction_jobs`

| Column | Type | Description |
|--------|------|-------------|
| `id` | UUID PK | |
| `source_ref` | String | doc_id or conversation identifier |
| `source` | String | `"document"` / `"conversation"` |
| `status` | String | `"pending"` / `"processing"` / `"done"` / `"error"` |
| `facts_extracted` | Int | Count of memories created |
| `error` | Text? | |
| `created_at` | DateTime | |

### Version graph example

```
Memory #1: "User works at company X"     is_active=False
           ↑ parent_id
Memory #2: "User works at company Y"     is_active=True,  relation="updates"
           ↑ parent_id
Memory #3: "User is Lead Engineer at Y"  is_active=True,  relation="extends"
```

`recall` and `context` return only `is_active=True` facts.

---

## Ingestion Pipelines

### Source 1: Explicit command — `remember(text)`

Synchronous. Returns result immediately.

```
remember("I now work at Y")
    ↓
LLM: extract atomic facts from text
    → ["User works at company Y"]
    ↓
For each fact:
    embed(fact)
    find top-5 similar active memories (cosine similarity > 0.75)
    ↓
    If similar found → LLM: determine relation
        "updates"  → mark old as is_active=False, save with parent_id
        "extends"  → save with parent_id, both stay active
        "derives"  → save with parent_id, both stay active
        "new"      → save without parent_id
    ↓
    If no similar found → save as new fact
```

### Source 2: Document upload

Asynchronous. Does not block the upload response.

```
upload(resume.pdf)
    ↓
[existing pipeline: parse → chunk → embed → Document/Chunk table]
    ↓
create MemoryExtractionJob(source="document", source_ref=doc_id)
    ↓
[background worker]
    LLM: "Extract personal facts about the user from this document text"
    → list of atomic facts
    ↓
    each fact → same resolve + store flow as explicit command
```

### Source 3: Conversation — `observe(conversation)`

Asynchronous (queued). Agent calls this at end of session.

```
observe(conversation_history)
    ↓
LLM: "What new facts about the user did I learn in this conversation?"
    → list of facts (only new information, not recap)
    ↓
    each fact → same resolve + store flow
```

---

## LLM Prompts

### Fact extraction

```
Extract atomic facts about the user from the following text.
Each fact must be a single statement with no pronouns — use "User" as subject.
Ignore temporary facts unless a specific date is mentioned.
Text: {text}
Return JSON: {"facts": ["...", "..."]}
```

### Relation resolution

```
New fact: "{new_fact}"
Existing similar facts:
{existing_facts_with_ids}

Determine the relation of the new fact to each existing fact:
- updates: new fact contradicts and supersedes the old one
- extends: new fact adds detail without contradiction
- derives: new fact is logically inferred from the old one
- new: not related to any existing fact

Return JSON: {"relations": [{"id": "...", "type": "updates|extends|derives|new"}]}
```

### Temporal expiry detection (within extraction prompt)

LLM marks time-bound facts with a `forget_after` field:

```
If a fact is time-bound (e.g. "has a meeting tomorrow"), include "forget_after": "ISO datetime".
For permanent facts, omit the field.
```

---

## Retrieval

### `recall(query)` — updated behavior

Searches both memories and document chunks, merges via RRF.

```
recall("where do I work?")
    ↓
parallel:
    A) semantic search on memories (is_active=True only)
    B) existing hybrid search on chunks (semantic + BM25)
    ↓
RRF fusion — memory results get a rank boost (+10 positions)
    ↓
LLM synthesizes answer from top results
    ↓
response marks source: [memory] or [document: filename.pdf]
```

### `context()` — new tool

Returns structured user profile. Agent calls this at session start.

```
context()
    ↓
fetch all active memories, sorted by created_at DESC
split into:
    static  — facts older than 30 days
    dynamic — facts from last 30 days
    ↓
LLM: compress each group into ≤150 tokens of plain text
    ↓
return:
{
  "static":  "Senior fullstack developer. Prefers Python and TypeScript...",
  "dynamic": "Currently building Memex, a self-hosted RAG system...",
  "raw_count": 42
}
```

Agent injects into system prompt:
```
User profile: {static}
Recent context: {dynamic}
```

### Auto-expiry cron job

Runs hourly. Marks time-bound facts as inactive:

```sql
UPDATE memories SET is_active = FALSE
WHERE forget_after < NOW() AND is_active = TRUE;
```

---

## MCP Tools

### Changed tools

| Tool | Before | After |
|------|--------|-------|
| `remember(content)` | Saves text as document | Extracts facts via LLM, resolves conflicts, saves to `memories` |
| `recall(query)` | Searches chunks only | Searches memories + chunks, marks source in response |
| `forget(id)` | Deletes document by doc_id | Accepts doc_id or memory_id |

### New tools

**`context()`**
- No parameters
- Returns: `{ static, dynamic, raw_count }`
- When to call: first tool at session start

**`observe(conversation)`**
- Parameter: `conversation` (string) — full session history
- Returns: `{ facts_extracted, memories_updated }`
- When to call: last tool at session end

**`memories()`**
- No parameters
- Returns: list of active Memory records with content, source, created_at, relation
- Replaces `list_memories` for the memory layer

### Typical agent session flow

```
[session start]
→ context()
  "User: senior dev, working on Memex..."

[during session]
→ recall("how do I usually name variables?")
→ remember("decided to use snake_case for all new modules")

[session end]
→ observe("<full conversation history>")
  "3 facts extracted, 1 memory updated"
```

---

## Out of Scope

- Multi-user support (single-user personal tool)
- Memory graph visualization (existing `memory-graph-playground` app handles this)
- Connector sync (Slack, Notion, etc.)
- Changing the existing document/chunk pipeline

---

## R&D Plan

Before committing to full implementation, three research questions need empirical answers. Each experiment is independent and can run in parallel.

---

### RQ1: Does the memory layer actually improve answer quality?

**Hypothesis:** `recall()` with memory augmentation gives more accurate and up-to-date answers than pure document RAG on personal/evolving facts.

**Method:**
1. Build an eval dataset of 50–100 synthetic conversation turns where the user states facts about themselves (job, location, preferences, projects). Include knowledge updates — contradicting facts stated at different points in time.
2. After ingestion, run the same set of questions through two systems:
   - **Baseline:** current Memex `recall()` (chunks only)
   - **Memory:** new `recall()` (memories + chunks, RRF-merged)
3. Score answers using LLM-as-judge with the same prompt Supermemory uses in their LongMemEval evaluation.

**Metrics:**
- Answer accuracy per category: single-session, multi-session, knowledge-update, temporal-reasoning, preference
- Overall accuracy delta vs baseline

**Success criterion:** Memory layer improves overall accuracy by ≥10 percentage points vs baseline. Knowledge-update category specifically must improve (this is where pure RAG is weakest).

**Estimated effort:** 2–3 days (dataset creation + eval harness + one run).

---

### RQ2: How accurately does the LLM engine extract facts and resolve relations?

**Hypothesis:** The extraction and relation-resolution prompts work correctly on realistic personal-knowledge inputs.

**Method:**
1. Build a labeled dataset of 30–50 input texts (explicit `remember()` calls, short conversation excerpts, brief document excerpts). For each, manually annotate:
   - Expected extracted facts (ground truth)
   - For pairs of conflicting/related facts: expected relation type (`updates` / `extends` / `derives` / `new`)
2. Run the extraction prompt over all inputs, compare to ground truth.
3. Run the relation prompt over all labeled fact pairs, compare to ground truth.

**Metrics:**
- Fact extraction: precision (no hallucinated facts), recall (no missed facts), F1
- Relation classification: accuracy per relation type, confusion matrix

**Success criterion:**
- Extraction precision ≥ 0.90 (hallucinations are worse than missed facts)
- Extraction recall ≥ 0.80
- Relation accuracy ≥ 0.85, specifically `updates` recall ≥ 0.90 (missing an update means stale facts survive)

**Estimated effort:** 1–2 days (labeling + eval script).

---

### RQ3: What is the cost and latency of the memory layer?

**Hypothesis:** The LLM overhead per `remember()` and the latency added to `recall()` are acceptable for a personal tool.

**Method:**
1. Instrument the `remember()` flow: measure tokens (input + output) for extraction call and resolution call separately. Run over 50 realistic inputs covering short facts, long paragraphs, and conversation excerpts.
2. Measure end-to-end latency for `recall()` with and without memory search, at varying memory store sizes (10 / 100 / 500 facts).
3. Measure `context()` latency (profile generation) at the same sizes.

**Metrics:**
- Average tokens per `remember()`: extraction call + resolution call
- Estimated cost per `remember()` at current Claude pricing
- `recall()` latency delta: with memory vs without, p50 and p95
- `context()` latency at 10 / 100 / 500 facts

**Success criterion:**
- `remember()` costs < $0.01 per call on average (acceptable for personal use)
- `recall()` latency increase < 200ms p95 vs baseline
- `context()` completes in < 2s at 500 facts

**Estimated effort:** 1 day (instrumentation + benchmark script).

---

### Research infrastructure

All three experiments share:

```
tests/research/
    datasets/
        rq1_eval_conversations.json   # labeled Q&A eval set
        rq2_extraction_cases.json     # labeled fact extraction cases
        rq2_relation_cases.json       # labeled relation pairs
    rq1_eval.py                       # answer quality benchmark
    rq2_extraction_eval.py            # extraction + relation accuracy
    rq3_benchmark.py                  # cost + latency measurement
    results/                          # output JSON reports (gitignored)
```

Results should be written as JSON reports so they can be re-run and compared across prompt iterations.

---

### Decision gate

R&D runs **before** implementing the full ingestion pipeline and MCP changes. The implementation proceeds only if:
- RQ1 shows meaningful quality improvement (otherwise the layer adds cost with no benefit)
- RQ2 shows extraction precision ≥ 0.90 (otherwise prompt iteration is needed first)
- RQ3 confirms cost and latency are within the success criteria

If RQ2 or RQ3 fail their criteria, the prompts and/or architecture are revised before proceeding.

---

## Open Questions

None — all decisions were made during design review.
