use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ==========================================
        // Tables
        // ==========================================

        // 1. Organizations
        manager
            .create_table(
                Table::create()
                    .table(Organizations::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Organizations::Id)
                            .string_len(50)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Organizations::Name)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Organizations::ApiKeyHash)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(ColumnDef::new(Organizations::PublicKey).text())
                    .col(
                        ColumnDef::new(Organizations::KeyAlgorithm)
                            .string_len(20)
                            .default("RS256"),
                    )
                    .col(
                        ColumnDef::new(Organizations::CreatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 2. Users
        manager
            .create_table(
                Table::create()
                    .table(Users::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Users::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Users::OrgId).string_len(50))
                    .col(ColumnDef::new(Users::ExternalId).string_len(255))
                    .col(ColumnDef::new(Users::Email).string_len(255))
                    .col(ColumnDef::new(Users::PasswordHash).string_len(255))
                    .col(ColumnDef::new(Users::Name).string_len(255))
                    .col(ColumnDef::new(Users::Username).string_len(50).unique_key())
                    .col(ColumnDef::new(Users::Role).string_len(50).default("user"))
                    .col(
                        ColumnDef::new(Users::CreatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Users::UpdatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Users::Table, Users::OrgId)
                            .to(Organizations::Table, Organizations::Id),
                    )
                    .to_owned(),
            )
            .await?;

        // 3. Prompts
        manager
            .create_table(
                Table::create()
                    .table(Prompts::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Prompts::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Prompts::Slug).string_len(255).not_null())
                    .col(ColumnDef::new(Prompts::OrgId).string_len(50))
                    .col(ColumnDef::new(Prompts::UserId).uuid())
                    .col(ColumnDef::new(Prompts::Name).string_len(255))
                    .col(ColumnDef::new(Prompts::Content).text().not_null())
                    .col(ColumnDef::new(Prompts::InputVariables).json().default("[]"))
                    .col(
                        ColumnDef::new(Prompts::PromptType)
                            .string_len(50)
                            .default("user"),
                    )
                    .col(ColumnDef::new(Prompts::Tags).json().default("[]"))
                    .col(
                        ColumnDef::new(Prompts::AllowedAgentSlugs)
                            .json()
                            .default("[\"*\"]"),
                    )
                    .col(ColumnDef::new(Prompts::IsPublic).boolean().default(false))
                    .col(ColumnDef::new(Prompts::IsSystem).boolean().default(false))
                    .col(ColumnDef::new(Prompts::UsageCount).integer().default(0))
                    .col(
                        ColumnDef::new(Prompts::CreatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Prompts::UpdatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Prompts::Table, Prompts::OrgId)
                            .to(Organizations::Table, Organizations::Id),
                    )
                    .to_owned(),
            )
            .await?;

        // 4. Agents (workflow_id FK added after workflows table)
        manager
            .create_table(
                Table::create()
                    .table(Agents::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Agents::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Agents::Slug).string_len(255).not_null())
                    .col(ColumnDef::new(Agents::OrgId).string_len(50))
                    .col(ColumnDef::new(Agents::UserId).uuid())
                    .col(ColumnDef::new(Agents::Name).string_len(255))
                    .col(ColumnDef::new(Agents::Description).text())
                    .col(ColumnDef::new(Agents::SystemPromptId).uuid())
                    .col(
                        ColumnDef::new(Agents::AllowedModels)
                            .json()
                            .default("[\"gpt-4o\"]"),
                    )
                    .col(
                        ColumnDef::new(Agents::DefaultModel)
                            .string_len(100)
                            .default("gpt-4o"),
                    )
                    .col(ColumnDef::new(Agents::Temperature).double().default(0.7))
                    .col(ColumnDef::new(Agents::McpConfig).json().default("[]"))
                    .col(ColumnDef::new(Agents::Skills).json().default("[]"))
                    .col(ColumnDef::new(Agents::ForkFromId).uuid())
                    .col(ColumnDef::new(Agents::WorkflowId).uuid())
                    .col(ColumnDef::new(Agents::IsPublic).boolean().default(false))
                    .col(ColumnDef::new(Agents::Username).string_len(50))
                    .col(ColumnDef::new(Agents::Avatar).string_len(500))
                    .col(
                        ColumnDef::new(Agents::CreatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Agents::UpdatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Agents::Table, Agents::OrgId)
                            .to(Organizations::Table, Organizations::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Agents::Table, Agents::SystemPromptId)
                            .to(Prompts::Table, Prompts::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Agents::Table, Agents::ForkFromId)
                            .to(Agents::Table, Agents::Id),
                    )
                    .to_owned(),
            )
            .await?;

        // 5. Workflows
        manager
            .create_table(
                Table::create()
                    .table(Workflows::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Workflows::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Workflows::Slug).string_len(255).not_null())
                    .col(ColumnDef::new(Workflows::OrgId).string_len(50))
                    .col(ColumnDef::new(Workflows::UserId).uuid())
                    .col(ColumnDef::new(Workflows::Name).string_len(255))
                    .col(ColumnDef::new(Workflows::Description).text())
                    .col(ColumnDef::new(Workflows::EndpointUrl).text())
                    .col(ColumnDef::new(Workflows::TriggerConfig).json())
                    .col(ColumnDef::new(Workflows::Definition).json())
                    .col(ColumnDef::new(Workflows::IsActive).boolean().default(true))
                    .col(ColumnDef::new(Workflows::IsPublic).boolean().default(false))
                    .col(
                        ColumnDef::new(Workflows::CreatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Workflows::UpdatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Workflows::Table, Workflows::OrgId)
                            .to(Organizations::Table, Organizations::Id),
                    )
                    .to_owned(),
            )
            .await?;

        // Add deferred FK: agents.workflow_id -> workflows.id
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_agents_workflow_id")
                    .from(Agents::Table, Agents::WorkflowId)
                    .to(Workflows::Table, Workflows::Id)
                    .to_owned(),
            )
            .await?;

        // 6. Chats
        manager
            .create_table(
                Table::create()
                    .table(Chats::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Chats::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Chats::OrgId).string_len(50))
                    .col(ColumnDef::new(Chats::UserId).uuid())
                    .col(ColumnDef::new(Chats::AgentId).uuid())
                    .col(ColumnDef::new(Chats::ExternalId).string_len(255))
                    .col(ColumnDef::new(Chats::Title).string_len(255))
                    .col(ColumnDef::new(Chats::Model).string_len(100))
                    .col(ColumnDef::new(Chats::LastMessageId).integer().default(0))
                    .col(ColumnDef::new(Chats::Metadata).json())
                    .col(ColumnDef::new(Chats::Incognito).boolean().default(false))
                    .col(
                        ColumnDef::new(Chats::CreatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Chats::UpdatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Chats::Table, Chats::OrgId)
                            .to(Organizations::Table, Organizations::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Chats::Table, Chats::AgentId)
                            .to(Agents::Table, Agents::Id),
                    )
                    .to_owned(),
            )
            .await?;

        // 7. Messages
        manager
            .create_table(
                Table::create()
                    .table(Messages::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Messages::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Messages::ChatId).uuid().not_null())
                    .col(ColumnDef::new(Messages::MessageId).integer().not_null())
                    .col(ColumnDef::new(Messages::Role).string_len(50).not_null())
                    .col(
                        ColumnDef::new(Messages::MessageType)
                            .string_len(50)
                            .not_null()
                            .default("chat"),
                    )
                    .col(
                        ColumnDef::new(Messages::State)
                            .string_len(50)
                            .not_null()
                            .default("context_visible"),
                    )
                    .col(
                        ColumnDef::new(Messages::Parts)
                            .json()
                            .not_null()
                            .default("[]"),
                    )
                    .col(ColumnDef::new(Messages::ToolCalls).json())
                    .col(ColumnDef::new(Messages::ToolCallId).text())
                    .col(ColumnDef::new(Messages::RefMessageId).integer())
                    .col(ColumnDef::new(Messages::Metadata).json())
                    .col(
                        ColumnDef::new(Messages::CreatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Messages::UpdatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Messages::Table, Messages::ChatId)
                            .to(Chats::Table, Chats::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // 8. API Keys
        manager
            .create_table(
                Table::create()
                    .table(ApiKeys::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(ApiKeys::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(ApiKeys::UserId).uuid())
                    .col(ColumnDef::new(ApiKeys::Name).string_len(255))
                    .col(ColumnDef::new(ApiKeys::Prefix).string_len(50))
                    .col(ColumnDef::new(ApiKeys::KeyHash).string_len(255))
                    .col(
                        ColumnDef::new(ApiKeys::CreatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .col(ColumnDef::new(ApiKeys::ExpiresAt).timestamp())
                    .col(ColumnDef::new(ApiKeys::LastUsedAt).timestamp())
                    .foreign_key(
                        ForeignKey::create()
                            .from(ApiKeys::Table, ApiKeys::UserId)
                            .to(Users::Table, Users::Id),
                    )
                    .to_owned(),
            )
            .await?;

        // 9. Models
        manager
            .create_table(
                Table::create()
                    .table(Models::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Models::Id)
                            .string_len(100)
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Models::Provider).string_len(50).not_null())
                    .col(ColumnDef::new(Models::Name).string_len(200))
                    .col(ColumnDef::new(Models::OwnedBy).string_len(100))
                    .col(ColumnDef::new(Models::ContextLength).integer())
                    .col(ColumnDef::new(Models::Capabilities).json().default("{}"))
                    .col(ColumnDef::new(Models::Pricing).json())
                    .col(ColumnDef::new(Models::RawData).json())
                    .col(ColumnDef::new(Models::IsAvailable).boolean().default(true))
                    .col(
                        ColumnDef::new(Models::CreatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Models::UpdatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 10. Model Sync Log
        manager
            .create_table(
                Table::create()
                    .table(ModelSyncLog::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ModelSyncLog::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ModelSyncLog::Provider)
                            .string_len(50)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ModelSyncLog::Status)
                            .string_len(20)
                            .not_null(),
                    )
                    .col(ColumnDef::new(ModelSyncLog::ModelCount).integer())
                    .col(ColumnDef::new(ModelSyncLog::ErrorMessage).text())
                    .col(
                        ColumnDef::new(ModelSyncLog::SyncedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 11. Handles
        manager
            .create_table(
                Table::create()
                    .table(Handles::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Handles::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Handles::OrgId).string().not_null())
                    .col(ColumnDef::new(Handles::Handle).string_len(50).not_null())
                    .col(
                        ColumnDef::new(Handles::TargetType)
                            .string_len(20)
                            .not_null(),
                    )
                    .col(ColumnDef::new(Handles::TargetId).uuid().not_null())
                    .col(
                        ColumnDef::new(Handles::CreatedAt)
                            .timestamp_with_time_zone()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Handles::UpdatedAt)
                            .timestamp_with_time_zone()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // ==========================================
        // Indexes
        // ==========================================

        // -- Users
        manager
            .create_index(
                Index::create()
                    .name("idx_users_external_id")
                    .table(Users::Table)
                    .col(Users::ExternalId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_users_org_id")
                    .table(Users::Table)
                    .col(Users::OrgId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_users_username")
                    .table(Users::Table)
                    .col(Users::Username)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_users_org_id_external_id")
                    .table(Users::Table)
                    .col(Users::OrgId)
                    .col(Users::ExternalId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("uniq_org_external_id")
                    .table(Users::Table)
                    .col(Users::OrgId)
                    .col(Users::ExternalId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // -- Prompts
        manager
            .create_index(
                Index::create()
                    .name("idx_prompts_slug")
                    .table(Prompts::Table)
                    .col(Prompts::Slug)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_prompts_user_id")
                    .table(Prompts::Table)
                    .col(Prompts::UserId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_prompts_org_id_user_id")
                    .table(Prompts::Table)
                    .col(Prompts::OrgId)
                    .col(Prompts::UserId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_prompts_org_id_is_public")
                    .table(Prompts::Table)
                    .col(Prompts::OrgId)
                    .col(Prompts::IsPublic)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_prompts_usage_count")
                    .table(Prompts::Table)
                    .col((Prompts::UsageCount, IndexOrder::Desc))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_prompts_created_at")
                    .table(Prompts::Table)
                    .col((Prompts::CreatedAt, IndexOrder::Desc))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("uniq_org_user_slug")
                    .table(Prompts::Table)
                    .col(Prompts::OrgId)
                    .col(Prompts::UserId)
                    .col(Prompts::Slug)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // -- Agents
        manager
            .create_index(
                Index::create()
                    .name("idx_agents_slug")
                    .table(Agents::Table)
                    .col(Agents::Slug)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_agents_user_id")
                    .table(Agents::Table)
                    .col(Agents::UserId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_agents_org_id_user_id")
                    .table(Agents::Table)
                    .col(Agents::OrgId)
                    .col(Agents::UserId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_agents_org_id_is_public")
                    .table(Agents::Table)
                    .col(Agents::OrgId)
                    .col(Agents::IsPublic)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_agents_created_at")
                    .table(Agents::Table)
                    .col((Agents::CreatedAt, IndexOrder::Desc))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_agents_username")
                    .table(Agents::Table)
                    .col(Agents::OrgId)
                    .col(Agents::Username)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("uniq_org_slug")
                    .table(Agents::Table)
                    .col(Agents::OrgId)
                    .col(Agents::Slug)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // -- Workflows
        manager
            .create_index(
                Index::create()
                    .name("idx_workflows_user_id")
                    .table(Workflows::Table)
                    .col(Workflows::UserId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_workflows_org_id_user_id")
                    .table(Workflows::Table)
                    .col(Workflows::OrgId)
                    .col(Workflows::UserId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_workflows_org_id_is_public")
                    .table(Workflows::Table)
                    .col(Workflows::OrgId)
                    .col(Workflows::IsPublic)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_workflows_created_at")
                    .table(Workflows::Table)
                    .col((Workflows::CreatedAt, IndexOrder::Desc))
                    .to_owned(),
            )
            .await?;

        // -- Chats
        manager
            .create_index(
                Index::create()
                    .name("idx_chats_user_id")
                    .table(Chats::Table)
                    .col(Chats::UserId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_chats_updated_at")
                    .table(Chats::Table)
                    .col((Chats::UpdatedAt, IndexOrder::Desc))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_chats_org_user_agent")
                    .table(Chats::Table)
                    .col(Chats::OrgId)
                    .col(Chats::UserId)
                    .col(Chats::AgentId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_chats_org_external_id")
                    .table(Chats::Table)
                    .col(Chats::OrgId)
                    .col(Chats::ExternalId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // -- Messages
        manager
            .create_index(
                Index::create()
                    .name("idx_messages_chat_id")
                    .table(Messages::Table)
                    .col(Messages::ChatId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_messages_chat_message_id")
                    .table(Messages::Table)
                    .col(Messages::ChatId)
                    .col(Messages::MessageId)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_messages_chat_state")
                    .table(Messages::Table)
                    .col(Messages::ChatId)
                    .col(Messages::State)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_messages_message_type")
                    .table(Messages::Table)
                    .col(Messages::MessageType)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_messages_created_at")
                    .table(Messages::Table)
                    .col(Messages::CreatedAt)
                    .to_owned(),
            )
            .await?;

        // -- Models
        manager
            .create_index(
                Index::create()
                    .name("idx_models_provider")
                    .table(Models::Table)
                    .col(Models::Provider)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_models_is_available")
                    .table(Models::Table)
                    .col(Models::IsAvailable)
                    .to_owned(),
            )
            .await?;

        // -- Model Sync Log
        manager
            .create_index(
                Index::create()
                    .name("idx_model_sync_log_provider")
                    .table(ModelSyncLog::Table)
                    .col(ModelSyncLog::Provider)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_model_sync_log_synced_at")
                    .table(ModelSyncLog::Table)
                    .col((ModelSyncLog::SyncedAt, IndexOrder::Desc))
                    .to_owned(),
            )
            .await?;

        // -- Handles
        manager
            .create_index(
                Index::create()
                    .name("idx_handles_org_handle")
                    .table(Handles::Table)
                    .col(Handles::OrgId)
                    .col(Handles::Handle)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_handles_target")
                    .table(Handles::Table)
                    .col(Handles::TargetType)
                    .col(Handles::TargetId)
                    .to_owned(),
            )
            .await?;

        // ==========================================
        // CHECK Constraints (supported by PG, MySQL 8.0.16+, SQLite)
        // ==========================================
        let db = manager.get_connection();
        db.execute_unprepared(
            "ALTER TABLE messages ADD CONSTRAINT valid_role CHECK (role IN ('user','assistant','tool','system'))"
        ).await?;
        db.execute_unprepared(
            "ALTER TABLE messages ADD CONSTRAINT valid_message_type CHECK (message_type IN ('chat','command','command_result','tool_call','tool_result','system'))"
        ).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop FK first to avoid dependency issues
        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .name("fk_agents_workflow_id")
                    .table(Agents::Table)
                    .to_owned(),
            )
            .await?;

        // Drop tables in reverse dependency order
        let tables = [
            Handles::Table.into_table_ref(),
            ModelSyncLog::Table.into_table_ref(),
            Models::Table.into_table_ref(),
            ApiKeys::Table.into_table_ref(),
            Messages::Table.into_table_ref(),
            Chats::Table.into_table_ref(),
            Workflows::Table.into_table_ref(),
            Agents::Table.into_table_ref(),
            Prompts::Table.into_table_ref(),
            Users::Table.into_table_ref(),
            Organizations::Table.into_table_ref(),
        ];

        for table in tables {
            manager
                .drop_table(Table::drop().table(table).if_exists().to_owned())
                .await?;
        }

        Ok(())
    }
}

// ==========================================
// Table / Column Identifiers
// ==========================================

#[derive(DeriveIden)]
enum Organizations {
    Table,
    Id,
    Name,
    ApiKeyHash,
    PublicKey,
    KeyAlgorithm,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
    OrgId,
    ExternalId,
    Email,
    PasswordHash,
    Name,
    Username,
    Role,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Prompts {
    Table,
    Id,
    Slug,
    OrgId,
    UserId,
    Name,
    Content,
    InputVariables,
    #[sea_orm(iden = "type")]
    PromptType,
    Tags,
    AllowedAgentSlugs,
    IsPublic,
    IsSystem,
    UsageCount,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Agents {
    Table,
    Id,
    Slug,
    OrgId,
    UserId,
    Name,
    Description,
    SystemPromptId,
    AllowedModels,
    DefaultModel,
    Temperature,
    McpConfig,
    Skills,
    ForkFromId,
    WorkflowId,
    IsPublic,
    Username,
    Avatar,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Workflows {
    Table,
    Id,
    Slug,
    OrgId,
    UserId,
    Name,
    Description,
    EndpointUrl,
    TriggerConfig,
    Definition,
    IsActive,
    IsPublic,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Chats {
    Table,
    Id,
    OrgId,
    UserId,
    AgentId,
    ExternalId,
    Title,
    Model,
    LastMessageId,
    Metadata,
    Incognito,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Messages {
    Table,
    Id,
    ChatId,
    MessageId,
    Role,
    MessageType,
    State,
    Parts,
    ToolCalls,
    ToolCallId,
    RefMessageId,
    Metadata,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum ApiKeys {
    Table,
    Id,
    UserId,
    Name,
    Prefix,
    KeyHash,
    CreatedAt,
    ExpiresAt,
    LastUsedAt,
}

#[derive(DeriveIden)]
enum Models {
    Table,
    Id,
    Provider,
    Name,
    OwnedBy,
    ContextLength,
    Capabilities,
    Pricing,
    RawData,
    IsAvailable,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum ModelSyncLog {
    Table,
    Id,
    Provider,
    Status,
    ModelCount,
    ErrorMessage,
    SyncedAt,
}

#[derive(DeriveIden)]
enum Handles {
    Table,
    Id,
    OrgId,
    Handle,
    TargetType,
    TargetId,
    CreatedAt,
    UpdatedAt,
}
