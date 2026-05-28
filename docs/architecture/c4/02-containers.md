# C4 Model — Уровень 2: Container Diagram

Контейнеры системы, их ответственности и способы взаимодействия.

```mermaid
graph TB
    subgraph Clients["Клиенты"]
        User["👤 Пользователь\nHTTP / MCP"]
        MCPClient["🔧 Claude Code\nMCP Client"]
    end

    subgraph Memex["Memex — Personal RAG System"]
        API["REST API\n──────────\nFastAPI\nRouting, validation\nPOST /documents\nPOST /query\nGET /documents"]

        MCP["MCP Server\n──────────\nstdio transport\nadd_document\nquery"]

        subgraph Ingestion["Ingestion Pipeline"]
            AdapterRegistry["AdapterRegistry\n──────────\nPdfAdapter\nMarkdownAdapter\nTextAdapter\nDocxAdapter"]
            Chunker["SmallToBigChunker\n──────────\nL2: ~512 tok\nL1: ~128 tok\nLanguageDetector"]
            EmbedStage["EmbeddingStage\n──────────\nOpenAI batch embed\nL1 chunks only"]
            IndexStage["IndexingStage\n──────────\nINSERT chunks\nvector + tsvector"]
        end

        subgraph Retrieval["Retrieval Pipeline"]
            QueryProc["QueryProcessor\n──────────\nnormalize\nlang detect"]
            SemSearch["SemanticSearch\n──────────\npgvector\ncosine similarity\ntop-20 L1"]
            BM25Search["BM25Search\n──────────\nPostgreSQL tsvector\nts_rank\ntop-20 L1"]
            RRF["RRF Merger\n──────────\nk=60\ndeduplicate\ntop-20 L1"]
            Expand["Expand to L2\n──────────\nparent_chunk_id\n→ top 5-10 L2"]
            Reranker["Reranker\n──────────\ncross-encoder\nms-marco-MiniLM\ntop 3-5 L2"]
            CtxBuilder["ContextBuilder\n──────────\nprompt assembly\nsource citations"]
        end

        DB[("PostgreSQL 15\n+ pgvector\n──────────\ndocuments\nchunks\nvector index\nGIN tsvector index")]
        FS["File Storage\n──────────\nlocal filesystem\n/data/files/\nоригиналы документов"]
    end

    subgraph External["Внешние API"]
        OpenAI["OpenAI\nEmbeddings API\ntext-embedding-3-small"]
        LLM["LLM API\nClaude / GPT-4o"]
    end

    User -->|HTTPS| API
    MCPClient -->|stdio MCP| MCP
    MCP -->|вызов| API

    API -->|файл + метаданные| AdapterRegistry
    AdapterRegistry -->|ParsedDocument| Chunker
    Chunker -->|L1+L2 chunks| EmbedStage
    EmbedStage -->|batch texts| OpenAI
    OpenAI -->|vectors| EmbedStage
    EmbedStage -->|chunks + vectors| IndexStage
    IndexStage -->|INSERT| DB
    AdapterRegistry -->|сохранить оригинал| FS

    API -->|query string| QueryProc
    QueryProc -->|normalized query| SemSearch
    QueryProc -->|normalized query| BM25Search
    SemSearch -->|embed query| OpenAI
    SemSearch -->|vector search| DB
    BM25Search -->|tsvector search| DB
    SemSearch -->|top-20 L1| RRF
    BM25Search -->|top-20 L1| RRF
    RRF -->|top-20 L1| Expand
    Expand -->|JOIN parent| DB
    Expand -->|5-10 L2| Reranker
    Reranker -->|3-5 L2| CtxBuilder
    CtxBuilder -->|prompt| LLM
    LLM -->|answer| API
```

## Контейнеры

| Контейнер | Технология | Ответственность |
|-----------|-----------|----------------|
| REST API | FastAPI (Python) | Точка входа, routing, валидация запросов |
| MCP Server | Python MCP SDK | Обёртка над API для Claude Code / AI-клиентов |
| AdapterRegistry | Python Protocol | Выбор и запуск адаптера по типу источника |
| SmallToBigChunker | Python | Нарезка документа на L1/L2 чанки, language detection |
| EmbeddingStage | Python + OpenAI SDK | Батчевое получение векторов для L1 чанков |
| IndexingStage | Python + psycopg | INSERT документов и чанков в PostgreSQL |
| QueryProcessor | Python | Нормализация запроса, определение языка |
| SemanticSearch | Python + pgvector | Поиск по cosine similarity в векторном индексе |
| BM25Search | Python + psycopg | Полнотекстовый поиск через tsvector |
| RRF Merger | Python | Слияние двух ranked lists по формуле 1/(rank+60) |
| Expand to L2 | Python + psycopg | JOIN L1 → parent L2 чанков |
| Reranker | sentence-transformers | cross-encoder score(query, chunk) на CPU |
| ContextBuilder | Python | Сборка prompt со ссылками на источники |
| PostgreSQL + pgvector | PostgreSQL 15 | Хранение documents, chunks, векторный и tsvector индексы |
| File Storage | Local filesystem | Оригинальные файлы документов |

## Схема данных (ключевые таблицы)

```
documents
  id, source, mime_type, title, metadata JSONB,
  checksum, indexed_at

chunks
  id, doc_id → documents.id
  parent_chunk_id → chunks.id   (NULL для L2)
  chunk_role: 'parent'|'leaf'
  chunk_index, section_heading, section_level, page_number
  prev_chunk_id, next_chunk_id → chunks.id
  language
  content TEXT
  content_vector VECTOR(1536)   (только L1)
  tsv TSVECTOR                  (L1 и L2)
```

## Ключевые архитектурные решения

- ADR-0001: Adapter Layer — Protocol-based Registry
- ADR-0002: Chunking — Small-to-Big (L1=128 tok, L2=512 tok)
- ADR-0003: Retrieval — Hybrid Search (Semantic + BM25 + RRF)
- ADR-0004: Embedding — OpenAI text-embedding-3-small
- ADR-0005: Reranker — cross-encoder/ms-marco локально
- ADR-0006: Мультиязычность — Language Detection per Chunk
