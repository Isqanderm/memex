# Rust Feature Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Устранить все функциональные расхождения между Python и Rust версиями Memex: выровнять API-поля, добавить два недостающих эндпоинта, EPUB-адаптер и полноценный MCP-сервер в виде отдельного Rust бинарника.

**Architecture:** Tasks 1–4 — точечные правки в `rust/src/api/` и `rust/src/ingestion/`. Task 5 — новый бинарник `rust/src/bin/mcp.rs`, который стартует независимо от HTTP-сервера: читает JSON-RPC с stdin, пишет в stdout (стандартный MCP stdio-транспорт через `rmcp`), переиспользует те же сервисные слои (`MemorySvc`, `RetrievalService`).

**Tech Stack:** Rust, axum 0.7, rmcp 0.1 (MCP stdio transport), epub crate, rusqlite, tokio

---

## Файловая структура

| Файл | Действие | Что меняем |
|------|----------|------------|
| `rust/src/api/memories.rs` | Modify | `relation` в `MemoryItem`; `serde rename` в `ProfileResponse` |
| `rust/src/api/documents.rs` | Modify | Новые эндпоинты `GET /:id/file` и `PATCH /:id` |
| `rust/src/db/repositories/documents.rs` | Modify | Метод `update_title` |
| `rust/src/db/repositories/memories.rs` | Modify | Добавить `relation` в SELECT |
| `rust/src/ingestion/adapters/epub.rs` | Create | `EpubAdapter` |
| `rust/src/ingestion/adapters/mod.rs` | Modify | Регистрация `EpubAdapter` |
| `rust/templates/upload.html` | Modify | Вернуть EPUB в список форматов |
| `rust/src/bin/mcp.rs` | Create | MCP сервер (9 инструментов) |
| `rust/Cargo.toml` | Modify | Добавить `epub`, `rmcp` |
| `tests/golden/test_memory.py` | Modify | Упростить `_assert_context_shape` после выравнивания полей |

---

## Task 1: Выровнять API-поля — `relation` и `context`

**Files:**
- Modify: `rust/src/api/memories.rs`
- Modify: `rust/src/db/repositories/memories.rs`
- Modify: `tests/golden/test_memory.py`

После этой задачи:
- `GET /api/memory/list` возвращает поле `relation` (как в Python)
- `GET /api/memory/context` возвращает `{static, dynamic, raw_count}` (как в Python, сейчас `static_summary`/`dynamic_summary`)

- [ ] **Step 1: Обновить `MemoryItem` и `ProfileResponse` в memories.rs**

Открыть `rust/src/api/memories.rs`. Найти структуры и заменить:

```rust
// Было:
#[derive(Serialize)]
pub struct MemoryItem {
    pub id: String,
    pub content: String,
    pub source: String,
    pub category: Option<String>,
    pub project: Option<String>,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct ProfileResponse {
    pub static_summary: String,
    pub dynamic_summary: String,
    pub raw_count: usize,
}

// Стало:
#[derive(Serialize)]
pub struct MemoryItem {
    pub id: String,
    pub content: String,
    pub source: String,
    pub category: Option<String>,
    pub project: Option<String>,
    pub relation: Option<String>,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct ProfileResponse {
    // "static" — зарезервированное слово в Rust, используем serde rename
    #[serde(rename = "static")]
    pub static_summary: String,
    #[serde(rename = "dynamic")]
    pub dynamic_summary: String,
    pub raw_count: usize,
}
```

- [ ] **Step 2: Добавить `relation` в SQL-запрос в репозитории**

Открыть `rust/src/db/repositories/memories.rs`. Найти функцию `list_active` (или аналогичную, которую вызывает `list_memories` эндпоинт). Добавить `relation` в SELECT и маппинг строки:

```rust
// Найти запрос типа:
//   SELECT id, content, source, category, project, created_at FROM memories ...
// Заменить на:
//   SELECT id, content, source, category, project, relation, created_at FROM memories ...

// В маппинге строки добавить:
pub relation: Option<String>,  // row.get(5)?  — проверить индекс по позиции
```

Точный индекс зависит от порядка колонок в SELECT. Убедиться что `created_at` — последний, `relation` — предпоследний.

