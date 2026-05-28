from src.models.chunk import ChunkData


class OpenAIEmbeddingClient:
    def __init__(self, api_key: str, model: str = "text-embedding-3-small"):
        import openai
        self._client = openai.AsyncOpenAI(api_key=api_key)
        self.model = model

    async def embed_batch(self, texts: list[str]) -> list[list[float]]:
        response = await self._client.embeddings.create(input=texts, model=self.model)
        return [item.embedding for item in response.data]


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
