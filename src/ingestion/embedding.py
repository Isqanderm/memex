from src.models.chunk import ChunkData


class LocalEmbeddingClient:
    """Local sentence-transformers embedding — no API calls, no cost.

    Uses e5-style prefixes: queries get "query: ", passages get "passage: ".
    Model is loaded once and cached at class level.
    """
    _model = None
    _model_name: str = ""

    def __init__(self, model: str = "intfloat/multilingual-e5-small"):
        self.model_name = model

    def _get_model(self):
        if LocalEmbeddingClient._model is None or LocalEmbeddingClient._model_name != self.model_name:
            from sentence_transformers import SentenceTransformer
            LocalEmbeddingClient._model = SentenceTransformer(self.model_name)
            LocalEmbeddingClient._model_name = self.model_name
        return LocalEmbeddingClient._model

    async def embed_batch(self, texts: list[str], is_query: bool = False) -> list[list[float]]:
        import asyncio
        prefix = "query: " if is_query else "passage: "
        prefixed = [prefix + t for t in texts]
        loop = asyncio.get_event_loop()
        embeddings = await loop.run_in_executor(
            None,
            lambda: self._get_model().encode(prefixed, normalize_embeddings=True).tolist(),
        )
        return embeddings

    @property
    def dimensions(self) -> int:
        return self._get_model().get_sentence_embedding_dimension()


class EmbeddingStage:
    def __init__(self, client, batch_size: int = 512):
        self.client = client
        self.batch_size = batch_size

    async def process(self, chunks: list[ChunkData]) -> list[ChunkData]:
        leaves = [c for c in chunks if c.chunk_role == "leaf"]

        for i in range(0, len(leaves), self.batch_size):
            batch = leaves[i:i + self.batch_size]
            texts = [c.content for c in batch]
            embeddings = await self.client.embed_batch(texts)
            for chunk, embedding in zip(batch, embeddings):
                chunk.embedding = embedding

        return chunks
