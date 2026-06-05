# Monorepo Dual CI/CD Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Настроить независимые CI-пайплайны и релизные процессы для Python (FastAPI + PostgreSQL) и Rust (Axum + SQLite) версий Memex в одном репозитории, чтобы изменения в одной версии не блокировали и не затрагивали другую.

**Architecture:** Два независимых CI-workflow с path-фильтрами запускаются только при изменении своей части кодовой базы. Два независимых release-workflow срабатывают на разных тег-паттернах (`v*.*.*` для Python, `rust/v*.*.*` для Rust). Golden-тесты в `tests/golden/` — общий контракт, проверяемый в Rust CI после сборки бинарника.

**Tech Stack:** GitHub Actions, Docker Buildx (multi-platform), `cross` (Rust ARM64 cross-compile), ghcr.io

---

## Файловая структура

| Файл | Действие | Ответственность |
|------|----------|-----------------|
| `.github/workflows/test.yml` | Modify | Добавить path-фильтры, переименовать job-ы в `python-*` |
| `.github/workflows/docker.yml` | Modify | Добавить path-фильтры, уточнить тег-паттерн |
| `.github/workflows/rust-ci.yml` | Create | Rust clippy + cargo test, path-фильтр `rust/**` |
| `.github/workflows/rust-release.yml` | Create | Сборка бинарников (amd64/arm64), Docker-образ с `rust-` префиксом |
| `rust/CHANGELOG.md` | Create | Отдельный changelog Rust-версии |
| `AGENTS.md` | Modify | Документировать two-product структуру |

---

## Task 1: Path-фильтры в Python CI (`test.yml`)

**Files:**
- Modify: `.github/workflows/test.yml`

Текущий `test.yml` запускается на любой push/PR к `main`. Нужно ограничить: Python CI должен игнорировать изменения только в `rust/`.

- [ ] **Step 1: Прочитать текущий файл**

```bash
cat .github/workflows/test.yml
```

- [ ] **Step 2: Добавить path-фильтры**

Заменить блок `on:` в `.github/workflows/test.yml`:

```yaml
on:
  push:
    branches: [main]
    paths:
      - 'src/**'
      - 'tests/**'
      - 'alembic/**'
      - 'pyproject.toml'
      - 'uv.lock'
  pull_request:
    branches: [main]
    paths:
      - 'src/**'
      - 'tests/**'
      - 'alembic/**'
      - 'pyproject.toml'
      - 'uv.lock'
```

- [ ] **Step 3: Проверить синтаксис**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/test.yml'))" && echo "OK"
```

Ожидаемый результат: `OK`

- [ ] **Step 4: Коммит**

```bash
git add .github/workflows/test.yml
git commit -m "ci(python): add path filters — skip on rust-only changes"
```

---

## Task 2: Path-фильтры в Docker-workflow (`docker.yml`)

**Files:**
- Modify: `.github/workflows/docker.yml`

Текущий `docker.yml` пересобирает Python Docker-образ при любом `push` на `main`, включая изменения только в `rust/`. Нужно добавить path-фильтры и уточнить тег-паттерн чтобы он не срабатывал на `rust/v*` теги.

- [ ] **Step 1: Прочитать текущий файл**

```bash
cat .github/workflows/docker.yml
```

- [ ] **Step 2: Обновить `on:` блок**

Заменить блок `on:` в `.github/workflows/docker.yml`:

```yaml
on:
  push:
    branches: [main]
    paths:
      - 'src/**'
      - 'alembic/**'
      - 'templates/**'
      - 'static/**'
      - 'pyproject.toml'
      - 'Dockerfile'
    tags:
      - 'v[0-9]*.[0-9]*.[0-9]*'   # только Python теги: v2.1.0, v2.2.0
```

Паттерн `v[0-9]*` исключает `rust/v*` теги (они начинаются с `rust/`, а не `v`).

- [ ] **Step 3: Проверить синтаксис**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/docker.yml'))" && echo "OK"
```

Ожидаемый результат: `OK`

- [ ] **Step 4: Коммит**

```bash
git add .github/workflows/docker.yml
git commit -m "ci(docker): add path filters and narrow tag pattern to Python releases"
```

---

## Task 3: Rust CI workflow

**Files:**
- Create: `.github/workflows/rust-ci.yml`

Rust CI запускается только при изменении файлов в `rust/**` или `tests/golden/**`. Запускает `cargo clippy`, `cargo test`, и golden-тесты против локально запущенного бинарника.

- [ ] **Step 1: Создать файл**

