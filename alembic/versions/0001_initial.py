"""initial schema

Revision ID: 0001
Revises:
Create Date: 2026-05-29
"""
from alembic import op
import sqlalchemy as sa

revision = '0001'
down_revision = None
branch_labels = None
depends_on = None


def upgrade() -> None:
    op.execute('CREATE EXTENSION IF NOT EXISTS vector')
    op.execute('CREATE EXTENSION IF NOT EXISTS pg_trgm')

    op.create_table('documents',
        sa.Column('id', sa.UUID(), primary_key=True),
        sa.Column('source', sa.Text(), nullable=False),
        sa.Column('mime_type', sa.String(100), nullable=False),
        sa.Column('title', sa.Text()),
        sa.Column('checksum', sa.String(64), nullable=False),
        sa.Column('metadata', sa.JSON(), nullable=False, server_default='{}'),
        sa.Column('indexed_at', sa.DateTime(timezone=True), server_default=sa.func.now()),
        sa.UniqueConstraint('checksum', name='uq_documents_checksum'),
    )

    op.create_table('chunks',
        sa.Column('id', sa.UUID(), primary_key=True),
        sa.Column('doc_id', sa.UUID(), sa.ForeignKey('documents.id', ondelete='CASCADE'), nullable=False),
        sa.Column('parent_chunk_id', sa.UUID(), sa.ForeignKey('chunks.id'), nullable=True),
        sa.Column('chunk_role', sa.String(10), nullable=False),
        sa.Column('chunk_index', sa.Integer(), nullable=False),
        sa.Column('section_heading', sa.Text()),
        sa.Column('section_level', sa.Integer()),
        sa.Column('page_number', sa.Integer()),
        sa.Column('prev_chunk_id', sa.UUID(), sa.ForeignKey('chunks.id'), nullable=True),
        sa.Column('next_chunk_id', sa.UUID(), sa.ForeignKey('chunks.id'), nullable=True),
        sa.Column('language', sa.String(20), nullable=False, server_default='simple'),
        sa.Column('content', sa.Text(), nullable=False),
        sa.Column('content_vector', sa.Text(), nullable=True),  # будет Vector через raw SQL
        sa.Column('tsv', sa.Text(), nullable=True),  # будет tsvector через raw SQL
    )

    op.create_table('ingestion_jobs',
        sa.Column('id', sa.UUID(), primary_key=True),
        sa.Column('status', sa.String(20), nullable=False, server_default='pending'),
        sa.Column('source', sa.Text(), nullable=False),
        sa.Column('checksum', sa.String(64), nullable=False),
        sa.Column('doc_id', sa.UUID(), sa.ForeignKey('documents.id'), nullable=True),
        sa.Column('error', sa.Text(), nullable=True),
        sa.Column('created_at', sa.DateTime(timezone=True), server_default=sa.func.now()),
        sa.Column('updated_at', sa.DateTime(timezone=True), server_default=sa.func.now()),
    )

    # Изменить типы через raw SQL (pgvector и tsvector не поддерживаются через SQLAlchemy column types напрямую)
    op.execute("ALTER TABLE chunks ALTER COLUMN content_vector TYPE vector(1536) USING NULL::vector(1536)")
    op.execute("ALTER TABLE chunks ALTER COLUMN tsv TYPE tsvector USING NULL::tsvector")

    # Индексы
    op.execute("CREATE INDEX idx_chunks_vector ON chunks USING hnsw (content_vector vector_cosine_ops) WHERE content_vector IS NOT NULL")
    op.execute("CREATE INDEX idx_chunks_tsv ON chunks USING GIN (tsv) WHERE tsv IS NOT NULL")
    op.execute("CREATE INDEX idx_ingestion_jobs_pending ON ingestion_jobs(status, created_at) WHERE status = 'pending'")
    op.execute("CREATE UNIQUE INDEX uq_ingestion_jobs_checksum_active ON ingestion_jobs(checksum) WHERE status IN ('pending', 'processing')")


def downgrade() -> None:
    op.drop_table('ingestion_jobs')
    op.drop_table('chunks')
    op.drop_table('documents')