- [ ] **Step 3: Убедиться что MemoryItem создаётся с полем relation**

Найти место в `memories.rs` где строится `MemoryItem` из данных репозитория. Добавить `relation: m.relation` (где `m` — объект из БД).

- [ ] **Step 4: Скомпилировать**

```bash
cargo build --manifest-path rust/Cargo.toml 2>&1 | grep "^error" | head -20
```

Ожидаемый результат: нет ошибок компиляции.

- [ ] **Step 5: Обновить golden-тест**

После выравнивания полей оба бэкенда возвращают `{static, dynamic}`. Упростить хелпер в `tests/golden/test_memory.py`:

```python
# Было:
def _assert_context_shape(data: dict) -> None:
    assert "raw_count" in data, f"Missing 'raw_count' in context: {data}"
    has_python = "static" in data and "dynamic" in data
    has_rust = "static_summary" in data and "dynamic_summary" in data
    assert has_python or has_rust, (
        f"Context must have (static+dynamic) or (static_summary+dynamic_summary), got: {list(data.keys())}"
    )

# Стало:
def _assert_context_shape(data: dict) -> None:
    assert "raw_count" in data, f"Missing 'raw_count' in context: {data}"
    assert "static" in data, f"Missing 'static' in context: {data}"
    assert "dynamic" in data, f"Missing 'dynamic' in context: {data}"
```

- [ ] **Step 6: Коммит**

```bash
git add rust/src/api/memories.rs rust/src/db/repositories/memories.rs tests/golden/test_memory.py
git commit -m "fix(rust): add relation field to MemoryItem, rename context fields to match Python"
```

---

## Task 2: GET /api/documents/:id/file

**Files:**
- Modify: `rust/src/api/documents.rs`

Эндпоинт читает файл с диска по пути `doc.source` и отдаёт его с правильным `Content-Type` и `Content-Disposition`.

- [ ] **Step 1: Добавить роут в `router()`**

В функции `router()` в `rust/src/api/documents.rs` добавить:

```rust
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/documents", post(upload_document))
        .route("/api/documents", get(list_documents))
        .route("/api/documents/:id", delete(delete_document))
        .route("/api/documents/:id", patch(update_document))   // Task 3
        .route("/api/documents/:id/file", get(get_document_file)) // ← добавить
}
```

- [ ] **Step 2: Добавить импорты**

В начале `documents.rs` убедиться что есть:

```rust
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::body::Body;
```

- [ ] **Step 3: Реализовать хендлер**

```rust
async fn get_document_file(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
) -> Result<Response<Body>, AppError> {
    let pool = state.pool.clone();
    let doc_id_clone = doc_id.clone();

    let doc = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(AppError::Pool)?;
        let doc_repo = DocumentRepository::new(&conn);
        doc_repo
            .get_by_id(&doc_id_clone)
            .map_err(AppError::Db)?
            .ok_or_else(|| AppError::NotFound(format!("document {doc_id_clone}")))
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??;

    let file_bytes = tokio::fs::read(&doc.source)
        .await
        .map_err(|e| AppError::Parse(format!("cannot read file: {e}")))?;

    let mime = mime_guess::from_path(&doc.source)
        .first_or_octet_stream()
        .to_string();

    // Извлечь оригинальное имя файла из пути (убрать checksum-префикс если есть)
    let filename = std::path::Path::new(&doc.source)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download")
        .to_string();

    // Убрать checksum-префикс "{16chars}-" если он есть
    let display_name = if filename.len() > 17 && filename.chars().nth(16) == Some('-') {
        filename[17..].to_string()
    } else {
        filename
    };

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{display_name}\""),
        )
        .body(Body::from(file_bytes))
        .map_err(|e| AppError::Parse(format!("response build error: {e}")))?;

    Ok(response)
}
```

- [ ] **Step 4: Скомпилировать**

```bash
cargo build --manifest-path rust/Cargo.toml 2>&1 | grep "^error" | head -20
```

Ожидаемый результат: нет ошибок.

- [ ] **Step 5: Коммит**

```bash
git add rust/src/api/documents.rs
git commit -m "feat(rust): add GET /api/documents/:id/file endpoint"
```

