import asyncio
import logging
from collections.abc import AsyncGenerator
from contextlib import asynccontextmanager

from fastapi import FastAPI
from fastapi.staticfiles import StaticFiles
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from src.config import get_settings
from src.db.session import close_db, get_session_factory, init_db
from src.dependencies import get_ingestion_pipeline
from src.ingestion.worker import IngestionWorker

logging.basicConfig(
    level=logging.INFO,
    format="%(levelname)s %(name)s %(message)s",
)
logger = logging.getLogger(__name__)
logging.getLogger("memex.profile").setLevel(logging.INFO)

_worker_task: asyncio.Task[None] | None = None
_expiry_task: asyncio.Task[None] | None = None


async def _memory_expiry_loop(
    session_factory: async_sessionmaker[AsyncSession],
) -> None:
    """Runs every hour: marks memories with forget_after < NOW() as inactive."""
    from src.db.repositories.memory_repo import MemoryRepository

    while True:
        await asyncio.sleep(3600)
        try:
            async with session_factory() as session:
                repo = MemoryRepository(session)
                count = await repo.expire_stale()
                await session.commit()
                if count:
                    logger.info("Expired %d stale memories", count)
        except Exception as exc:
            logger.warning("Memory expiry error: %s", exc)


@asynccontextmanager
async def lifespan(app: FastAPI) -> AsyncGenerator[None, None]:
    global _worker_task, _expiry_task
    # Import before use — get_memory_service is referenced below before the
    # deferred import block, causing UnboundLocalError in Python's scoping rules.
    from src.dependencies import get_embedding_client, get_memory_service, get_retrieval_service

    settings = get_settings()
    await init_db(settings.database_url)
    settings.upload_dir.mkdir(parents=True, exist_ok=True)

    session_factory = get_session_factory()
    worker = IngestionWorker(
        session_factory=session_factory,
        pipeline=get_ingestion_pipeline(),
        memory_service_factory=get_memory_service,
    )
    _worker_task = asyncio.create_task(worker.start())
    _expiry_task = asyncio.create_task(_memory_expiry_loop(session_factory))

    # Warm up models so first query isn't slow
    loop = asyncio.get_event_loop()
    await loop.run_in_executor(None, get_retrieval_service().reranker._get_model)
    embed_client = get_embedding_client()
    if hasattr(embed_client, "_get_model"):
        # Load model AND run one dummy inference to trigger JIT compilation
        await loop.run_in_executor(
            None,
            lambda: embed_client._get_model().encode(["warmup"], normalize_embeddings=True),
        )
    logger.info("Memex started (models warmed up)")

    yield

    worker.stop()
    for task in (_worker_task, _expiry_task):
        if task:
            task.cancel()
            try:
                await task
            except asyncio.CancelledError:
                pass
    await close_db()
    logger.info("Memex stopped")


app = FastAPI(title="Memex", version="0.1.0", lifespan=lifespan)

import time as _time  # noqa: E402
from collections.abc import Awaitable, Callable  # noqa: E402

from starlette.middleware.base import BaseHTTPMiddleware  # noqa: E402
from starlette.requests import Request as _Request  # noqa: E402
from starlette.responses import Response as _Response  # noqa: E402


class _TimingMiddleware(BaseHTTPMiddleware):
    async def dispatch(
        self,
        request: _Request,
        call_next: Callable[[_Request], Awaitable[_Response]],
    ) -> _Response:
        t0 = _time.perf_counter()
        response = await call_next(request)
        ms = (_time.perf_counter() - t0) * 1000
        logger.info("%.0fms  %s %s", ms, request.method, request.url.path)
        return response

app.add_middleware(_TimingMiddleware)

try:
    app.mount("/static", StaticFiles(directory="static"), name="static")
except Exception:
    pass  # static dir may not exist yet

from src.api import documents as docs_router  # noqa: E402
from src.api import jobs as jobs_router  # noqa: E402
from src.api import query as query_router  # noqa: E402
from src.api.memories import router as memory_router  # noqa: E402
from src.ui import pages as ui_router  # noqa: E402


@app.get("/health")
async def health() -> str:
    return "ok"


app.include_router(docs_router.router, prefix="/api")
app.include_router(query_router.router, prefix="/api")
app.include_router(jobs_router.router, prefix="/api")
app.include_router(ui_router.router)
app.include_router(memory_router)
