import pytest
from sqlalchemy import text


@pytest.mark.integration
async def test_pgvector_extension(db_session):
    result = await db_session.execute(
        text("SELECT extname FROM pg_extension WHERE extname = 'vector'")
    )
    assert result.scalar() == "vector"


@pytest.mark.integration
async def test_tables_exist(db_session):
    result = await db_session.execute(
        text("SELECT tablename FROM pg_tables WHERE schemaname = 'public'")
    )
    tables = {row[0] for row in result}
    assert {"documents", "chunks", "ingestion_jobs"} <= tables


@pytest.mark.integration
async def test_chunks_has_vector_column(db_session):
    result = await db_session.execute(text("""
        SELECT data_type FROM information_schema.columns
        WHERE table_name = 'chunks' AND column_name = 'content_vector'
    """))
    row = result.fetchone()
    assert row is not None