---

## Task 3: PATCH /api/documents/:id

**Files:**
- Modify: `rust/src/db/repositories/documents.rs`
- Modify: `rust/src/api/documents.rs`

Обновление заголовка документа. Python-версия обновляет `title` и `tags`; в Rust схеме нет `tags` (нет JSONB), поэтому обновляем только `title`.

- [ ] **Step 1: Добавить `update_title` в репозиторий**

В `rust/src/db/repositories/documents.rs` добавить метод:

```rust
pub fn update_title(&self, id: &str, title: Option<&str>) -> rusqlite::Result<bool> {
    let rows = self.conn.execute(
        "UPDATE documents SET title = ?1 WHERE id = ?2",
        rusqlite::params![title, id],
    )?;
    Ok(rows > 0)
}
```

- [ ] **Step 2: Добавить структуру запроса и хендлер в documents.rs**

```rust
#[derive(serde::Deserialize)]
pub struct UpdateDocumentRequest {
    pub title: Option<String>,
}

#[derive(Serialize)]
pub struct UpdateDocumentResponse {
    pub id: String,
    pub title: Option<String>,
}

async fn update_document(
    State(state): State<AppState>,
    Path(doc_id): Path<String>,
    Json(req): Json<UpdateDocumentRequest>,
) -> Result<Json<UpdateDocumentResponse>, AppError> {
    let pool = state.pool.clone();
    let doc_id_clone = doc_id.clone();
    let title = req.title.clone();

    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(AppError::Pool)?;
        let doc_repo = DocumentRepository::new(&conn);

        // Проверить что документ существует
        let _doc = doc_repo
            .get_by_id(&doc_id_clone)
            .map_err(AppError::Db)?
            .ok_or_else(|| AppError::NotFound(format!("document {doc_id_clone}")))?;

        doc_repo
            .update_title(&doc_id_clone, title.as_deref())
            .map_err(AppError::Db)?;

        Ok::<_, AppError>(UpdateDocumentResponse {
            id: doc_id_clone,
            title,
        })
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??
    .map(Json)
    // Упростить: spawn_blocking возвращает Result<Result<T>>, нужна цепочка:
}
```

Упрощённый вариант завершения:

```rust
    let result = tokio::task::spawn_blocking(move || -> Result<UpdateDocumentResponse, AppError> {
        let conn = pool.get().map_err(AppError::Pool)?;
        let doc_repo = DocumentRepository::new(&conn);
        let _doc = doc_repo
            .get_by_id(&doc_id_clone)
            .map_err(AppError::Db)?
            .ok_or_else(|| AppError::NotFound(format!("document {doc_id_clone}")))?;
        doc_repo.update_title(&doc_id_clone, title.as_deref()).map_err(AppError::Db)?;
        Ok(UpdateDocumentResponse { id: doc_id_clone, title })
    })
    .await
    .map_err(|e| AppError::Llm(format!("task join error: {e}")))??;

    Ok(Json(result))
}
```

- [ ] **Step 3: Добавить роут**

Убедиться что в `router()` есть `.route("/api/documents/:id", patch(update_document))` (добавлено в Task 2, Step 1).

- [ ] **Step 4: Скомпилировать и проверить**

```bash
cargo build --manifest-path rust/Cargo.toml 2>&1 | grep "^error" | head -20
```

Ожидаемый результат: нет ошибок.

- [ ] **Step 5: Коммит**

```bash
git add rust/src/db/repositories/documents.rs rust/src/api/documents.rs
git commit -m "feat(rust): add PATCH /api/documents/:id for title update"
```

---

## Task 4: EPUB адаптер

**Files:**
- Modify: `rust/Cargo.toml`
- Create: `rust/src/ingestion/adapters/epub.rs`
- Modify: `rust/src/ingestion/adapters/mod.rs`
- Modify: `rust/templates/upload.html`

- [ ] **Step 1: Добавить зависимость в Cargo.toml**

В секцию `[dependencies]` в `rust/Cargo.toml` добавить:

```toml
epub = "2"
```

- [ ] **Step 2: Создать `rust/src/ingestion/adapters/epub.rs`**

