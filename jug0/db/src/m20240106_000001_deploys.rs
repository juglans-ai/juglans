use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Deploys::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Deploys::Id).uuid().not_null().primary_key())
                    .col(
                        ColumnDef::new(Deploys::Slug)
                            .string_len(60)
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(Deploys::OrgId).string_len(50))
                    .col(ColumnDef::new(Deploys::UserId).uuid())
                    .col(ColumnDef::new(Deploys::Repo).string_len(255).not_null())
                    .col(
                        ColumnDef::new(Deploys::Branch)
                            .string_len(100)
                            .default("main"),
                    )
                    .col(
                        ColumnDef::new(Deploys::Status)
                            .string_len(20)
                            .not_null()
                            .default("pending"),
                    )
                    .col(ColumnDef::new(Deploys::Url).text())
                    .col(ColumnDef::new(Deploys::ImageUri).text())
                    .col(ColumnDef::new(Deploys::Env).json().not_null().default("{}"))
                    .col(ColumnDef::new(Deploys::ErrorMessage).text())
                    .col(
                        ColumnDef::new(Deploys::CreatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Deploys::UpdatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Deploys::Table, Deploys::OrgId)
                            .to(Organizations::Table, Organizations::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Deploys::Table, Deploys::UserId)
                            .to(Users::Table, Users::Id),
                    )
                    .to_owned(),
            )
            .await?;

        // Indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_deploys_org_user")
                    .table(Deploys::Table)
                    .col(Deploys::OrgId)
                    .col(Deploys::UserId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_deploys_status")
                    .table(Deploys::Table)
                    .col(Deploys::Status)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_deploys_created_at")
                    .table(Deploys::Table)
                    .col((Deploys::CreatedAt, IndexOrder::Desc))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Deploys::Table).if_exists().to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Deploys {
    Table,
    Id,
    Slug,
    OrgId,
    UserId,
    Repo,
    Branch,
    Status,
    Url,
    ImageUri,
    Env,
    ErrorMessage,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Organizations {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
