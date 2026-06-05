# Rust Pre-Release Bugfixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Исправить все баги и критические упущения выявленные в code review перед релизом v3.0.0.

**Architecture:** Точечные правки в существующих файлах — без рефакторинга, только конкретные баги. Каждый таск независим и коммитится отдельно.

**Tech Stack:** Rust, axum, reqwest, rusqlite, tokio

---

## Файловая структура

| Файл | Что меняем |
|------|------------|
| `rust/src/search/vectors.rs:182` | L2↔cosine math fix |
| `rust/src/search/vectors.rs:38` | assert_eq! → proper error |
| `rust/src/search/context.rs:98` | filename stripping fix |
| `rust/src/api/documents.rs` | DefaultBodyLimit на upload |
| `rust/src/llm/openai.rs:19` | reqwest timeout |
| `rust/src/llm/claude.rs:19` | reqwest timeout |
| `rust/src/db/pool.rs:31` | busy_timeout PRAGMA |
| `rust/src/memory/worker.rs` | MemoryExpiryWorker — уже имеет shutdown, нет проблемы |
| `.github/workflows/rust-release.yml` | добавить memex-mcp в artifacts |
| `docs/rust.md` | poppler-utils для binary install |

---

## Task 1: Исправить математику L2→cosine в `find_similar_memories`

**Files:**
- Modify: `rust/src/search/vectors.rs`

**Проблема:** `dist_threshold = 1.0 - similarity_threshold` неверно для L2-дистанции.
Для unit-векторов: `L2 = sqrt(2*(1-cos))`. При `similarity=0.60` код даёт `dist=0.40`, правильно `dist=0.894` — фильтрация в 8× строже нужного.

- [ ] **Step 1: Найти строку**

```bash
grep -n "dist_threshold" rust/src/search/vectors.rs
```

Ожидаемый результат: строка `let dist_threshold = 1.0 - similarity_threshold;`

- [ ] **Step 2: Исправить формулу**

В `rust/src/search/vectors.rs`, функция `find_similar_memories`, заменить:

```rust
let dist_threshold = 1.0 - similarity_threshold;
```

на:

```rust
// For unit-normalized vectors: L2 = sqrt(2*(1-cosine))
let dist_threshold = (2.0_f32 * (1.0 - similarity_threshold)).sqrt();
```

- [ ] **Step 3: Добавить unit-тест**

В секцию `#[cfg(test)]` в том же файле добавить:

```rust
#[test]
fn cosine_to_l2_threshold_correct() {
    // cosine=1.0 (identical) → L2=0.0
    let t = (2.0_f32 * (1.0 - 1.0_f32)).sqrt();
    assert!((t - 0.0).abs() < 1e-6);

    // cosine=0.0 (orthogonal) → L2=sqrt(2)≈1.414
    let t = (2.0_f32 * (1.0 - 0.0_f32)).sqrt();
    assert!((t - 1.4142135).abs() < 1e-4);

    // cosine=0.60 → L2≈0.894, NOT 0.40
    let t = (2.0_f32 * (1.0 - 0.6_f32)).sqrt();
    assert!(t > 0.88 && t < 0.91, "expected ~0.894, got {t}");
}
```

- [ ] **Step 4: Запустить тест**

```bash
cargo test --manifest-path rust/Cargo.toml cosine_to_l2_threshold_correct 2>&1 | tail -5
```

Ожидаемый результат: `test ... ok`

- [ ] **Step 5: Коммит**

```bash
git add rust/src/search/vectors.rs
git commit -m "fix(rust): correct L2-distance threshold formula for cosine similarity"
```

---

## Task 2: assert_eq! → proper error в `insert_chunk`

**Files:**
- Modify: `rust/src/search/vectors.rs`

**Проблема:** `assert_eq!(embedding.len(), self.dimensions, ...)` паникует вместо возврата ошибки при смене модели.

- [ ] **Step 1: Найти строку**

```bash
grep -n "assert_eq!" rust/src/search/vectors.rs
```

