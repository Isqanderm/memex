---
name: memex
description: "Personal RAG memory system — save, search, and recall information across documents and notes. Use when the user wants to remember something, find stored information, index a file, list or delete memories."
license: MIT
compatibility: Requires Memex running at http://memex:8000 via Docker. See https://github.com/Isqanderm/memex for setup.
metadata:
  author: Isqanderm
  version: "1.0.0"
  tags: "memory rag knowledge-base documents search"
---

# Memex — Personal Knowledge Base

Memex is a personal RAG system. Use it to store, index, and semantically search documents and notes via MCP tools.

## When to Use Memex

- User says "remember", "запомни", "save", "сохрани" → `mcp_memex_remember`
- User says "recall", "find", "найди", "что ты знаешь о" → `mcp_memex_recall`
- User provides a file to index → `mcp_memex_index_file`
- User wants to see what's stored → `mcp_memex_list_memories`
- User wants to delete something → `mcp_memex_forget`

## Tools

### mcp_memex_remember — Save text as a memory

Arguments:
- `content` (required) — text to remember
- `title` (optional but recommended) — short title
- `tags` (optional) — list of strings for organisation

**Always suggest tags** based on content topic:
- Personal preferences → `["personal", "preferences"]`
- Meeting notes → `["meeting", "work"]`
- Technical info → `["tech", "reference"]`
- Project-specific → `["project-name"]`

Returns `job_id`. **You MUST call `mcp_memex_check_indexing` after this — the memory is NOT saved until the job is done.**

### mcp_memex_recall — Search memories

Arguments:
- `query` (required) — question or topic **in the same language as the stored content**
- `raw` (default false) — false = LLM synthesises answer; true = raw chunks, faster/cheaper
- `top_k` (default 5) — used only when raw=true

### mcp_memex_index_file — Index a file from disk

Arguments:
- `path` (required) — absolute path to file
- `tags` (optional) — list of strings

Supports: PDF, DOCX, MD, TXT, PPTX, XLSX, EPUB.
Returns `job_id`. **You MUST call `mcp_memex_check_indexing` after this.**

### mcp_memex_check_indexing — Poll indexing status

Arguments:
- `job_id` (required)

Returns: `pending` / `processing` / `done` / `error`.
- If `pending` or `processing` — wait 3 seconds and call again
- If `done` — memory is ready to search
- If `error` — report error to user

### mcp_memex_list_memories — List all documents

No arguments. Returns all documents with id, title, tags, date.

### mcp_memex_forget — Delete a memory

Arguments:
- `doc_id` (required)

## Workflows

### Save a note (MANDATORY sequence)

```
1. mcp_memex_remember(content, title, tags)
   → returns job_id
2. mcp_memex_check_indexing(job_id)    ← REQUIRED, do not skip
   → if "pending"/"processing": wait 3s, repeat step 2
   → if "done": memory is saved, confirm to user
   → if "error": report to user
```

**Do NOT confirm "Saved" before check_indexing returns "done".** Skipping check_indexing leaves the document without title and tags.

### Answer from memory

```
1. mcp_memex_recall(query)
2. Present the answer and cite sources
3. If no results: try rephrasing or say nothing was found
```

### Index a file

```
1. mcp_memex_index_file(path, tags)
   → returns job_id
2. mcp_memex_check_indexing(job_id)    ← REQUIRED
   → poll until "done" or "error"
```

## Notes

- **Language**: query language should match the stored content language for best results
- Memex runs at `http://memex:8000` — internal Docker network only, not exposed externally
