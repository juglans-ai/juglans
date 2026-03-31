use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(WorkflowRuns::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(WorkflowRuns::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(WorkflowRuns::WorkflowId).uuid().not_null())
                    .col(
                        ColumnDef::new(WorkflowRuns::Trigger)
                            .string_len(20)
                            .not_null()
                            .default("manual"),
                    )
                    .col(
                        ColumnDef::new(WorkflowRuns::Status)
                            .string_len(20)
                            .not_null()
                            .default("pending"),
                    )
                    .col(ColumnDef::new(WorkflowRuns::StartedAt).timestamp())
                    .col(ColumnDef::new(WorkflowRuns::CompletedAt).timestamp())
                    .col(ColumnDef::new(WorkflowRuns::Result).json())
                    .col(ColumnDef::new(WorkflowRuns::Error).text())
                    .col(
                        ColumnDef::new(WorkflowRuns::CreatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(WorkflowRuns::UpdatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_workflow_runs_workflow_id")
                            .from(WorkflowRuns::Table, WorkflowRuns::WorkflowId)
                            .to(Workflows::Table, Workflows::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Index for querying runs by workflow
        manager
            .create_index(
                Index::create()
                    .name("idx_workflow_runs_workflow_id")
                    .table(WorkflowRuns::Table)
                    .col(WorkflowRuns::WorkflowId)
                    .to_owned(),
            )
            .await?;

        // Index for querying by status
        manager
            .create_index(
                Index::create()
                    .name("idx_workflow_runs_status")
                    .table(WorkflowRuns::Table)
                    .col(WorkflowRuns::Status)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(WorkflowRuns::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum WorkflowRuns {
    Table,
    Id,
    WorkflowId,
    Trigger,
    Status,
    StartedAt,
    CompletedAt,
    Result,
    Error,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Workflows {
    Table,
    Id,
}
