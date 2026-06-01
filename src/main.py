import asyncio
import logging
from contextlib import asynccontextmanager
from fastapi import FastAPI
from fastapi.staticfiles import StaticFiles
from src.db.session import init_db, close_db, get_session_factory
from src.ingestion.worker import IngestionWorker
from src.dependencies import get_ingestion_pipeline
from src.config import get_settings

logging.basicConfig(
    level=logging.INFO,
    format="%(levelname)s %(name)s %(message)s",
)
logger = logging.getLogger(__name__)
logging.getLogger("memex.profile").setLevel(logging.INFO)

_worker_task = None
_expiry_task = None


async def _memory_expiry_loop(session_factory):
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
async def lifespan(app: FastAPI):
    global _worker_task, _expiry_task
    settings = get_settings()
    await init_db(settings.database_url)
    settings.upload_dir.mkdir(parents=True, exist_ok=True)

    session_factory = get_session_factory()
    worker = IngestionWorker(
        session_factory=session_factory,
        pipeline=get_ingestion_pipeline(),
    )
    _worker_task = asyncio.create_task(worker.start())
    _expiry_task = asyncio.create_task(_memory_expiry_loop(session_factory))
    logger.info("Memex started")

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

import time as _time
from starlette.middleware.base import BaseHTTPMiddleware
from starlette.requests import Request as _Request

class _TimingMiddleware(BaseHTTPMiddleware):
    async def dispatch(self, request: _Request, call_next):
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

# Routers will be imported below
from src.api import documents as docs_router
from src.api import query as query_router
from src.api import jobs as jobs_router
from src.ui import pages as ui_router
from src.api.memories import router as memory_router

app.include_router(docs_router.router, prefix="/api")
app.include_router(query_router.router, prefix="/api")
app.include_router(jobs_router.router, prefix="/api")
app.include_router(ui_router.router)
app.include_router(memory_router)
