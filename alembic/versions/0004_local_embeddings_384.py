"""switch to local embeddings: resize vectors 1536→384

Revision ID: 0004
Revises: 0003
Create Date: 2026-06-01
"""
from alembic import op
import sqlalchemy as sa

revision = '0004'
down_revision = '0003'
branch_labels = None
depends_on = None


def upgrade() -> None:
    # Drop HNSW indexes before altering column type
    op.execute("DROP INDEX IF EXISTS ix_chunks_vector")
    op.execute("DROP INDEX IF EXISTS ix_memories_vector")

    # NULL out existing vectors — they're 1536-dim, incompatible with new 384-dim model
    op.execute("UPDATE chunks SET content_vector = NULL")
    op.execute("UPDATE memories SET content_vector = NULL")

    # Resize columns
    op.execute("ALTER TABLE chunks ALTER COLUMN content_vector TYPE vector(384) USING NULL::vector(384)")
    op.execute("ALTER TABLE memories ALTER COLUMN content_vector TYPE vector(384) USING NULL::vector(384)")

    # Recreate HNSW indexes with new dimension
    op.execute("""
        CREATE INDEX ix_chunks_vector ON chunks
        USING hnsw (content_vector vector_cosine_ops)
        WITH (m = 16, ef_construction = 64)
    """)
    op.execute("""
        CREATE INDEX ix_memories_vector ON memories
        USING hnsw (content_vector vector_cosine_ops)
        WITH (m = 16, ef_construction = 64)
    """)


def downgrade() -> None:
    op.execute("DROP INDEX IF EXISTS ix_chunks_vector")
    op.execute("DROP INDEX IF EXISTS ix_memories_vector")
    op.execute("UPDATE chunks SET content_vector = NULL")
    op.execute("UPDATE memories SET content_vector = NULL")
    op.execute("ALTER TABLE chunks ALTER COLUMN content_vector TYPE vector(1536) USING NULL::vector(1536)")
    op.execute("ALTER TABLE memories ALTER COLUMN content_vector TYPE vector(1536) USING NULL::vector(1536)")
    op.execute("""
        CREATE INDEX ix_chunks_vector ON chunks
        USING hnsw (content_vector vector_cosine_ops)
        WITH (m = 16, ef_construction = 64)
    """)