```yaml
# .github/workflows/rust-ci.yml
name: Rust CI

on:
  push:
    branches: [main]
    paths:
      - 'rust/**'
      - 'tests/golden/**'
      - 'tests/conftest.py'
  pull_request:
    branches: [main]
    paths:
      - 'rust/**'
      - 'tests/golden/**'
      - 'tests/conftest.py'

jobs:
  test:
    name: Rust tests
    runs-on: ubuntu-latest

    steps:
      - uses: actions/checkout@v4

      - name: Cache cargo registry
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            rust/target
          key: ${{ runner.os }}-cargo-${{ hashFiles('rust/Cargo.lock') }}
          restore-keys: ${{ runner.os }}-cargo-

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy

      - name: Clippy
        run: cargo clippy --manifest-path rust/Cargo.toml --all-targets -- -D warnings

      - name: Unit tests
        run: cargo test --manifest-path rust/Cargo.toml

  golden:
    name: Golden tests (contract)
    runs-on: ubuntu-latest
    needs: test

    steps:
      - uses: actions/checkout@v4

      - name: Cache cargo registry
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            rust/target
          key: ${{ runner.os }}-cargo-${{ hashFiles('rust/Cargo.lock') }}
          restore-keys: ${{ runner.os }}-cargo-

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Build Rust binary
        run: cargo build --release --manifest-path rust/Cargo.toml

      - name: Set up Python
        uses: astral-sh/setup-uv@v4
        with:
          enable-cache: true

      - name: Install Python deps
        run: uv sync --extra dev

      - name: Create Rust .env
        run: |
          cat > .env << 'EOF'
          DATABASE_PATH=data/memex.db
          TANTIVY_PATH=data/tantivy
          UPLOAD_DIR=data/uploads
          LOCAL_EMBEDDING_MODEL=multilingual-e5-small
          EMBEDDING_DIMENSIONS=384
          LLM_PROVIDER=openai
          LLM_MODEL=gpt-4o-mini
          LLM_MAX_TOKENS=2048
          LLM_TEMPERATURE=0.1
          OPENAI_LLM_API_KEY=dummy-key-for-contract-tests
          HOST=0.0.0.0
          PORT=8000
          EOF

      - name: Start Rust backend
        run: |
          set -a && source .env && set +a
          ./rust/target/release/memex &
          echo "RUST_PID=$!" >> $GITHUB_ENV
          for i in $(seq 1 30); do
            curl -sf http://localhost:8000/health && break
            sleep 2
          done

      - name: Run golden tests (unit, no LLM)
        run: |
          MEMEX_BASE_URL=http://localhost:8000 \
            uv run pytest tests/golden/ -m "unit and not e2e" \
            --tb=short -v \
            --junitxml=test-results/golden-rust-ci.xml

      - name: Upload test results
        uses: actions/upload-artifact@v4
        if: always()
        with:
          name: golden-test-results-rust
          path: test-results/golden-rust-ci.xml
```

- [ ] **Step 2: Проверить синтаксис**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/rust-ci.yml'))" && echo "OK"
```

Ожидаемый результат: `OK`

- [ ] **Step 3: Коммит**

```bash
git add .github/workflows/rust-ci.yml
git commit -m "ci(rust): add Rust CI with clippy, unit tests, and golden contract tests"
```

---

## Task 4: Rust release workflow

**Files:**
- Create: `.github/workflows/rust-release.yml`

Срабатывает на теги `rust/v*.*.*`. Собирает бинарники для linux/amd64 и linux/arm64 (cross-compile), публикует GitHub Release с артефактами и Docker-образ с префиксом `rust-`.

- [ ] **Step 1: Создать файл**

```yaml
# .github/workflows/rust-release.yml
name: Rust Release

on:
  push:
    tags:
      - 'rust/v[0-9]*.[0-9]*.[0-9]*'   # rust/v1.0.0, rust/v1.2.3

