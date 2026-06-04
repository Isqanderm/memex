# Task 1: Project Scaffold

**Goal:** Создать Rust-проект `rust/` с Cargo.toml, точкой входа, конфигом и типом ошибок.

**Files:**
- Create: `rust/Cargo.toml`
- Create: `rust/src/main.rs`
- Create: `rust/src/config.rs`
- Create: `rust/src/error.rs`

---

### Task 1.1: Cargo.toml

- [ ] **Шаг 1: Создать rust/Cargo.toml**

```toml
[package]
name = "memex"
version = "3.0.0"
edition = "2021"

[[bin]]
name = "memex"
path = "src/main.rs"

[dependencies]
# HTTP сервер
axum = { version = "0.7", features = ["macros", "multipart"] }
tokio = { version = "1", features = ["full"] }
tower = "0.4"
tower-http = { version = "0.5", features = ["fs", "trace"] }

# База данных
rusqlite = { version = "0.31", features = ["bundled", "load_extension"] }
r2d2 = "0.8"
r2d2_sqlite = "0.24"
sqlite-vec = "0.1"

# Полнотекстовый поиск
tantivy = "0.22"

# Эмбеддинги и ранжировщик
fastembed = "4"

# Сериализация
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# HTTP клиент (для LLM API)
reqwest = { version = "0.12", features = ["json", "stream"] }

# Типы ошибок
thiserror = "2"
anyhow = "1"

# UUID
uuid = { version = "1", features = ["v4"] }

# Дата/время
chrono = { version = "0.4", features = ["serde"] }

# SHA256 для чексумм
sha2 = "0.10"
hex = "0.4"

# Шаблоны
minijinja = { version = "2", features = ["loader"] }

# Парсинг документов
pulldown-cmark = "0.12"
calamine = { version = "0.25", features = ["dates"] }
zip = "2"
quick-xml = "0.37"

# Определение языка
whichlang = "0.1"

# Конфиг
dotenvy = "0.15"

# Логирование
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# Async streaming
tokio-stream = "0.1"
futures = "0.3"
bytes = "1"

[dev-dependencies]
tempfile = "3"
tokio-test = "0.4"
```

- [ ] **Шаг 2: Проверить что Cargo.toml парсится**

```bash
cd rust && cargo metadata --no-deps --format-version 1 | head -5
```

Ожидаем: JSON с `"name":"memex"`. Если зависимость не найдена — проверить версию на crates.io.

- [ ] **Шаг 3: Коммит**

```bash
git add rust/Cargo.toml
git commit -m "chore(rust): add Cargo.toml with all dependencies"
```

---

### Task 1.2: error.rs

- [ ] **Шаг 1: Создать rust/src/error.rs**

```rust
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("r2d2 pool error: {0}")]
    Pool(#[from] r2d2::Error),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("llm error: {0}")]
    Llm(String),

    #[error("embedding error: {0}")]
    Embedding(String),

    #[error("document parsing error: {0}")]
    Parse(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, self.to_string()).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
```

- [ ] **Шаг 2: Написать тест**

```rust
// В конце rust/src/error.rs
#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    #[test]
    fn not_found_returns_404() {
        let err = AppError::NotFound("doc 123".to_string());
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn bad_request_returns_400() {
        let err = AppError::BadRequest("invalid input".to_string());
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
```

- [ ] **Шаг 3: Запустить тест (пока не компилируется — ожидаем ошибку)**

```bash
cd rust && cargo test error 2>&1 | head -20
```

---

### Task 1.3: config.rs

- [ ] **Шаг 1: Написать тест конфига**

