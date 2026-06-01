"""fix chunk self-referential FKs: add ON DELETE SET NULL

Revision ID: 0005
Revises: 0004
Create Date: 2026-06-01

Without ON DELETE SET NULL, deleting a parent chunk fails because leaf
chunks still reference it via parent_chunk_id. Same for prev/next linked list.
"""
from alembic import op

revision = '0005'
down_revision = '0004'
branch_labels = None
depends_on = None


def upgrade() -> None:
    # Drop existing FK constraints (auto-named by PostgreSQL)
    op.execute("""
        ALTER TABLE chunks
            DROP CONSTRAINT IF EXISTS chunks_parent_chunk_id_fkey,
            DROP CONSTRAINT IF EXISTS chunks_prev_chunk_id_fkey,
            DROP CONSTRAINT IF EXISTS chunks_next_chunk_id_fkey
    """)

    # Re-add with ON DELETE SET NULL
    op.execute("""
        ALTER TABLE chunks
            ADD CONSTRAINT chunks_parent_chunk_id_fkey
                FOREIGN KEY (parent_chunk_id) REFERENCES chunks(id) ON DELETE SET NULL,
            ADD CONSTRAINT chunks_prev_chunk_id_fkey
                FOREIGN KEY (prev_chunk_id) REFERENCES chunks(id) ON DELETE SET NULL,
            ADD CONSTRAINT chunks_next_chunk_id_fkey
                FOREIGN KEY (next_chunk_id) REFERENCES chunks(id) ON DELETE SET NULL
    """)


def downgrade() -> None:
    op.execute("""
        ALTER TABLE chunks
            DROP CONSTRAINT IF EXISTS chunks_parent_chunk_id_fkey,
            DROP CONSTRAINT IF EXISTS chunks_prev_chunk_id_fkey,
            DROP CONSTRAINT IF EXISTS chunks_next_chunk_id_fkey
    """)
    op.execute("""
        ALTER TABLE chunks
            ADD CONSTRAINT chunks_parent_chunk_id_fkey
                FOREIGN KEY (parent_chunk_id) REFERENCES chunks(id),
            ADD CONSTRAINT chunks_prev_chunk_id_fkey
                FOREIGN KEY (prev_chunk_id) REFERENCES chunks(id),
            ADD CONSTRAINT chunks_next_chunk_id_fkey
                FOREIGN KEY (next_chunk_id) REFERENCES chunks(id)
    """)