jobs:
  build:
    name: Build ${{ matrix.artifact }}
    runs-on: ubuntu-latest
    permissions:
      contents: write

    strategy:
      matrix:
        include:
          - target: x86_64-unknown-linux-gnu
            artifact: memex-linux-amd64
            cross: false
          - target: aarch64-unknown-linux-gnu
            artifact: memex-linux-arm64
            cross: true

    steps:
      - uses: actions/checkout@v4

      - name: Cache cargo registry
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            rust/target
          key: ${{ runner.os }}-cargo-${{ matrix.target }}-${{ hashFiles('rust/Cargo.lock') }}
          restore-keys: ${{ runner.os }}-cargo-${{ matrix.target }}-

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Install cross (for ARM64)
        if: matrix.cross
        run: cargo install cross --git https://github.com/cross-rs/cross

      - name: Build (native)
        if: "!matrix.cross"
        run: cargo build --release --manifest-path rust/Cargo.toml --target ${{ matrix.target }}

      - name: Build (cross-compile)
        if: matrix.cross
        run: cross build --release --manifest-path rust/Cargo.toml --target ${{ matrix.target }}

      - name: Rename binary
        run: |
          cp rust/target/${{ matrix.target }}/release/memex ${{ matrix.artifact }}
          chmod +x ${{ matrix.artifact }}

      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.artifact }}
          path: ${{ matrix.artifact }}

  release:
    name: GitHub Release
    runs-on: ubuntu-latest
    needs: build
    permissions:
      contents: write

    steps:
      - uses: actions/checkout@v4

      - name: Download all artifacts
        uses: actions/download-artifact@v4
        with:
          path: artifacts/

      - name: Extract version from tag
        run: |
          # rust/v1.2.3 → 1.2.3
          VERSION="${GITHUB_REF_NAME#rust/v}"
          echo "VERSION=$VERSION" >> $GITHUB_ENV

      - name: Read Rust CHANGELOG
        id: changelog
        run: |
          # Extract section for current version from rust/CHANGELOG.md
          python3 - << 'EOF'
          import re, os
          version = os.environ['VERSION']
          text = open('rust/CHANGELOG.md').read()
          pattern = rf'## \[{re.escape(version)}\].*?(?=\n## \[|\Z)'
          match = re.search(pattern, text, re.DOTALL)
          body = match.group(0).strip() if match else f'Rust release {version}'
          with open(os.environ['GITHUB_OUTPUT'], 'a') as f:
              f.write(f'body<<EOF\n{body}\nEOF\n')
          EOF

      - name: Create GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          name: "Rust ${{ env.VERSION }}"
          body: ${{ steps.changelog.outputs.body }}
          files: |
            artifacts/memex-linux-amd64/memex-linux-amd64
            artifacts/memex-linux-arm64/memex-linux-arm64
          tag_name: ${{ github.ref_name }}

  docker:
    name: Docker image (rust-)
    runs-on: ubuntu-latest
    needs: build
    permissions:
      contents: read
      packages: write

    steps:
      - uses: actions/checkout@v4

      - name: Extract version from tag
        run: |
          VERSION="${GITHUB_REF_NAME#rust/v}"
          echo "VERSION=$VERSION" >> $GITHUB_ENV

      - name: Log in to GHCR
        uses: docker/login-action@v3
        with:
          registry: ghcr.io
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - name: Set up Docker Buildx
        uses: docker/setup-buildx-action@v3

      - name: Build and push Docker image
        uses: docker/build-push-action@v6
        with:
          context: .
          file: rust/Dockerfile
          platforms: linux/amd64,linux/arm64
          push: true
          tags: |
            ghcr.io/${{ github.repository }}:rust-${{ env.VERSION }}
            ghcr.io/${{ github.repository }}:rust-latest
          cache-from: type=gha
          cache-to: type=gha,mode=max
```

- [ ] **Step 2: Проверить синтаксис**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/rust-release.yml'))" && echo "OK"
```

Ожидаемый результат: `OK`

- [ ] **Step 3: Коммит**

```bash
git add .github/workflows/rust-release.yml
git commit -m "ci(rust): add release workflow — binaries + Docker on rust/v* tags"
```

---

## Task 5: `rust/CHANGELOG.md`

**Files:**
- Create: `rust/CHANGELOG.md`

Отдельный changelog для Rust-версии. Начинается с текущей версии из `rust/Cargo.toml` (3.0.0).

- [ ] **Step 1: Прочитать текущую версию Rust**

```bash
grep '^version' rust/Cargo.toml | head -1
```

Ожидаемый результат: `version = "3.0.0"`

- [ ] **Step 2: Создать файл**

