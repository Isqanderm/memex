from unittest.mock import AsyncMock, MagicMock, patch

from src.main import app, lifespan


async def test_lifespan_starts_and_stops_without_error():
    mock_settings = MagicMock()
    mock_settings.upload_dir = MagicMock()

    mock_worker = MagicMock()
    mock_worker.start = AsyncMock(return_value=None)

    with (
        patch("src.main.get_settings", return_value=mock_settings),
        patch("src.main.init_db", new_callable=AsyncMock),
        patch("src.main.close_db", new_callable=AsyncMock),
        patch("src.main.get_session_factory", return_value=MagicMock()),
        patch("src.main.get_ingestion_pipeline", return_value=MagicMock()),
        patch("src.main.IngestionWorker", return_value=mock_worker),
    ):
        async with lifespan(app):
            pass
