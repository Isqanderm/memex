import pytest
from src.ingestion.chunker import SmallToBigChunker
from src.models.parsed import ParsedDocument, Section
from src.models.chunk import ChunkData


def make_doc(content: str, heading: str | None = None) -> ParsedDocument:
    return ParsedDocument(
        source="test.txt",
        mime_type="text/plain",
        sections=[Section(content=content, heading=heading)],
    )


def test_produces_both_levels():
    chunker = SmallToBigChunker(l2_size=10, l1_size=3, l2_overlap=2)
    doc = make_doc("word " * 50)
    chunks = chunker.chunk(doc)
    roles = {c.chunk_role for c in chunks}
    assert "parent" in roles
    assert "leaf" in roles


def test_leaves_reference_parents():
    chunker = SmallToBigChunker(l2_size=10, l1_size=3, l2_overlap=2)
    doc = make_doc("word " * 50)
    chunks = chunker.chunk(doc)
    leaves = [c for c in chunks if c.chunk_role == "leaf"]
    parents = [c for c in chunks if c.chunk_role == "parent"]
    assert len(leaves) > 0
    assert all(c.parent_temp_index is not None for c in leaves)
    assert all(0 <= c.parent_temp_index < len(parents) for c in leaves)


def test_short_doc_has_at_least_two_chunks():
    chunker = SmallToBigChunker(l2_size=512, l1_size=128, l2_overlap=64)
    doc = make_doc("Short text here.")
    chunks = chunker.chunk(doc)
    assert len(chunks) >= 2  # минимум 1 parent + 1 leaf


def test_parent_chunk_index_sequential():
    chunker = SmallToBigChunker(l2_size=10, l1_size=3, l2_overlap=2)
    doc = make_doc("word " * 50)
    chunks = chunker.chunk(doc)
    parents = sorted([c for c in chunks if c.chunk_role == "parent"], key=lambda c: c.chunk_index)
    assert [c.chunk_index for c in parents] == list(range(len(parents)))


def test_section_heading_preserved():
    chunker = SmallToBigChunker(l2_size=512, l1_size=128, l2_overlap=64)
    doc = make_doc("Some content here.", heading="My Section")
    chunks = chunker.chunk(doc)
    for c in chunks:
        assert c.section_heading == "My Section"


def test_empty_section_skipped():
    chunker = SmallToBigChunker(l2_size=512, l1_size=128, l2_overlap=64)
    doc = ParsedDocument(source="x", mime_type="text/plain", sections=[
        Section(content=""),
        Section(content="Real content here."),
    ])
    chunks = chunker.chunk(doc)
    assert len(chunks) >= 2
    assert all(c.content.strip() for c in chunks)


def test_multi_section_doc():
    chunker = SmallToBigChunker(l2_size=512, l1_size=128, l2_overlap=64)
    doc = ParsedDocument(source="x", mime_type="text/plain", sections=[
        Section(content="First section content.", heading="Intro"),
        Section(content="Second section content.", heading="Body"),
    ])
    chunks = chunker.chunk(doc)
    parents = [c for c in chunks if c.chunk_role == "parent"]
    assert len(parents) >= 2