```rust
use std::path::Path;

use anyhow::Context;

use super::super::pipeline::{ParsedDocument, Section};
use super::DocumentAdapter;

pub struct EpubAdapter;

impl DocumentAdapter for EpubAdapter {
    fn can_handle(&self, path: &Path, _mime_type: &str) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("epub"))
            .unwrap_or(false)
    }

    fn parse(&self, path: &Path) -> anyhow::Result<ParsedDocument> {
        let source = path.to_string_lossy().into_owned();
        let mut doc = epub::doc::EpubDoc::new(path)
            .with_context(|| format!("Cannot open EPUB: {source}"))?;

        let title = doc.mdata("title").map(|s| s.to_string());

        let mut sections = Vec::new();
        let spine_len = doc.spine.len();

        for idx in 0..spine_len {
            if let Some((content, _mime)) = doc.get_resource_by_path(
                doc.spine.get(idx).cloned().unwrap_or_default().as_str(),
            ) {
                // Strip HTML tags — EPUB content is XHTML
                let text = strip_html(&String::from_utf8_lossy(&content));
                let trimmed = text.trim().to_string();
                if !trimmed.is_empty() {
                    sections.push(Section {
                        content: trimmed,
                        heading: None,
                        level: 0,
                        page_number: Some(idx as u32 + 1),
                    });
                }
            }
        }

        Ok(ParsedDocument {
            source,
            mime_type: "application/epub+zip".to_string(),
            sections,
            title,
            metadata: serde_json::json!({}),
        })
    }
}

fn strip_html(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}
```

- [ ] **Step 3: Зарегистрировать адаптер в `mod.rs`**

В `rust/src/ingestion/adapters/mod.rs`:

1. Добавить объявление модуля рядом с остальными:
```rust
pub mod epub;
```

2. Добавить import:
```rust
use epub::EpubAdapter;
```

3. В `build_default_registry()` добавить регистрацию:
```rust
pub fn build_default_registry() -> AdapterRegistry {
    let mut registry = AdapterRegistry::new();
    registry.register(PdfAdapter);
    registry.register(DocxAdapter);
    registry.register(XlsxAdapter);
    registry.register(PptxAdapter);
    registry.register(MarkdownAdapter);
    registry.register(TextAdapter);
    registry.register(EpubAdapter);  // ← добавить
    registry
}
```

- [ ] **Step 4: Вернуть EPUB в upload.html**

В `rust/templates/upload.html` найти:

```html
{% for fmt in ['PDF','DOCX','TXT','MD','PPTX','XLSX'] %}
```

Заменить на:

```html
{% for fmt in ['PDF','DOCX','TXT','MD','PPTX','XLSX','EPUB'] %}
```

И:
```html
accept=".pdf,.docx,.txt,.md,.pptx,.xlsx"
```

Заменить на:
```html
accept=".pdf,.docx,.txt,.md,.pptx,.xlsx,.epub"
```

- [ ] **Step 5: Скомпилировать**

```bash
cargo build --manifest-path rust/Cargo.toml 2>&1 | grep "^error" | head -20
```

Ожидаемый результат: нет ошибок.

- [ ] **Step 6: Коммит**

```bash
git add rust/Cargo.toml rust/Cargo.lock rust/src/ingestion/adapters/epub.rs \
        rust/src/ingestion/adapters/mod.rs rust/templates/upload.html
git commit -m "feat(rust): add EPUB adapter"
```

---

## Task 5: MCP сервер (`memex-mcp` бинарник)

**Files:**
- Modify: `rust/Cargo.toml`
- Create: `rust/src/bin/mcp.rs`

MCP сервер запускается как отдельный процесс рядом с HTTP-сервером. Читает JSON-RPC с stdin, пишет в stdout (stdio-транспорт MCP). Реализует 9 инструментов совместимых с Python `mcp_server.py`.

**Инструменты:**
| Имя | Описание |
|-----|----------|
| `remember` | Сохранить текст как факт в память |
| `recall` | Найти релевантные воспоминания по запросу |
| `context` | Получить профиль пользователя (static + dynamic summary) |
| `observe` | Извлечь факты из диалога |
| `memories` | Список всех активных воспоминаний |
| `index_file` | Проиндексировать файл с диска |
| `check_indexing` | Проверить статус задачи индексирования |
| `list_documents` | Список всех проиндексированных документов |
| `forget` | Удалить воспоминание по ID |

