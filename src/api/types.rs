//! What the server answers, field for field as the web chat asks it.

use serde::{Deserialize, Serialize};

/// A member is always this pair; `community_uuid: None` is the own community.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct MemberRef {
    #[serde(rename = "gradidoID")]
    pub gradido_id: String,
    #[serde(rename = "communityUuid")]
    pub community_uuid: Option<String>,
}

impl MemberRef {
    /// One key per person, whatever case the ids came in.
    pub fn key(&self, own_community: &str) -> String {
        format!("{}/{}", self.community_uuid.as_deref().unwrap_or(own_community), self.gradido_id).to_lowercase()
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct User {
    #[serde(rename = "gradidoID")]
    pub gradido_id: String,
    pub community_uuid: Option<String>,
    pub community_name: Option<String>,
    pub alias: Option<String>,
    pub avatar_color_index: Option<i64>,
    pub avatar_updated_at: Option<String>,
}

impl User {
    pub fn member(&self) -> MemberRef {
        MemberRef {
            gradido_id: self.gradido_id.clone(),
            community_uuid: self.community_uuid.clone(),
        }
    }

    pub fn name(&self) -> &str {
        crate::core::avatar::member_name(self.alias.as_deref(), &self.gradido_id)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    #[serde(rename = "gradidoID")]
    pub gradido_id: String,
    pub community_uuid: String,
    pub alias: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub language: Option<String>,
    pub transfers_in_chat: Option<bool>,
    pub avatar_color_index: Option<i64>,
    /// The own small picture, base64 JPEG.
    pub avatar: Option<String>,
}

impl Me {
    pub fn name(&self) -> String {
        match &self.alias {
            Some(alias) if !alias.is_empty() => alias.clone(),
            _ => format!("{} {}", self.first_name.as_deref().unwrap_or(""), self.last_name.as_deref().unwrap_or(""))
                .trim()
                .to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatImageMeta {
    pub image_uuid: String,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quote {
    pub id: i64,
    pub mine: bool,
    pub excerpt: String,
    pub has_image: bool,
    pub sender_user: Option<User>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    /// The server's arrival order: the only ordering and the paging cursor.
    pub id: i64,
    pub message_uuid: String,
    pub conversation_id: i64,
    pub group_uuid: Option<String>,
    pub sender: MemberRef,
    pub sender_user: Option<User>,
    pub announcement: bool,
    pub mine: bool,
    pub subject: Option<String>,
    pub body: String,
    pub created_at: String,
    pub edited_at: Option<String>,
    /// DELIVERED | PENDING | FAILED, on own messages only.
    pub delivery_state: Option<String>,
    /// EMAIL | NONE, on own messages only.
    pub notify: Option<String>,
    /// MAILED | MUTED, on own messages only.
    pub mail_state: Option<String>,
    #[serde(default)]
    pub images: Vec<ChatImageMeta>,
    pub forwarded: bool,
    pub forwarded_from: Option<User>,
    pub reply_to: Option<Quote>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessagePage {
    pub messages: Vec<ChatMessage>,
    pub has_more: bool,
    pub muted_by_me: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatUpdate {
    pub latest_id: i64,
    pub unread_conversations: i32,
    pub has_more: bool,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub edited: Option<Vec<ChatMessage>>,
    pub edited_cursor: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub user: User,
    pub first_at: String,
    pub last_at: String,
    pub bookings: i64,
    pub favorite: bool,
    pub home_community: bool,
    /// REFERRER | ARRIVAL
    pub origin: Option<String>,
    pub unread_chat_messages: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContactList {
    pub count: i64,
    pub contacts: Vec<Contact>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGroup {
    pub group_uuid: String,
    pub conversation_id: i64,
    pub title: String,
    pub community_name: Option<String>,
    pub created_by: Option<User>,
    pub created_at: String,
    /// OWNER | MODERATOR | MEMBER
    pub role: String,
    pub joined_at: String,
    pub muted_by_me: bool,
    pub member_count: i64,
    pub unread_messages: i64,
    pub last_message_at: Option<String>,
}

impl ChatGroup {
    pub fn may_manage(&self) -> bool {
        self.role == "OWNER" || self.role == "MODERATOR"
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGroupMember {
    pub user: User,
    pub role: String,
    pub joined_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberAvatar {
    #[serde(rename = "gradidoID")]
    pub gradido_id: String,
    pub community_uuid: Option<String>,
    /// base64 JPEG, 128 px
    pub avatar: String,
    pub avatar_updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Greeting {
    pub motif: Option<String>,
    pub line: Option<String>,
    pub has_picture: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferPicture {
    pub motif: Option<String>,
    pub has_picture: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    pub id: i64,
    /// SEND | RECEIVE are the ones a thread shows.
    pub type_id: String,
    /// A decimal; the server writes it as a string.
    pub amount: serde_json::Value,
    pub balance_date: String,
    pub memo: Option<String>,
    pub link_id: Option<i64>,
    pub greeting: Option<Greeting>,
    pub picture: Option<TransferPicture>,
}

impl Transaction {
    pub fn amount(&self) -> f64 {
        match &self.amount {
            serde_json::Value::String(text) => text.parse().unwrap_or(0.0),
            other => other.as_f64().unwrap_or(0.0),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Balance {
    /// With a counterparty: how many bookings the two share.
    pub count: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransactionList {
    pub balance: Balance,
    pub transactions: Vec<Transaction>,
}

pub fn parse_date(text: &str) -> chrono::DateTime<chrono::Local> {
    chrono::DateTime::parse_from_rfc3339(text)
        .map(|at| at.with_timezone(&chrono::Local))
        .unwrap_or_default()
}