```rust
// rust/src/config.rs (начать с теста)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_paths_are_sensible() {
        let cfg = Config {
            database_path: "data/memex.db".to_string(),
            tantivy_path: "data/tantivy".to_string(),
            upload_dir: "data/uploads".to_string(),
            local_embedding_model: "intfloat/multilingual-e5-small".to_string(),
            embedding_dimensions: 384,
            llm_provider: LlmProviderKind::Claude,
            llm_model: "claude-sonnet-4-6".to_string(),
            llm_max_tokens: 2048,
            llm_temperature: 0.1,
            anthropic_api_key: Some("sk-test".to_string()),
            openai_llm_api_key: None,
            l2_chunk_size: 512,
            l1_chunk_size: 128,
            l2_chunk_overlap: 64,
            semantic_top_k: 20,
            bm25_top_k: 20,
            rrf_k: 60,
            reranker_top_n: 5,
            host: "0.0.0.0".to_string(),
            port: 8000,
        };
        assert_eq!(cfg.embedding_dimensions, 384);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn validate_fails_when_claude_without_key() {
        let cfg = Config {
            llm_provider: LlmProviderKind::Claude,
            anthropic_api_key: None,
            ..Config::default_for_test()
        };
        assert!(cfg.validate().is_err());
    }
}
```

- [ ] **Шаг 2: Запустить тест — убедиться что FAIL (Config не определён)**

```bash
cd rust && cargo test config 2>&1 | tail -5
```

Ожидаем: `error[E0433]: failed to resolve: use of undeclared type 'Config'`

- [ ] **Шаг 3: Реализовать config.rs**

```rust
use std::str::FromStr;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmProviderKind {
    Claude,
    OpenAI,
}

impl FromStr for LlmProviderKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "claude" => Ok(LlmProviderKind::Claude),
            "openai" => Ok(LlmProviderKind::OpenAI),
            other => Err(format!("unknown llm provider: {other}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub database_path: String,
    pub tantivy_path: String,
    pub upload_dir: String,
    pub local_embedding_model: String,
    pub embedding_dimensions: usize,
    pub llm_provider: LlmProviderKind,
    pub llm_model: String,
    pub llm_max_tokens: u32,
    pub llm_temperature: f32,
    pub anthropic_api_key: Option<String>,
    pub openai_llm_api_key: Option<String>,
    pub l2_chunk_size: usize,
    pub l1_chunk_size: usize,
    pub l2_chunk_overlap: usize,
    pub semantic_top_k: usize,
    pub bm25_top_k: usize,
    pub rrf_k: usize,
    pub reranker_top_n: usize,
    pub host: String,
    pub port: u16,
}

impl Config {
    /// Загружает конфиг из переменных окружения (с .env файлом).
    pub fn from_env() -> Result<Self, String> {
        let _ = dotenvy::dotenv(); // игнорируем если .env нет

        let get = |key: &str| -> Result<String, String> {
            std::env::var(key).map_err(|_| format!("missing env var: {key}"))
        };

        let get_or = |key: &str, default: &str| -> String {
            std::env::var(key).unwrap_or_else(|_| default.to_string())
        };

        let get_parse = |key: &str, default: &str| -> Result<usize, String> {
            get_or(key, default)
                .parse::<usize>()
                .map_err(|e| format!("{key}: {e}"))
        };

        let llm_provider = get("LLM_PROVIDER")?.parse::<LlmProviderKind>()?;

        let cfg = Config {
            database_path: get_or("DATABASE_PATH", "data/memex.db"),
            tantivy_path: get_or("TANTIVY_PATH", "data/tantivy"),
            upload_dir: get_or("UPLOAD_DIR", "data/uploads"),
            local_embedding_model: get_or(
                "LOCAL_EMBEDDING_MODEL",
                "intfloat/multilingual-e5-small",
            ),
            embedding_dimensions: get_parse("EMBEDDING_DIMENSIONS", "384")?,
            llm_provider,
            llm_model: get("LLM_MODEL")?,
            llm_max_tokens: get_or("LLM_MAX_TOKENS", "2048")
                .parse()
                .map_err(|e| format!("LLM_MAX_TOKENS: {e}"))?,
            llm_temperature: get_or("LLM_TEMPERATURE", "0.1")
                .parse()
                .map_err(|e| format!("LLM_TEMPERATURE: {e}"))?,
            anthropic_api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            openai_llm_api_key: std::env::var("OPENAI_LLM_API_KEY").ok(),
            l2_chunk_size: get_parse("L2_CHUNK_SIZE", "512")?,
            l1_chunk_size: get_parse("L1_CHUNK_SIZE", "128")?,
            l2_chunk_overlap: get_parse("L2_CHUNK_OVERLAP", "64")?,
            semantic_top_k: get_parse("SEMANTIC_TOP_K", "20")?,
            bm25_top_k: get_parse("BM25_TOP_K", "20")?,
            rrf_k: get_parse("RRF_K", "60")?,
            reranker_top_n: get_parse("RERANKER_TOP_N", "5")?,
            host: get_or("HOST", "0.0.0.0"),
            port: get_or("PORT", "8000")
                .parse()
                .map_err(|e| format!("PORT: {e}"))?,
        };

        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<(), String> {
        match &self.llm_provider {
            LlmProviderKind::Claude if self.anthropic_api_key.is_none() => {
                Err("ANTHROPIC_API_KEY is required when LLM_PROVIDER=claude".to_string())
            }
            LlmProviderKind::OpenAI if self.openai_llm_api_key.is_none() => {
                Err("OPENAI_LLM_API_KEY is required when LLM_PROVIDER=openai".to_string())
            }
            _ => Ok(()),
        }
    }

    #[cfg(test)]
    pub fn default_for_test() -> Self {
        Config {
            database_path: ":memory:".to_string(),
            tantivy_path: "/tmp/tantivy_test".to_string(),
            upload_dir: "/tmp/uploads_test".to_string(),
            local_embedding_model: "intfloat/multilingual-e5-small".to_string(),
            embedding_dimensions: 384,
            llm_provider: LlmProviderKind::Claude,
            llm_model: "claude-haiku-4-5-20251001".to_string(),
            llm_max_tokens: 256,
            llm_temperature: 0.1,
            anthropic_api_key: Some("sk-test".to_string()),
            openai_llm_api_key: None,
            l2_chunk_size: 512,
            l1_chunk_size: 128,
            l2_chunk_overlap: 64,
            semantic_top_k: 20,
            bm25_top_k: 20,
            rrf_k: 60,
            reranker_top_n: 5,
            host: "127.0.0.1".to_string(),
            port: 8000,
        }
    }
}
```

