import uuid

import pytest
from sqlalchemy import text

from src.ingestion.indexing import IndexingStage
from src.models.chunk import ChunkData
from src.models.parsed import ParsedDocument, Section


@pytest.mark.integration
async def test_indexing_creates_document_and_chunks(db_session):
    parsed = ParsedDocument(
        source="test.txt",
        mime_type="text/plain",
        sections=[Section(content="Hello world. This is a test.")],
    )
    checksum = f"cs-{uuid.uuid4()}"
    chunks = [
        ChunkData(content="Hello world. This is a test.", chunk_role="parent", chunk_index=0),
        ChunkData(content="Hello world.", chunk_role="leaf", chunk_index=0,
                  parent_temp_index=0, embedding=[0.1] * 1536, language="en"),
        ChunkData(content="This is a test.", chunk_role="leaf", chunk_index=1,
                  parent_temp_index=0, embedding=[0.2] * 1536, language="en"),
    ]

    stage = IndexingStage()
    doc_id = await stage.index(db_session, parsed, chunks, checksum=checksum)
    assert doc_id is not None

    result = await db_session.execute(
        text("SELECT count(*) FROM chunks WHERE doc_id = :id"), {"id": doc_id}
    )
    assert result.scalar() == 3  # 1 parent + 2 leaves


@pytest.mark.integration
async def test_leaves_have_vectors(db_session):
    parsed = ParsedDocument(source="v.txt", mime_type="text/plain",
                            sections=[Section(content="Vector test")])
    checksum = f"cs-v-{uuid.uuid4()}"
    chunks = [
        ChunkData(content="Vector test", chunk_role="parent", chunk_index=0),
        ChunkData(content="Vector test", chunk_role="leaf", chunk_index=0,
                  parent_temp_index=0, embedding=[0.5] * 1536, language="en"),
    ]
    stage = IndexingStage()
    doc_id = await stage.index(db_session, parsed, chunks, checksum=checksum)

    result = await db_session.execute(
        text("SELECT content_vector IS NOT NULL FROM chunks WHERE doc_id = :id AND chunk_role = 'leaf'"),
        {"id": doc_id}
    )
    assert result.scalar() is True


@pytest.mark.integration
async def test_parents_have_no_vector(db_session):
    parsed = ParsedDocument(source="p.txt", mime_type="text/plain",
                            sections=[Section(content="Parent test")])
    checksum = f"cs-p-{uuid.uuid4()}"
    chunks = [
        ChunkData(content="Parent test", chunk_role="parent", chunk_index=0),
        ChunkData(content="Parent test", chunk_role="leaf", chunk_index=0,
                  parent_temp_index=0, embedding=[0.3] * 1536, language="en"),
    ]
    stage = IndexingStage()
    doc_id = await stage.index(db_session, parsed, chunks, checksum=checksum)

    result = await db_session.execute(
        text("SELECT content_vector IS NULL FROM chunks WHERE doc_id = :id AND chunk_role = 'parent'"),
        {"id": doc_id}
    )
    assert result.scalar() is True