- [ ] **Step 1: Добавить `rmcp` в Cargo.toml**

```toml
[dependencies]
# ... существующие зависимости ...
rmcp = { version = "0.1", features = ["server", "transport-io"] }

[[bin]]
name = "memex-mcp"
path = "src/bin/mcp.rs"
```

Проверить что уже есть `[[bin]]` для `memex` и `memex-migrate` — добавить третий блок рядом.

- [ ] **Step 2: Создать `rust/src/bin/mcp.rs`**

```rust
//! Memex MCP server — stdio transport, compatible with Claude Code.
//!
//! Usage in .claude/settings.json:
//!   {
//!     "mcpServers": {
//!       "memex": {
//!         "command": "/path/to/memex-mcp",
//!         "env": { "DATABASE_PATH": "/path/to/data/memex.db" }
//!       }
//!     }
//!   }

use std::sync::Arc;

use anyhow::Context;
use rmcp::{
    ServerHandler,
    model::{
        CallToolResult, Content, Implementation, ListToolsResult, ServerCapabilities, ServerInfo,
        Tool, ToolInputSchema,
    },
    service::RequestContext,
    Error as McpError,
};
use serde_json::{json, Value};
use tracing::info;

use memex::{
    config::Config,
    db::pool::build_pool,
    ingestion::{EmbeddingClient, pipeline::IngestionPipeline, worker::IngestionWorker},
    llm::factory::build_llm,
    memory::{service::MemorySvc, profile::ProfileService, worker::MemoryWorker},
    search::{
        service::RetrievalService,
        tantivy_fts::TantivyStore,
        vectors::VectorStore,
    },
};

// ── Server state ────────────────────────────────────────────────────────────

#[derive(Clone)]
struct MemexServer {
    pool: Arc<memex::db::pool::DbPool>,
    config: Arc<Config>,
    memory_svc: Arc<MemorySvc>,
    retrieval_svc: Arc<RetrievalService>,
    profile_svc: Arc<ProfileService>,
    embed: Arc<EmbeddingClient>,
    vectors: Arc<VectorStore>,
}

impl MemexServer {
    async fn init() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        let config = Arc::new(Config::from_env());

        let pool = Arc::new(
            build_pool(&config.database_path).context("Failed to open SQLite")?,
        );
        let tantivy = Arc::new(
            TantivyStore::open(&config.tantivy_path).context("Failed to open Tantivy")?,
        );
        let embed = Arc::new(
            EmbeddingClient::new(&config.local_embedding_model)
                .context("Failed to load embedding model")?,
        );
        let vectors = Arc::new(VectorStore::new(pool.clone()));
        let llm = Arc::new(build_llm(&config));

        let retrieval_svc = Arc::new(RetrievalService::new(
            pool.clone(),
            tantivy.clone(),
            vectors.clone(),
            embed.clone(),
        ));
        let memory_svc = Arc::new(MemorySvc::new(pool.clone(), llm.clone()));
        let profile_svc = Arc::new(ProfileService::new(pool.clone(), llm.clone()));

        Ok(Self {
            pool,
            config,
            memory_svc,
            retrieval_svc,
            profile_svc,
            embed,
            vectors,
        })
    }
}

// ── Tool definitions ─────────────────────────────────────────────────────────

fn all_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "remember".into(),
            description: Some("Save text as a memory fact. Extracts and stores atomic facts using LLM.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "content": {"type": "string", "description": "Text to remember"},
                    "source": {"type": "string", "default": "explicit", "description": "Source label"}
                },
                "required": ["content"]
            }),
        },
        Tool {
            name: "recall".into(),
            description: Some("Find memories relevant to a query using semantic search.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "top_k": {"type": "integer", "default": 5},
                    "category": {"type": "string", "enum": ["research","reminder","insight","decision","preference"]}
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "context".into(),
            description: Some("Get user profile: static background + dynamic recent activity summary.".into()),
            input_schema: json!({"type": "object", "properties": {}}),
        },
        Tool {
            name: "observe".into(),
            description: Some("Extract facts from a conversation and store them as memories.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "conversation": {"type": "string", "description": "Conversation text to extract facts from"}
                },
                "required": ["conversation"]
            }),
        },
        Tool {
            name: "memories".into(),
            description: Some("List all active memories, optionally filtered by category.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "category": {"type": "string", "enum": ["research","reminder","insight","decision","preference"]}
                }
            }),
        },
        Tool {
            name: "index_file".into(),
            description: Some("Index a file from disk path into Memex for search.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Absolute path to file"}
                },
                "required": ["path"]
            }),
        },
        Tool {
            name: "check_indexing".into(),
            description: Some("Check the status of an indexing job by job_id.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "job_id": {"type": "string"}
                },
                "required": ["job_id"]
            }),
        },
        Tool {
            name: "list_documents".into(),
            description: Some("List all indexed documents.".into()),
            input_schema: json!({"type": "object", "properties": {}}),
        },
        Tool {
            name: "forget".into(),
            description: Some("Delete a memory by its ID.".into()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "memory_id": {"type": "string"}
                },
                "required": ["memory_id"]
            }),
        },
    ]
}

// ── ServerHandler implementation ─────────────────────────────────────────────

impl ServerHandler for MemexServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: Default::default(),
            capabilities: ServerCapabilities {
                tools: Some(Default::default()),
                ..Default::default()
            },
            server_info: Implementation {
                name: "memex".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            instructions: Some(
                "Memex personal RAG system. Use remember/recall for memory, index_file to add documents, recall/context to retrieve."
                    .into(),
            ),
        }
    }

    async fn list_tools(
        &self,
        _ctx: RequestContext<'_>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult { tools: all_tools() })
    }

    async fn call_tool(
        &self,
        name: &str,
        args: Option<Value>,
        _ctx: RequestContext<'_>,
    ) -> Result<CallToolResult, McpError> {
        let args = args.unwrap_or(Value::Object(Default::default()));
        let text = match name {
            "remember" => self.tool_remember(&args).await,
            "recall"   => self.tool_recall(&args).await,
            "context"  => self.tool_context().await,
            "observe"  => self.tool_observe(&args).await,
            "memories" => self.tool_memories(&args).await,
            "index_file" => self.tool_index_file(&args).await,
            "check_indexing" => self.tool_check_indexing(&args).await,
            "list_documents" => self.tool_list_documents().await,
            "forget"   => self.tool_forget(&args).await,
            _ => Err(format!("Unknown tool: {name}")),
        };

        match text {
            Ok(t) => Ok(CallToolResult {
                content: vec![Content::Text { text: t }],
                is_error: Some(false),
            }),
            Err(e) => Ok(CallToolResult {
                content: vec![Content::Text { text: e }],
                is_error: Some(true),
            }),
        }
    }
}

// ── Tool implementations ──────────────────────────────────────────────────────

impl MemexServer {
    async fn tool_remember(&self, args: &Value) -> Result<String, String> {
        let content = args["content"]
            .as_str()
            .ok_or("missing content")?
            .to_string();
        let source = args["source"].as_str().unwrap_or("explicit").to_string();

        let result = self
            .memory_svc
            .remember(&content, &source)
            .await
            .map_err(|e| e.to_string())?;

        Ok(format!(
            "Extracted {} facts, updated {} memories",
            result.facts_extracted, result.memories_updated
        ))
    }

    async fn tool_recall(&self, args: &Value) -> Result<String, String> {
        let query = args["query"].as_str().ok_or("missing query")?.to_string();
        let top_k = args["top_k"].as_u64().unwrap_or(5) as usize;
        let memory_category = args["category"].as_str().map(|s| s.to_string());

        let response = self
            .retrieval_svc
            .query(&query, top_k, memory_category.as_deref())
            .await
            .map_err(|e| e.to_string())?;

        Ok(response.answer)
    }

    async fn tool_context(&self) -> Result<String, String> {
        let profile = self
            .profile_svc
            .get_profile()
            .await
            .map_err(|e| e.to_string())?;

        Ok(format!(
            "## Background\n{}\n\n## Recent Activity\n{}\n\n({} raw facts stored)",
            profile.static_summary, profile.dynamic_summary, profile.raw_count
        ))
    }

    async fn tool_observe(&self, args: &Value) -> Result<String, String> {
        let conversation = args["conversation"]
            .as_str()
            .ok_or("missing conversation")?
            .to_string();

        let result = self
            .memory_svc
            .observe(&conversation)
            .await
            .map_err(|e| e.to_string())?;

        Ok(format!(
            "Extracted {} facts, updated {} memories",
            result.facts_extracted, result.memories_updated
        ))
    }

    async fn tool_memories(&self, args: &Value) -> Result<String, String> {
        let category = args["category"].as_str().map(|s| s.to_string());

        let pool = self.pool.clone();
        let memories = tokio::task::spawn_blocking(move || {
            let conn = pool.get().map_err(|e| e.to_string())?;
            let repo = memex::db::repositories::memories::MemoryRepository::new(&conn);
            repo.list_active(category.as_deref()).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

        if memories.is_empty() {
            return Ok("No memories found.".into());
        }
        let lines: Vec<String> = memories
            .iter()
            .map(|m| format!("[{}] {}", m.id, m.content))
            .collect();
        Ok(lines.join("\n"))
    }

    async fn tool_index_file(&self, args: &Value) -> Result<String, String> {
        let path = args["path"].as_str().ok_or("missing path")?.to_string();
        let path_buf = std::path::PathBuf::from(&path);

        if !path_buf.exists() {
            return Err(format!("File not found: {path}"));
        }

        // Compute checksum
        let bytes = std::fs::read(&path_buf).map_err(|e| e.to_string())?;
        use sha2::{Digest, Sha256};
        let checksum = hex::encode(Sha256::digest(&bytes));

        let pool = self.pool.clone();
        let checksum_clone = checksum.clone();
        let path_clone = path.clone();

        let job_id = tokio::task::spawn_blocking(move || {
            let conn = pool.get().map_err(|e| e.to_string())?;
            let doc_repo = memex::db::repositories::documents::DocumentRepository::new(&conn);
            let job_repo = memex::db::repositories::jobs::JobRepository::new(&conn);

            if let Some(doc) = doc_repo.get_by_checksum(&checksum_clone).map_err(|e| e.to_string())? {
                return Ok::<String, String>(format!("already_indexed:{}", doc.id));
            }
            if let Some(job) = job_repo.get_by_checksum_active(&checksum_clone).map_err(|e| e.to_string())? {
                return Ok(format!("already_queued:{}", job.id));
            }

            let job_id = job_repo.create(&path_clone, &checksum_clone).map_err(|e| e.to_string())?;
            Ok(format!("queued:{job_id}"))
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

        if job_id.starts_with("already_indexed:") {
            Ok(format!("Already indexed. doc_id={}", &job_id[16..]))
        } else if job_id.starts_with("already_queued:") {
            Ok(format!("Already queued. job_id={}", &job_id[15..]))
        } else {
            Ok(format!("Indexing started. job_id={}", &job_id[7..]))
        }
    }

    async fn tool_check_indexing(&self, args: &Value) -> Result<String, String> {
        let job_id = args["job_id"].as_str().ok_or("missing job_id")?.to_string();
        let pool = self.pool.clone();

        let status = tokio::task::spawn_blocking(move || {
            let conn = pool.get().map_err(|e| e.to_string())?;
            let repo = memex::db::repositories::jobs::JobRepository::new(&conn);
            repo.get_by_id(&job_id).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

        match status {
            None => Err("Job not found".into()),
            Some(job) => Ok(format!(
                "status={} doc_id={} error={}",
                job.status,
                job.doc_id.as_deref().unwrap_or("-"),
                job.error.as_deref().unwrap_or("-")
            )),
        }
    }

    async fn tool_list_documents(&self) -> Result<String, String> {
        let pool = self.pool.clone();
        let docs = tokio::task::spawn_blocking(move || {
            let conn = pool.get().map_err(|e| e.to_string())?;
            let repo = memex::db::repositories::documents::DocumentRepository::new(&conn);
            repo.list_all().map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

        if docs.is_empty() {
            return Ok("No indexed documents.".into());
        }
        let lines: Vec<String> = docs
            .iter()
            .map(|d| {
                format!(
                    "[{}] {} ({})",
                    d.id,
                    d.title.as_deref().unwrap_or(&d.source),
                    d.mime_type
                )
            })
            .collect();
        Ok(lines.join("\n"))
    }

    async fn tool_forget(&self, args: &Value) -> Result<String, String> {
        let memory_id = args["memory_id"].as_str().ok_or("missing memory_id")?.to_string();
        let pool = self.pool.clone();

        let deleted = tokio::task::spawn_blocking(move || {
            let conn = pool.get().map_err(|e| e.to_string())?;
            let repo = memex::db::repositories::memories::MemoryRepository::new(&conn);
            repo.delete(&memory_id).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

        if deleted {
            Ok("Memory deleted.".into())
        } else {
            Err("Memory not found.".into())
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr) // MCP: stdout reserved for protocol, log to stderr
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn".into()),
        )
        .init();

    info!("Starting Memex MCP server");

    let server = MemexServer::init().await?;
    let service = server.serve(rmcp::transport::io::stdin_stdout()).await?;
    service.waiting().await?;

    Ok(())
}
```

