// jug0/src/entities/messages.rs
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "messages")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,

    pub chat_id: Uuid,

    /// chat 内唯一消息标识（1, 2, 3...）
    pub message_id: i32,

    /// 角色：user, assistant, tool, system
    pub role: String,

    /// 消息类型：chat, command, command_result, tool_call, tool_result, system
    pub message_type: String,

    /// 消息状态：context_visible | context_hidden | display_only | silent
    pub state: String,

    /// 消息内容片段
    #[sea_orm(column_type = "JsonBinary")]
    pub parts: serde_json::Value,

    /// AI 发起的工具调用列表
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub tool_calls: Option<serde_json::Value>,

    /// 工具结果对应的调用 ID
    #[sea_orm(column_type = "Text", nullable)]
    pub tool_call_id: Option<String>,

    /// 引用的消息 message_id（如 command_result 引用 command）
    pub ref_message_id: Option<i32>,

    /// 扩展元数据
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub metadata: Option<serde_json::Value>,

    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::chats::Entity",
        from = "Column::ChatId",
        to = "super::chats::Column::Id"
    )]
    Chat,
}

impl Related<super::chats::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Chat.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

/// 消息类型常量
pub mod message_types {
    pub const CHAT: &str = "chat";
    pub const COMMAND: &str = "command";
    pub const COMMAND_RESULT: &str = "command_result";
    pub const TOOL_CALL: &str = "tool_call";
    pub const TOOL_RESULT: &str = "tool_result";
    pub const SYSTEM: &str = "system";
}

/// 消息状态常量
pub mod states {
    pub const CONTEXT_VISIBLE: &str = "context_visible";
    pub const CONTEXT_HIDDEN: &str = "context_hidden";
    pub const DISPLAY_ONLY: &str = "display_only";
    pub const SILENT: &str = "silent";
}

/// 角色常量
pub mod roles {
    pub const USER: &str = "user";
    pub const ASSISTANT: &str = "assistant";
    pub const TOOL: &str = "tool";
    pub const SYSTEM: &str = "system";
}
