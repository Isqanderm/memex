import asyncio
import logging
from contextlib import asynccontextmanager
from fastapi import FastAPI
from fastapi.staticfiles import StaticFiles
from src.db.session import init_db, close_db, get_session_factory
from src.ingestion.worker import IngestionWorker
from src.dependencies import get_ingestion_pipeline
from src.config import get_settings

logger = logging.getLogger(__name__)

_worker_task = None


@asynccontextmanager
async def lifespan(app: FastAPI):
    global _worker_task
    settings = get_settings()
    await init_db(settings.database_url)
    settings.upload_dir.mkdir(parents=True, exist_ok=True)

    worker = IngestionWorker(
        session_factory=get_session_factory(),
        pipeline=get_ingestion_pipeline(),
    )
    _worker_task = asyncio.create_task(worker.start())
    logger.info("Memex started")

    yield

    worker.stop()
    if _worker_task:
        _worker_task.cancel()
        try:
            await _worker_task
        except asyncio.CancelledError:
            pass
    await close_db()
    logger.info("Memex stopped")


app = FastAPI(title="Memex", version="0.1.0", lifespan=lifespan)

try:
    app.mount("/static", StaticFiles(directory="static"), name="static")
except Exception:
    pass  # static dir may not exist yet

# Routers will be imported below
from src.api import documents as docs_router
from src.api import query as query_router
from src.api import jobs as jobs_router
from src.ui import pages as ui_router

app.include_router(docs_router.router, prefix="/api")
app.include_router(query_router.router, prefix="/api")
app.include_router(jobs_router.router, prefix="/api")
app.include_router(ui_router.router)
