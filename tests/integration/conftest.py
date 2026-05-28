import os
import pytest
import pytest_asyncio
from testcontainers.postgres import PostgresContainer
from sqlalchemy.ext.asyncio import create_async_engine, async_sessionmaker, AsyncSession

PG_IMAGE = "pgvector/pgvector:pg15"

# Docker Desktop on macOS exposes the daemon via a user-local socket.
# Set DOCKER_HOST if not already set so testcontainers can connect.
_DOCKER_DESKTOP_SOCK = os.path.expanduser("~/.docker/run/docker.sock")
if not os.environ.get("DOCKER_HOST") and os.path.exists(_DOCKER_DESKTOP_SOCK):
    os.environ["DOCKER_HOST"] = f"unix://{_DOCKER_DESKTOP_SOCK}"


@pytest.fixture(scope="session")
def pg_container():
    with PostgresContainer(PG_IMAGE, username="test", password="test", dbname="test") as pg:
        yield pg


@pytest.fixture(scope="session")
def db_url(pg_container):
    host = pg_container.get_container_host_ip()
    port = pg_container.get_exposed_port(5432)
    return f"postgresql+asyncpg://test:test@{host}:{port}/test"


@pytest.fixture(scope="session")
def sync_db_url(pg_container):
    host = pg_container.get_container_host_ip()
    port = pg_container.get_exposed_port(5432)
    return f"postgresql://test:test@{host}:{port}/test"


@pytest.fixture(scope="session", autouse=True)
def apply_migrations(sync_db_url):
    import os
    from alembic.config import Config
    from alembic import command
    os.environ["DATABASE_URL"] = sync_db_url
    cfg = Config("alembic.ini")
    cfg.set_main_option("sqlalchemy.url", sync_db_url)
    command.upgrade(cfg, "head")


@pytest_asyncio.fixture(scope="session", loop_scope="session")
async def engine(db_url):
    eng = create_async_engine(db_url)
    yield eng
    await eng.dispose()


@pytest_asyncio.fixture(scope="session", loop_scope="session")
async def session_factory(engine):
    return async_sessionmaker(engine, expire_on_commit=False)


@pytest_asyncio.fixture(loop_scope="session")
async def db_session(session_factory) -> AsyncSession:
    async with session_factory() as session:
        yield session
        await session.rollback()
