"""
Re-embed all chunks and memories with the current embedding model.

Run after switching embedding_provider or changing embedding_dimensions:
    uv run python scripts/reindex.py

What it does:
  1. Finds all chunks with NULL content_vector (leaf chunks only)
  2. Embeds them with the configured model
  3. Updates the DB
  4. Same for memories

Requires server to be running (uses its embedding client config).
"""
import asyncio
import os
import sys

os.chdir(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, ".")

from dotenv import load_dotenv
load_dotenv(".env")


async def main():
    from sqlalchemy.ext.asyncio import create_async_engine, async_sessionmaker
    from sqlalchemy import select, update, text
    from src.db.models import Chunk, Memory
    from src.dependencies import get_embedding_client

    db_url = os.getenv("DATABASE_URL", "postgresql+asyncpg://memex:memex@localhost:5432/memex")
    engine = create_async_engine(db_url)
    Session = async_sessionmaker(engine, expire_on_commit=False)

    client = get_embedding_client()
    # Warm up model
    if hasattr(client, "_get_model"):
        print("Loading embedding model...")
        client._get_model().encode(["warmup"], normalize_embeddings=True)
        print(f"Model ready: {client.model_name}")

    async with Session() as session:
        # ── Re-embed chunks ────────────────────────────────────────────────
        result = await session.execute(
            select(Chunk).where(
                Chunk.chunk_role == "leaf",
                Chunk.content_vector == None,
            )
        )
        chunks = result.scalars().all()
        print(f"\nChunks to embed: {len(chunks)}")

        batch_size = 64
        for i in range(0, len(chunks), batch_size):
            batch = chunks[i:i + batch_size]
            texts = [c.content for c in batch]
            embeddings = await client.embed_batch(texts, is_query=False)
            for chunk, emb in zip(batch, embeddings):
                chunk.content_vector = emb
            await session.flush()
            print(f"  chunks {i + 1}–{min(i + batch_size, len(chunks))}/{len(chunks)} ✓")

        # ── Re-embed memories ──────────────────────────────────────────────
        result2 = await session.execute(
            select(Memory).where(Memory.content_vector == None, Memory.is_active == True)
        )
        memories = result2.scalars().all()
        print(f"\nMemories to embed: {len(memories)}")

        for i in range(0, len(memories), batch_size):
            batch = memories[i:i + batch_size]
            texts = [m.content for m in batch]
            embeddings = await client.embed_batch(texts, is_query=False)
            for mem, emb in zip(batch, embeddings):
                mem.content_vector = emb
            await session.flush()
            print(f"  memories {i + 1}–{min(i + batch_size, len(memories))}/{len(memories)} ✓")

        await session.commit()

    await engine.dispose()
    print("\nDone. All vectors updated.")


if __name__ == "__main__":
    asyncio.run(main())
