import uuid
from src.retrieval.context import ContextBuilder
from src.retrieval.expand import L2Chunk


def make_chunk(content: str, title: str = "Doc", heading: str | None = None) -> L2Chunk:
    return L2Chunk(
        chunk_id=uuid.uuid4(),
        content=content,
        doc_id=uuid.uuid4(),
        section_heading=heading,
        page_number=1,
        doc_title=title,
    )


def test_prompt_contains_query():
    builder = ContextBuilder()
    ctx = builder.build("how to index JSONB?", [make_chunk("GIN index content")])
    assert "how to index JSONB?" in ctx.prompt


def test_prompt_contains_content():
    builder = ContextBuilder()
    ctx = builder.build("query", [make_chunk("GIN index content")])
    assert "GIN index content" in ctx.prompt


def test_sources_metadata():
    builder = ContextBuilder()
    chunk = make_chunk("Content here", title="PG Guide", heading="Indexes")
    ctx = builder.build("query", [chunk])
    assert len(ctx.sources) == 1
    assert ctx.sources[0]["index"] == 1
    assert ctx.sources[0]["title"] == "PG Guide"
    assert ctx.sources[0]["section"] == "Indexes"


def test_multiple_sources_numbered():
    builder = ContextBuilder()
    chunks = [make_chunk(f"Content {i}", title=f"Doc {i}") for i in range(3)]
    ctx = builder.build("query", chunks)
    assert len(ctx.sources) == 3
    assert "[1]" in ctx.prompt
    assert "[2]" in ctx.prompt
    assert "[3]" in ctx.prompt


def test_empty_chunks():
    builder = ContextBuilder()
    ctx = builder.build("query", [])
    assert "query" in ctx.prompt
    assert ctx.sources == []
