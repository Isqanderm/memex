"""fix ingestion_jobs.doc_id FK to SET NULL on document delete

Revision ID: 0002
Revises: 0001
Create Date: 2026-05-30
"""
from alembic import op

revision = "0002"
down_revision = "0001"
branch_labels = None
depends_on = None


def upgrade() -> None:
    op.drop_constraint("ingestion_jobs_doc_id_fkey", "ingestion_jobs", type_="foreignkey")
    op.create_foreign_key(
        "ingestion_jobs_doc_id_fkey",
        "ingestion_jobs",
        "documents",
        ["doc_id"],
        ["id"],
        ondelete="SET NULL",
    )


def downgrade() -> None:
    op.drop_constraint("ingestion_jobs_doc_id_fkey", "ingestion_jobs", type_="foreignkey")
    op.create_foreign_key(
        "ingestion_jobs_doc_id_fkey",
        "ingestion_jobs",
        "documents",
        ["doc_id"],
        ["id"],
    )
