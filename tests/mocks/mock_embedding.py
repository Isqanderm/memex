import random


class MockEmbeddingClient:
    def __init__(self, dimensions: int = 1536):
        self.dimensions = dimensions
        self.calls: list[list[str]] = []

    async def embed_batch(self, texts: list[str]) -> list[list[float]]:
        self.calls.append(texts)
        return [[random.uniform(-1, 1) for _ in range(self.dimensions)] for _ in texts]
