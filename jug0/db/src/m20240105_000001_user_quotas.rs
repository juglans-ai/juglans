use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(UserQuotas::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(UserQuotas::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(UserQuotas::UserId).uuid().not_null())
                    .col(
                        ColumnDef::new(UserQuotas::Tier)
                            .string_len(20)
                            .not_null()
                            .default("free"),
                    )
                    .col(ColumnDef::new(UserQuotas::MonthlyLimit).big_integer())
                    .col(
                        ColumnDef::new(UserQuotas::CreatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(UserQuotas::UpdatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_user_quotas_user_id")
                            .from(UserQuotas::Table, UserQuotas::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_user_quotas_user_id")
                    .table(UserQuotas::Table)
                    .col(UserQuotas::UserId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(UserQuotas::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum UserQuotas {
    Table,
    Id,
    UserId,
    Tier,
    MonthlyLimit,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
