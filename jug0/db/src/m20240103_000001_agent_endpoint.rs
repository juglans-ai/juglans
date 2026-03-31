use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 1. Add endpoint_url column to agents
        manager
            .alter_table(
                Table::alter()
                    .table(Agents::Table)
                    .add_column(ColumnDef::new(Agents::EndpointUrl).text())
                    .to_owned(),
            )
            .await?;

        // 2. Migrate existing workflow endpoint_url data
        let db = manager.get_connection();
        db.execute_unprepared(
            "UPDATE agents SET endpoint_url = (SELECT endpoint_url FROM workflows WHERE workflows.id = agents.workflow_id) WHERE workflow_id IS NOT NULL"
        ).await?;

        // 3. Drop FK constraint
        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .name("fk_agents_workflow_id")
                    .table(Agents::Table)
                    .to_owned(),
            )
            .await?;

        // 4. Drop workflow_id column
        manager
            .alter_table(
                Table::alter()
                    .table(Agents::Table)
                    .drop_column(Agents::WorkflowId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 1. Re-add workflow_id column
        manager
            .alter_table(
                Table::alter()
                    .table(Agents::Table)
                    .add_column(ColumnDef::new(Agents::WorkflowId).uuid())
                    .to_owned(),
            )
            .await?;

        // 2. Re-create FK
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_agents_workflow_id")
                    .from(Agents::Table, Agents::WorkflowId)
                    .to(Workflows::Table, Workflows::Id)
                    .to_owned(),
            )
            .await?;

        // 3. Drop endpoint_url column
        manager
            .alter_table(
                Table::alter()
                    .table(Agents::Table)
                    .drop_column(Agents::EndpointUrl)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum Agents {
    Table,
    WorkflowId,
    EndpointUrl,
}

#[derive(DeriveIden)]
enum Workflows {
    Table,
    Id,
}
