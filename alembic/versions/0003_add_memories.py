"""add memories tables

Revision ID: 0003
Revises: 0002
Create Date: 2026-06-01
"""
from alembic import op
import sqlalchemy as sa

revision = '0003'
down_revision = '0002'
branch_labels = None
depends_on = None


def upgrade() -> None:
    op.create_table(
        'memories',
        sa.Column('id', sa.UUID(), primary_key=True),
        sa.Column('content', sa.Text(), nullable=False),
        sa.Column('raw_input', sa.Text(), nullable=False),
        sa.Column('source', sa.String(20), nullable=False),
        sa.Column('is_active', sa.Boolean(), nullable=False, server_default='true'),
        sa.Column('forget_after', sa.DateTime(timezone=True), nullable=True),
        sa.Column('relation', sa.String(20), nullable=True),
        sa.Column('parent_id', sa.UUID(), sa.ForeignKey('memories.id'), nullable=True),
        sa.Column('content_vector', sa.Text(), nullable=True),
        sa.Column('created_at', sa.DateTime(timezone=True), server_default=sa.func.now()),
    )
    op.execute("""
        ALTER TABLE memories
        ALTER COLUMN content_vector TYPE vector(1536)
        USING content_vector::vector(1536)
    """)
    op.create_index('ix_memories_is_active', 'memories', ['is_active'])
    op.create_index(
        'ix_memories_vector',
        'memories',
        ['content_vector'],
        postgresql_using='hnsw',
        postgresql_with={'m': 16, 'ef_construction': 64},
        postgresql_ops={'content_vector': 'vector_cosine_ops'},
    )

    op.create_table(
        'memory_extraction_jobs',
        sa.Column('id', sa.UUID(), primary_key=True),
        sa.Column('source_ref', sa.Text(), nullable=False),
        sa.Column('source', sa.String(20), nullable=False),
        sa.Column('status', sa.String(20), nullable=False, server_default='pending'),
        sa.Column('facts_extracted', sa.Integer(), nullable=False, server_default='0'),
        sa.Column('error', sa.Text(), nullable=True),
        sa.Column('created_at', sa.DateTime(timezone=True), server_default=sa.func.now()),
        sa.Column('updated_at', sa.DateTime(timezone=True), server_default=sa.func.now()),
    )


def downgrade() -> None:
    op.drop_table('memory_extraction_jobs')
    op.drop_index('ix_memories_vector', 'memories')
    op.drop_index('ix_memories_is_active', 'memories')
    op.drop_table('memories')
