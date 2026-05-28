import pytest
from src.ingestion.embedding import EmbeddingStage
from src.models.chunk import ChunkData
from tests.mocks.mock_embedding import MockEmbeddingClient


@pytest.mark.asyncio
async def test_embeds_only_leaf_chunks():
    client = MockEmbeddingClient(dimensions=4)
    stage = EmbeddingStage(client=client)

    chunks = [
        ChunkData(content="parent text", chunk_role="parent", chunk_index=0),
        ChunkData(content="leaf text 1", chunk_role="leaf", chunk_index=0, parent_temp_index=0),
        ChunkData(content="leaf text 2", chunk_role="leaf", chunk_index=1, parent_temp_index=0),
    ]

    result = await stage.process(chunks)

    parents = [c for c in result if c.chunk_role == "parent"]
    leaves = [c for c in result if c.chunk_role == "leaf"]

    assert all(p.embedding is None for p in parents)
    assert all(l.embedding is not None for l in leaves)
    assert all(len(l.embedding) == 4 for l in leaves)


@pytest.mark.asyncio
async def test_batches_requests():
    client = MockEmbeddingClient()
    stage = EmbeddingStage(client=client, batch_size=2)
    chunks = [
        ChunkData(content=f"leaf {i}", chunk_role="leaf", chunk_index=i, parent_temp_index=0)
        for i in range(5)
    ]
    await stage.process(chunks)
    assert len(client.calls) == 3  # ceil(5/2) = 3


@pytest.mark.asyncio
async def test_no_leaves_no_calls():
    client = MockEmbeddingClient()
    stage = EmbeddingStage(client=client)
    chunks = [ChunkData(content="parent", chunk_role="parent", chunk_index=0)]
    await stage.process(chunks)
    assert len(client.calls) == 0


@pytest.mark.asyncio
async def test_returns_same_chunks():
    client = MockEmbeddingClient(dimensions=8)
    stage = EmbeddingStage(client=client)
    chunks = [
        ChunkData(content="parent", chunk_role="parent", chunk_index=0),
        ChunkData(content="leaf", chunk_role="leaf", chunk_index=0, parent_temp_index=0),
    ]
    result = await stage.process(chunks)
    assert len(result) == 2