- [ ] **Шаг 4: Запустить тесты конфига**

```bash
cd rust && cargo test config 2>&1
```

Ожидаем: `test config::tests::default_paths_are_sensible ... ok`  
Ожидаем: `test config::tests::validate_fails_when_claude_without_key ... ok`

---

### Task 1.4: main.rs — минимальный сервер

- [ ] **Шаг 1: Создать rust/src/main.rs**

```rust
mod config;
mod error;

// Заглушки — заполняются в следующих задачах
mod db;
mod search;
mod ingestion;
mod llm;
mod memory;
mod api;

use std::sync::Arc;
use axum::Router;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub struct AppState {
    // Заполняется в Task 2+
    // pub pool: db::Pool,
    // pub tantivy: Arc<search::TantivyStore>,
    // pub embed: Arc<ingestion::EmbeddingClient>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "memex=info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = config::Config::from_env()
        .map_err(|e| anyhow::anyhow!("config error: {e}"))?;

    info!("Memex starting on {}:{}", config.host, config.port);

    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    let app = Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }));

    info!("Listening on {addr}");
    axum::serve(listener, app).await?;

    Ok(())
}
```

- [ ] **Шаг 2: Создать заглушки модулей**

```bash
mkdir -p rust/src/{db,search,ingestion,llm,memory,api}
touch rust/src/db/mod.rs rust/src/search/mod.rs rust/src/ingestion/mod.rs
touch rust/src/llm/mod.rs rust/src/memory/mod.rs rust/src/api/mod.rs
```

- [ ] **Шаг 3: Убедиться что проект компилируется**

```bash
cd rust && cargo build 2>&1 | tail -10
```

Ожидаем: `Finished dev [unoptimized + debuginfo] target(s) in ...`

- [ ] **Шаг 4: Запустить все тесты**

```bash
cd rust && cargo test 2>&1
```

Ожидаем: минимум 2 теста проходят (config + error).

- [ ] **Шаг 5: Коммит**

```bash
git add rust/
git commit -m "feat(rust): project scaffold — config, error types, basic axum server"
```
