from fastapi import APIRouter, Depends
from pydantic import BaseModel
from sqlalchemy.ext.asyncio import AsyncSession
from src.api.documents import get_db_session
from src.dependencies import get_retrieval_service, get_embedding_client
from src.retrieval.memory_search import MemorySearch
from src.db.repositories.memory_repo import MemoryRepository

router = APIRouter(tags=["query"])


class QueryRequest(BaseModel):
    query: str
    top_k: int = 5
    memory_category: str | None = None


class QueryResponse(BaseModel):
    answer: str
    sources: list[dict]


@router.post("/query", response_model=QueryResponse)
async def query_documents(
    request: QueryRequest,
    session: AsyncSession = Depends(get_db_session),
):
    service = get_retrieval_service()
    embedding_client = get_embedding_client()

    async def embed(text: str) -> list[float]:
        results = await embedding_client.embed_batch([text], is_query=True)
        return results[0]

    # Create per-request memory search with session-scoped repository
    memory_search = MemorySearch(repo=MemoryRepository(session))

    result = await service.query(
        session, request.query, embed_fn=embed,
        memory_search=memory_search,
        memory_category=request.memory_category,
    )
    return QueryResponse(answer=result.answer, sources=result.sources)


class ChunksRequest(BaseModel):
    query: str
    top_k: int = 5


@router.post("/search/chunks")
async def search_chunks(
    request: ChunksRequest,
    session: AsyncSession = Depends(get_db_session),
):
    service = get_retrieval_service()
    embedding_client = get_embedding_client()

    async def embed(text: str) -> list[float]:
        results = await embedding_client.embed_batch([text], is_query=True)
        return results[0]

    # Create per-request memory search with session-scoped repository (for consistency, even if not used by search_chunks)
    memory_search = MemorySearch(repo=MemoryRepository(session))

    chunks = await service.search_chunks(session, request.query, embed_fn=embed, top_k=request.top_k)
    return {"chunks": chunks}