Ожидаемый результат: `assert_eq!(embedding.len(), self.dimensions, ...)`

- [ ] **Step 2: Заменить assert на error**

Найти в `insert_chunk` блок:

```rust
assert_eq!(
    embedding.len(),
    self.dimensions,
    "embedding length mismatch: expected {}, got {}",
    self.dimensions,
    embedding.len()
);
```

Заменить на:

```rust
if embedding.len() != self.dimensions {
    return Err(rusqlite::Error::InvalidParameterName(format!(
        "embedding length mismatch: expected {}, got {}",
        self.dimensions,
        embedding.len()
    )));
}
```

Сделать то же самое для `insert_memory` если там тоже есть `assert_eq!`:

```bash
grep -n "assert_eq!" rust/src/search/vectors.rs
```

- [ ] **Step 3: Проверить компиляцию**

```bash
cargo build --manifest-path rust/Cargo.toml 2>&1 | grep "^error" | head -5
```

Ожидаемый результат: нет ошибок.

- [ ] **Step 4: Коммит**

```bash
git add rust/src/search/vectors.rs
git commit -m "fix(rust): replace panic assert with proper error in insert_chunk/insert_memory"
```

---

## Task 3: Исправить обрезание имён файлов в `context.rs`

**Files:**
- Modify: `rust/src/search/context.rs`

**Проблема:** `splitn(6, '-')` режет имена с 5+ дефисами: `"my-awesome-research-paper.pdf"` → `"paper.pdf"`.
Правильный подход: убрать 17-символьный checksum-префикс как в `get_document_file`.

- [ ] **Step 1: Найти строку**

```bash
grep -n "splitn" rust/src/search/context.rs
```

Ожидаемый результат: `let parts: Vec<&str> = raw_name.splitn(6, '-').collect();`

- [ ] **Step 2: Заменить логику**

Найти блок:

```rust
.map(|raw_name| {
    let parts: Vec<&str> = raw_name.splitn(6, '-').collect();
    parts.last().copied().unwrap_or(raw_name).to_string()
});
```

Заменить на:

```rust
.map(|raw_name| {
    // Files are stored as "{16-char checksum}-{original_name}"
    // Strip the checksum prefix if present
    if raw_name.len() > 17 && raw_name.chars().nth(16) == Some('-') {
        raw_name[17..].to_string()
    } else {
        raw_name.to_string()
    }
});
```

- [ ] **Step 3: Проверить компиляцию**

```bash
cargo build --manifest-path rust/Cargo.toml 2>&1 | grep "^error" | head -5
```

Ожидаемый результат: нет ошибок.

- [ ] **Step 4: Коммит**

```bash
git add rust/src/search/context.rs
git commit -m "fix(rust): strip checksum prefix correctly in context source filenames"
```

---

## Task 4: DefaultBodyLimit на upload endpoint

**Files:**
- Modify: `rust/src/api/documents.rs`
- Modify: `rust/src/main.rs` (если limit вешается на router уровне)

**Проблема:** multipart body читается без ограничений — 4 GB файл → OOM → падение сервера.

- [ ] **Step 1: Найти где подключается router documents**

```bash
grep -n "documents\|router\|DefaultBodyLimit" rust/src/main.rs
grep -n "DefaultBodyLimit" rust/src/api/documents.rs
```

- [ ] **Step 2: Добавить limit в router документов**

В `rust/src/api/documents.rs`, в функции `router()`:

```rust
use axum::extract::DefaultBodyLimit;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/documents", post(upload_document))
        .route("/api/documents", get(list_documents))
        .route("/api/documents/:id", delete(delete_document))
        .route("/api/documents/:id", patch(update_document))
        .route("/api/documents/:id/file", get(get_document_file))
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024)) // 100 MB upload limit
}
```

Убедиться что `DefaultBodyLimit` импортирован: в axum 0.7 он в `axum::extract::DefaultBodyLimit`.

- [ ] **Step 3: Проверить компиляцию**

```bash
cargo build --manifest-path rust/Cargo.toml 2>&1 | grep "^error" | head -5
```