- [ ] **Step 3: Скомпилировать `memex-mcp`**

```bash
cargo build --release --manifest-path rust/Cargo.toml --bin memex-mcp 2>&1 | grep "^error" | head -30
```

Ожидаемый результат: бинарник `rust/target/release/memex-mcp`.

Если rmcp API отличается от использованного в плане — исправить по документации (`cargo doc --open --manifest-path rust/Cargo.toml -p rmcp`).

- [ ] **Step 4: Проверить что HTTP-сервер всё ещё компилируется**

```bash
cargo build --release --manifest-path rust/Cargo.toml --bin memex 2>&1 | grep "^error" | head -10
```

Ожидаемый результат: нет ошибок.

- [ ] **Step 5: Обновить README.md — раздел MCP для Rust**

В `README.md` найти раздел `### MCP (Claude Code)`. Добавить после существующего блока:

```markdown
#### Rust версия

```json
{
  "mcpServers": {
    "memex": {
      "command": "/path/to/memex-mcp",
      "env": {
        "DATABASE_PATH": "/path/to/data/memex.db",
        "TANTIVY_PATH": "/path/to/data/tantivy",
        "LLM_PROVIDER": "openai",
        "OPENAI_LLM_API_KEY": "sk-..."
      }
    }
  }
}
```

`memex-mcp` устанавливается вместе с `memex` бинарником из GitHub Release.
```

