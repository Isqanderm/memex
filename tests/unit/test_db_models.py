from src.db.models import Chunk, Document, IngestionJob


def test_document_tablename():
    assert Document.__tablename__ == "documents"

def test_chunk_tablename():
    assert Chunk.__tablename__ == "chunks"

def test_ingestion_job_tablename():
    assert IngestionJob.__tablename__ == "ingestion_jobs"

def test_document_has_checksum_column():
    cols = {c.name for c in Document.__table__.columns}
    assert "checksum" in cols
    assert "source" in cols
    assert "mime_type" in cols

def test_chunk_has_required_columns():
    cols = {c.name for c in Chunk.__table__.columns}
    assert "content_vector" in cols
    assert "chunk_role" in cols
    assert "parent_chunk_id" in cols
    assert "language" in cols

def test_memory_model_importable():
    from src.db.models import Memory, MemoryExtractionJob
    assert Memory.__tablename__ == "memories"
    assert MemoryExtractionJob.__tablename__ == "memory_extraction_jobs"
