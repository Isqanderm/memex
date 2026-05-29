from sqlalchemy.ext.asyncio import AsyncSession, create_async_engine, async_sessionmaker


def create_engine(database_url: str):
    return create_async_engine(database_url, echo=False, pool_pre_ping=True)


def create_session_factory(engine) -> async_sessionmaker[AsyncSession]:
    return async_sessionmaker(engine, expire_on_commit=False)


_engine = None
_session_factory = None


def get_session_factory() -> async_sessionmaker[AsyncSession]:
    if _session_factory is None:
        raise RuntimeError("Session factory not initialized. Call init_db() first.")
    return _session_factory


async def init_db(database_url: str):
    global _engine, _session_factory
    _engine = create_engine(database_url)
    _session_factory = create_session_factory(_engine)


async def close_db():
    global _engine
    if _engine:
        await _engine.dispose()
        _engine = None


async def get_db_session() -> AsyncSession:
    """FastAPI dependency для получения сессии БД.

    Явный try/finally вместо вложенных context manager'ов — избегает
    IllegalStateChangeError когда cleanup сессии пересекается с
    незавершённой операцией (например, при перехваченном исключении в хэндлере).
    """
    factory = get_session_factory()
    session: AsyncSession = factory()
    try:
        yield session
        await session.commit()
    except Exception:
        await session.rollback()
        raise
    finally:
        await session.close()
