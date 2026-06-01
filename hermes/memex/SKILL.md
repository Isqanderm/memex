---
name: memex
description: "Personal memory and RAG system — evolving facts about the user, document search, session context. Use at session start (context), during session (remember/recall), and at session end (observe)."
license: MIT
compatibility: Requires Memex running at http://memex:8000 via Docker. See https://github.com/Isqanderm/memex for setup.
metadata:
  author: Isqanderm
  version: "2.0.0"
  tags: "memory rag knowledge-base documents search context"
---

# Memex — Personal Memory & Knowledge Base

Memex stores evolving facts about the user and indexes documents for semantic search. Facts are extracted automatically by LLM, conflict-resolved (e.g. "moved to Berlin" supersedes "lives in Moscow"), and injected into every `recall()` response.

## Session Protocol (MANDATORY)

**Start of every session:**
```
mcp_memex_context()   ← inject user profile into your system context
```

**End of every session:**
```
mcp_memex_observe(conversation)   ← extract new facts from this conversation
```

## When to Use Which Tool

| Signal | Tool |
|---|---|
| Session just started | `mcp_memex_context` |
| "remember", "запомни", "save this" | `mcp_memex_remember` |
| "recall", "find", "what do you know about" | `mcp_memex_recall` |
| User provides a file to index | `mcp_memex_index_file` |
| "what facts do you have about me?" | `mcp_memex_memories` |
| "list my documents" | `mcp_memex_list_memories` |
| "forget", "delete" | `mcp_memex_forget` |
| Session is ending | `mcp_memex_observe` |

---

## Tools

### mcp_memex_context — User profile (call at session START)

No arguments. Returns:
- `static` — stable facts about the user (location, job, preferences)
- `dynamic` — recent activity and current projects
- `raw_count` — total number of stored facts

**Inject into your context:** `"User profile: {static}. Recent: {dynamic}."`

### mcp_memex_remember — Save text as a memory

Arguments:
- `content` (required) — text to remember

**Returns immediately** with `facts_extracted` and `memories_updated` counts. No job_id, no polling required. The LLM automatically extracts atomic facts and resolves conflicts with existing memories.

Example:
```
mcp_memex_remember("I now work at Acme Corp as a senior engineer")
→ {"facts_extracted": 1, "memories_updated": 0}
```

### mcp_memex_recall — Search memories and documents

Arguments:
- `query` (required) — question or topic in the same language as stored content
- `raw` (default false) — false = LLM answer; true = raw chunks (faster/cheaper)
- `top_k` (default 5) — used only when raw=true

Personal memory facts are automatically included in the response alongside document chunks.

### mcp_memex_observe — Extract facts from conversation (call at session END)

Arguments:
- `conversation` (required) — full conversation history as text

Extracts new personal facts from the conversation and saves them. Call this as the last tool before ending a session.

Returns `facts_extracted` and `memories_updated`.

### mcp_memex_memories — List active memory facts

No arguments. Returns all active facts about the user with content, source, and timestamp. Use when user asks "what do you know about me?".

### mcp_memex_index_file — Index a file from disk

Arguments:
- `path` (required) — absolute path to file
- `tags` (optional) — list of strings

Supports: PDF, DOCX, MD, TXT, PPTX, XLSX, EPUB.
Returns `job_id`. **Call `mcp_memex_check_indexing` after this.**

### mcp_memex_check_indexing — Poll indexing status

Arguments:
- `job_id` (required)

Returns: `pending` / `processing` / `done` / `error`.
- If `pending` or `processing` — wait 3 seconds and call again
- If `done` — document is ready to search

### mcp_memex_list_memories — List indexed documents

No arguments. Returns all documents with id, title, tags, date.

### mcp_memex_forget — Delete a memory or document

Arguments:
- `doc_id` (required) — memory id (from `mcp_memex_memories`) or document id

---

## Typical Session

```
[Session start]
mcp_memex_context()
→ "User profile: Senior fullstack engineer. Prefers Python.
   Recent: Working on Memex memory layer."

[During session — user says "remember I switched to TypeScript"]
mcp_memex_remember("I now use TypeScript for all new projects")
→ {"facts_extracted": 1, "memories_updated": 1}  # superseded Python preference

[User asks something]
mcp_memex_recall("what stack does the user prefer?")
→ Answer citing memory facts + documents

[Session end]
mcp_memex_observe("<full conversation text>")
→ {"facts_extracted": 2, "memories_updated": 0}
```

## Notes

- **`remember` is now synchronous** — no job_id, no polling. Returns immediately.
- **`remember` has no title/tags** — facts are extracted automatically by LLM.
- **Language** — query language should match stored content for best results.
- Memex runs at `http://memex:8000` — internal Docker network only.
