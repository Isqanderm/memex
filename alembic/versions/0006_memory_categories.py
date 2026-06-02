"""add category and project to memories

Revision ID: 0006
Revises: 0005
Create Date: 2026-06-02
"""
from alembic import op
import sqlalchemy as sa

revision = '0006'
down_revision = '0005'
branch_labels = None
depends_on = None


def upgrade() -> None:
    op.add_column('memories', sa.Column('category', sa.String(20), nullable=True))
    op.add_column('memories', sa.Column('project', sa.String(100), nullable=True))
    op.create_index('ix_memories_category', 'memories', ['category'])


def downgrade() -> None:
    op.drop_index('ix_memories_category', 'memories')
    op.drop_column('memories', 'project')
    op.drop_column('memories', 'category')
