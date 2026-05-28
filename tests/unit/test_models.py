from src.models.parsed import ParsedDocument, Section
from src.models.chunk import ChunkData

def test_parsed_document_sections():
    doc = ParsedDocument(
        source="test.md",
        mime_type="text/markdown",
        sections=[Section(content="Hello", heading="Intro", level=1)],
    )
    assert len(doc.sections) == 1
    assert doc.sections[0].heading == "Intro"

def test_section_defaults():
    s = Section(content="text")
    assert s.heading is None
    assert s.level == 0
    assert s.page_number is None

def test_chunk_data_defaults():
    chunk = ChunkData(content="text", chunk_role="leaf", chunk_index=0)
    assert chunk.language == "simple"
    assert chunk.embedding is None
    assert chunk.parent_temp_index is None

def test_chunk_data_parent():
    chunk = ChunkData(content="text", chunk_role="parent", chunk_index=0)
    assert chunk.chunk_role == "parent"