Ожидаемый результат: нет ошибок.

- [ ] **Step 4: Коммит**

```bash
git add rust/src/api/documents.rs
git commit -m "fix(rust): add 100MB body limit on document upload endpoint"
```

---

## Task 5: Таймаут на reqwest LLM-клиентах

**Files:**
- Modify: `rust/src/llm/openai.rs`
- Modify: `rust/src/llm/claude.rs`

**Проблема:** зависший OpenAI/Claude → блокирует все `spawn_blocking` потоки → API встаёт.

- [ ] **Step 1: Обновить `OpenAiProvider::new`**

В `rust/src/llm/openai.rs`:

Текущий код:
```rust
client: reqwest::Client::new(),
```

Заменить на:
```rust
client: reqwest::Client::builder()
    .timeout(std::time::Duration::from_secs(120))
    .build()
    .expect("failed to build reqwest client"),
```

- [ ] **Step 2: Обновить `ClaudeProvider::new`**

В `rust/src/llm/claude.rs`:

```rust
client: reqwest::Client::builder()
    .timeout(std::time::Duration::from_secs(120))
    .build()
    .expect("failed to build reqwest client"),
```

- [ ] **Step 3: Проверить компиляцию**

```bash
cargo build --manifest-path rust/Cargo.toml 2>&1 | grep "^error" | head -5
```

Ожидаемый результат: нет ошибок.

- [ ] **Step 4: Коммит**

```bash
git add rust/src/llm/openai.rs rust/src/llm/claude.rs
git commit -m "fix(rust): add 120s timeout to LLM reqwest clients"
```

---

## Task 6: `busy_timeout` в SQLite connection pool

**Files:**
- Modify: `rust/src/db/pool.rs`

**Проблема:** конкурентная запись в WAL-режиме без `busy_timeout` → немедленный `SQLITE_BUSY` → 500 клиенту.

- [ ] **Step 1: Найти `init_connection`**

```bash
grep -n "PRAGMA\|init_connection" rust/src/db/pool.rs | head -10
```

- [ ] **Step 2: Добавить `busy_timeout` PRAGMA**

В `rust/src/db/pool.rs`, функция `init_connection`, изменить:

```rust
fn init_connection(conn: &mut Connection) -> rusqlite::Result<()> {
    // WAL + foreign keys
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;",
    )?;
```

на:

```rust
fn init_connection(conn: &mut Connection) -> rusqlite::Result<()> {
    // WAL + foreign keys + busy timeout (5s before returning SQLITE_BUSY)
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         PRAGMA synchronous=NORMAL;
         PRAGMA busy_timeout=5000;",
    )?;
```

- [ ] **Step 3: Проверить компиляцию и unit-тест**

```bash
cargo test --manifest-path rust/Cargo.toml pool_opens_and_schema_created 2>&1 | tail -5
```

Ожидаемый результат: `test ... ok`

- [ ] **Step 4: Коммит**

```bash
git add rust/src/db/pool.rs
git commit -m "fix(rust): add busy_timeout=5000ms to SQLite connection pool"
```

---

## Task 7: Добавить `memex-mcp` в release pipeline

**Files:**
- Modify: `.github/workflows/rust-release.yml`

**Проблема:** README обещает `memex-mcp` бинарник, но `rust-release.yml` собирает только `memex`.

- [ ] **Step 1: Прочитать текущий Build step**

```bash
grep -A8 "name: Build$" .github/workflows/rust-release.yml
```

- [ ] **Step 2: Обновить Build step для сборки обоих бинарников**

Найти:

```yaml
      - name: Build
        run: cargo build --release --manifest-path rust/Cargo.toml

      - name: Rename binary
        run: |
          cp rust/target/release/memex ${{ matrix.artifact }}
          chmod +x ${{ matrix.artifact }}
```

Заменить на:

```yaml
      - name: Build
        run: cargo build --release --manifest-path rust/Cargo.toml --bins

      - name: Rename binaries
        run: |
          cp rust/target/release/memex ${{ matrix.artifact }}
          chmod +x ${{ matrix.artifact }}
          MCP_ARTIFACT=$(echo "${{ matrix.artifact }}" | sed 's/memex-/memex-mcp-/')
          cp rust/target/release/memex-mcp $MCP_ARTIFACT
          chmod +x $MCP_ARTIFACT
          echo "MCP_ARTIFACT=$MCP_ARTIFACT" >> $GITHUB_ENV
```

- [ ] **Step 3: Обновить Upload artifact для загрузки обоих**

Найти:

```yaml
      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.artifact }}
          path: ${{ matrix.artifact }}
```

Заменить на:

```yaml
      - name: Upload artifacts
        uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.artifact }}
          path: |
            ${{ matrix.artifact }}
            ${{ env.MCP_ARTIFACT }}
```

- [ ] **Step 4: Обновить GitHub Release для включения memex-mcp**

Найти блок `files:` в release job:

```yaml
          files: |
            artifacts/memex-linux-amd64/memex-linux-amd64
            artifacts/memex-linux-arm64/memex-linux-arm64
```

Заменить на:

```yaml
          files: |
            artifacts/memex-linux-amd64/memex-linux-amd64
            artifacts/memex-linux-amd64/memex-mcp-linux-amd64
            artifacts/memex-linux-arm64/memex-linux-arm64
            artifacts/memex-linux-arm64/memex-mcp-linux-arm64
```

- [ ] **Step 5: Проверить синтаксис**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/rust-release.yml'))" && echo "OK"
```

Ожидаемый результат: `OK`

- [ ] **Step 6: Коммит**

```bash
git add .github/workflows/rust-release.yml
git commit -m "ci(rust): add memex-mcp binary to release artifacts"
```

---

## Task 8: Документация — `poppler-utils` для binary install

**Files:**
- Modify: `docs/rust.md`

**Проблема:** PDF-загрузки молча падают на Raspberry Pi при установке через бинарник, потому что `poppler-utils` не установлен и не упомянут в доке.

- [ ] **Step 1: Найти раздел Option B в docs/rust.md**

```bash
grep -n "Option B\|poppler\|binary" docs/rust.md | head -10
```

- [ ] **Step 2: Добавить примечание про зависимости**

После блока с командами скачивания в "Option B: Pre-built binary" добавить:

```markdown
> **PDF support** requires `poppler-utils` on the host system:
> ```bash
> # Debian / Ubuntu / Raspberry Pi OS
> sudo apt-get install -y poppler-utils
> ```
> Without it, PDF uploads will fail silently. DOCX, MD, TXT, PPTX, XLSX, EPUB work without any dependencies.
```

- [ ] **Step 3: Проверить что секция выглядит правильно**

```bash
grep -A 10 "poppler" docs/rust.md
```

- [ ] **Step 4: Коммит**

```bash
git add docs/rust.md
git commit -m "docs(rust): add poppler-utils requirement for PDF support in binary install"
```

---

## Self-Review

**Покрытие ревью:**

| Баг/упущение | Задача | Статус |
|---|---|---|
| L2/cosine math — vectors.rs:182 | Task 1 | ✅ |
| assert_eq! паника — vectors.rs:38 | Task 2 | ✅ |
| Обрезание имён — context.rs:98 | Task 3 | ✅ |
| DefaultBodyLimit upload | Task 4 | ✅ |
| LLM timeout | Task 5 | ✅ |
| SQLite busy_timeout | Task 6 | ✅ |
| memex-mcp в release | Task 7 | ✅ |
| poppler-utils в доке | Task 8 | ✅ |
| MemoryExpiryWorker shutdown | — | Уже реализован (memory/worker.rs имеет `shutdown: watch::Receiver<bool>`) |
| Нет аутентификации | — | Вне скоупа (single-user by design) |
| Rate limiting | — | Вне скоупа (отложить) |
| Система миграций | — | Большой фича (отложить на 3.1) |

**Placeholder scan:** нет TBD, весь код конкретный.