```markdown
# Rust Changelog

All notable changes to the Rust (SQLite) version of Memex.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/)
Versioning: independent from Python version (see root CHANGELOG.md)

---

## [3.0.0] — 2026-06-04

### Added

- Initial Rust implementation: Axum + SQLite + ONNX embeddings
- Full API parity with Python version for core endpoints:
  `/api/documents`, `/api/jobs`, `/api/memory/*`, `/api/search/chunks`, `/api/query`
- Web UI via minijinja (Jinja2-compatible templates)
- MCP-compatible REST API
- Local embedding model via fastembed (multilingual-e5-small, 384 dims)
- BM25 full-text search via tantivy
- Cross-encoder reranker via ONNX
- Single binary deployment — no PostgreSQL, no Python required
- `GET /health` endpoint

### Architecture

- Single binary, ~25 MB stripped
- SQLite for storage (vs PostgreSQL in Python version)
- Idle RAM: ~80-120 MB (vs ~2.5 GB Python+PostgreSQL)
- Cold start: ~2-3 seconds including model loading

### Known differences from Python version

- `GET /api/memory/context` returns `static_summary`/`dynamic_summary` (Python: `static`/`dynamic`)
- `POST /api/search/chunks` returns array directly (Python: `{"chunks": [...]}`)
- `DELETE /api/memory/:id` returns 204 No Content (Python: `{"status": "deleted"}`)
- No `PATCH /api/documents/:id` endpoint
- No `GET /api/documents/:id/file` endpoint
```

- [ ] **Step 3: Коммит**

```bash
git add rust/CHANGELOG.md
git commit -m "docs(rust): add separate CHANGELOG for Rust version"
```

---

## Task 6: Обновить `AGENTS.md` — two-product структура

**Files:**
- Modify: `AGENTS.md`

AGENTS.md сейчас описывает только Python-стек. Нужно добавить раздел про Rust-версию и правила того, что считается API-контрактом.

- [ ] **Step 1: Прочитать текущий AGENTS.md**

```bash
head -60 AGENTS.md
```

- [ ] **Step 2: Добавить раздел после первого `---` разделителя (после секции "Что такое Memex")**

Вставить после строки с первым `---`:

```markdown
## Two-Product Structure

Memex существует в двух вариантах с независимыми версиями:

| | Python (primary) | Rust (lightweight) |
|---|---|---|
| **DB** | PostgreSQL 15 + pgvector | SQLite + tantivy |
| **Версия** | `pyproject.toml` | `rust/Cargo.toml` |
| **Тег релиза** | `v2.1.0` | `rust/v3.0.0` |
| **Docker** | `ghcr.io/…/memex:2.1.0` | `ghcr.io/…/memex:rust-3.0.0` |
| **Changelog** | `CHANGELOG.md` | `rust/CHANGELOG.md` |
| **CI** | `.github/workflows/test.yml` | `.github/workflows/rust-ci.yml` |
| **RAM idle** | ~2.5 GB | ~80-120 MB |

### Правила поддержки двух версий

1. **Python — primary**: новые фичи разрабатываются в Python. Rust получает фичи отдельным циклом.
2. **API-контракт фиксирован golden-тестами**: `tests/golden/` — источник правды. Любое изменение контракта требует обновления golden-тестов.
3. **Известные намеренные расхождения** задокументированы в `tests/golden/test_memory.py` и `tests/golden/test_search.py` через хелперы `_assert_context_shape` и `_extract_chunks`.
4. **Rust CI запускает golden-тесты** после сборки бинарника — регрессии видны немедленно.
5. **Версии независимы**: Python можно релизить без Rust и наоборот.
```

- [ ] **Step 3: Проверить что файл корректный markdown**

```bash
python3 -c "
import re
text = open('AGENTS.md').read()
headers = re.findall(r'^#+\s+.+', text, re.MULTILINE)
print(f'Sections: {len(headers)}')
for h in headers[:10]:
    print(h)
"
```

Ожидаемый результат: 10+ секций без ошибок парсинга.

- [ ] **Step 4: Коммит**

```bash
git add AGENTS.md
git commit -m "docs: document two-product structure in AGENTS.md"
```

---

## Self-Review

**Покрытие требований:**

| Требование | Задача |
|---|---|
| Python CI не запускается на изменениях `rust/` | Task 1 |
| Docker Python не пересобирается на `rust/` изменениях | Task 2 |
| Docker Python не триггерится на `rust/v*` тегах | Task 2 |
| Rust CI: clippy + unit tests | Task 3 |
| Rust CI: golden-тесты после сборки | Task 3 |
| Rust Release: бинарники amd64 + arm64 | Task 4 |
| Rust Release: Docker с `rust-` префиксом | Task 4 |
| Rust Release: GitHub Release из `rust/CHANGELOG.md` | Task 4, 5 |
| Документация two-product структуры | Task 6 |

**Placeholder scan:** нет TBD, все YAML-блоки полные.

**Важное замечание для исполнителя:** В `rust-ci.yml` golden-тесты запускаются с `OPENAI_LLM_API_KEY=dummy-key-for-contract-tests` — это намеренно. Golden-тесты с маркером `unit and not e2e` не вызывают LLM, поэтому dummy-ключ достаточен для прохождения тестов.