- [ ] **Step 6: Коммит**

```bash
git add rust/Cargo.toml rust/Cargo.lock rust/src/bin/mcp.rs README.md
git commit -m "feat(rust): add native MCP server binary (memex-mcp) with 9 tools"
```

---

## Self-Review

**Покрытие требований:**

| Требование | Задача |
|---|---|
| `relation` в MemoryItem | Task 1 |
| `static`/`dynamic` в context (выровнять с Python) | Task 1 |
| Упростить golden test после выравнивания | Task 1 |
| `GET /api/documents/:id/file` | Task 2 |
| `PATCH /api/documents/:id` | Task 3 |
| EPUB адаптер | Task 4 |
| MCP сервер (9 инструментов) | Task 5 |
| README MCP для Rust | Task 5 |

**Placeholder scan:** нет TBD. Весь код конкретен.

**Важные замечания для исполнителя:**

1. **Task 1, Step 2** — точный индекс `relation` в SQL row mapping зависит от порядка колонок в SELECT. Прочитать файл `memories.rs` перед изменением и сверить индексы.

2. **Task 5** — `rmcp` API может отличаться от использованного в плане (версия 0.1.x активно развивается). При ошибках компиляции смотреть `cargo doc` или примеры в репозитории `https://github.com/modelcontextprotocol/rust-sdk`. Ключевые точки адаптации: сигнатуры `list_tools`/`call_tool`, структуры `CallToolResult`/`Content`, метод `serve` и `stdin_stdout`.

3. **Tasks 2–3** — если в `AppError` нет нужного варианта для IO-ошибок, использовать `AppError::Parse(format!(...))` — он принимает `String`.
