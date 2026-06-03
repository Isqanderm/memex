import pytest
from alembic.config import Config

from alembic import command


@pytest.mark.integration
def test_migrations_upgrade_head_succeeds(sync_db_url: str) -> None:
    """alembic upgrade head completes without error.

    The apply_migrations autouse fixture already ran upgrade head before
    this test. Running it again is idempotent — verifies the head state
    is stable.
    """
    cfg = Config("alembic.ini")
    cfg.set_main_option("sqlalchemy.url", sync_db_url)
    command.upgrade(cfg, "head")


@pytest.mark.integration
def test_migrations_downgrade_base_and_upgrade(sync_db_url: str) -> None:
    """Full round-trip: downgrade to base then upgrade back to head.

    Verifies both downgrade() and upgrade() paths are functional,
    catching broken downgrade scripts before they cause issues in prod.
    """
    cfg = Config("alembic.ini")
    cfg.set_main_option("sqlalchemy.url", sync_db_url)
    command.downgrade(cfg, "base")
    command.upgrade(cfg, "head")
