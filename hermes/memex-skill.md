---
name: memex
description: "Long-term memory for Hermes — store, search and manage documents via Memex RAG"
version: 1.0.0
platforms: [linux]
metadata:
  hermes:
    tags: [memory, rag, knowledge-base, documents]
    related_skills: []
---

## When to Use

- User says "remember", "save", "store", "note this down" → `mcp_memex_remember`
- User says "recall", "find", "what do I know about", "search memory" → `mcp_memex_recall`
- User says "index this file", "add this PDF/doc to memory" → `mcp_memex_index_file`
- User says "list everything", "what's in memory", "show all docs" → `mcp_memex_list_memories`
- User says "forget", "delete from memory", "remove" → `mcp_memex_forget`
- After any `remember` or `index_file` call → poll with `mcp_memex_check_indexing`

## Tools

### mcp_memex_remember
Save text as a memory.

Arguments:
- `content` (required) — text to store
- `title` (optional) — short label; auto-derived from first 60 chars if omitted
- `tags` (optional) — string array, e.g. `["meeting", "client", "q3"]`

Returns: `job_id`. Always follow up with `check_indexing`.

### mcp_memex_recall
Search the knowledge base.

Arguments:
- `query` (required) — natural language question or topic
- `raw` (optional, default `false`) — `true` returns raw chunks without LLM synthesis; faster and cheaper when you just need excerpts
- `top_k` (optional, default `5`) — number of chunks to return (only applies when `raw=true`)

Returns: synthesised answer with sources (raw=false) or raw text chunks (raw=true).

### mcp_memex_index_file
Index a file from disk.

Arguments:
- `path` (required) — absolute path to file (PDF, DOCX, MD, TXT, PPTX, XLSX, EPUB)
- `tags` (optional) — string array

Returns: `job_id`. Always follow up with `check_indexing`.

### mcp_memex_check_indexing
Poll indexing status.

Arguments:
- `job_id` (required) — from `remember` or `index_file`

Returns: `pending` / `processing` / `done` (with doc_id) / `error`.

### mcp_memex_list_memories
List all documents. No arguments. Returns id, title, tags, mime type, date.

### mcp_memex_forget
Delete a document.

Arguments:
- `doc_id` (required) — from `list_memories` or `recall` sources

## Workflow

### Storing a memory
```
remember(content, title?, tags?)
  → job_id
  → check_indexing(job_id)          # repeat until "done"
  → confirm to user: "Saved."
```

### Retrieving a memory
```
recall(query)
  → answer + sources
  → present to user
```

### When to use raw=true
Use `recall(query, raw=true)` when you need raw excerpts to reason over yourself,
rather than a pre-synthesised LLM answer. Faster and uses fewer tokens.

## Notes

- Indexing is always async — never skip `check_indexing` after `remember` or `index_file`
- `tags` help scope future searches; use consistent tags within a project or topic
- `recall` searches both semantic (meaning) and full-text (keywords) — natural language queries work best
- `forget` is permanent and immediate
