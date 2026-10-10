//! One conversation as the program holds it: pages of messages from the
//! server, what the poll adds, and — in a direct chat — the transfers the two
//! exchanged, mixed in by time. Same rules as the web's ChatThread.vue.

use crate::api::types::{ChatMessage, ChatMessagePage, MemberRef, Transaction, TransactionList, parse_date};
use chrono::{DateTime, Local};

pub const PAGE: i64 = 50;
pub const TRANSFER_PAGE: i64 = 25;

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Member(MemberRef),
    Group(String),
}

pub enum Item<'a> {
    Message(&'a ChatMessage),
    Transfer(&'a Transaction),
}

impl Item<'_> {
    pub fn at(&self) -> DateTime<Local> {
        match self {
            Item::Message(message) => parse_date(&message.created_at),
            Item::Transfer(transfer) => parse_date(&transfer.balance_date),
        }
    }
}

#[derive(Debug)]
pub struct Thread {
    pub key: String,
    pub target: Target,
    /// Ascending by id, no id twice.
    pub messages: Vec<ChatMessage>,
    pub has_more: bool,
    pub loaded: bool,
    pub loading: bool,
    pub failed: bool,
    /// Unknown until the first message exists.
    pub conversation_id: Option<i64>,
    pub muted: bool,
    pub transfers: Vec<Transaction>,
    pub transfer_pages: i64,
    pub transfers_have_more: bool,
    /// The highest id the server was told is read.
    pub read_up_to: i64,
}

impl Thread {
    pub fn new(key: String, target: Target) -> Self {
        Self {
            key,
            target,
            messages: Vec::new(),
            has_more: false,
            loaded: false,
            loading: false,
            failed: false,
            conversation_id: None,
            muted: false,
            transfers: Vec::new(),
            transfer_pages: 0,
            transfers_have_more: false,
            read_up_to: 0,
        }
    }

    pub fn is_group(&self) -> bool {
        matches!(self.target, Target::Group(_))
    }

    pub fn oldest_id(&self) -> Option<i64> {
        self.messages.first().map(|m| m.id)
    }

    pub fn newest_id(&self) -> Option<i64> {
        self.messages.last().map(|m| m.id)
    }

    /// The newest message somebody else wrote: what there is to mark as read.
    pub fn newest_foreign_id(&self) -> Option<i64> {
        self.messages.iter().rev().find(|m| !m.mine).map(|m| m.id)
    }

    fn insert(&mut self, message: ChatMessage) -> bool {
        match self.messages.binary_search_by_key(&message.id, |m| m.id) {
            Ok(_) => false,
            Err(at) => {
                self.conversation_id.get_or_insert(message.conversation_id);
                self.messages.insert(at, message);
                true
            }
        }
    }

    /// A page asked from the server, the first one or an older one.
    pub fn add_page(&mut self, page: ChatMessagePage) {
        self.has_more = page.has_more;
        self.muted = page.muted_by_me;
        self.loaded = true;
        self.loading = false;
        self.failed = false;
        for message in page.messages {
            self.insert(message);
        }
    }

    fn belongs(&self, message: &ChatMessage, own_community: &str) -> bool {
        match &self.target {
            Target::Group(uuid) => message.group_uuid.as_deref() == Some(uuid),
            Target::Member(member) => {
                if message.group_uuid.is_some() {
                    return false;
                }
                match self.conversation_id {
                    Some(id) => message.conversation_id == id,
                    // A thread that was empty learns its conversation from the
                    // first message the other one writes.
                    None => !message.mine && message.sender.key(own_community) == member.key(own_community),
                }
            }
        }
    }

    /// What the poll brought. Returns how many are new here.
    pub fn arrivals(&mut self, messages: &[ChatMessage], own_community: &str) -> usize {
        if !self.loaded {
            return 0;
        }
        let oldest = self.oldest_id();
        let mut added = 0;
        for message in messages {
            if !self.belongs(message, own_community) {
                continue;
            }
            // Below the oldest page held there are pages not loaded yet:
            // the message will come with its own page.
            if self.has_more && oldest.is_some_and(|oldest| message.id < oldest) {
                continue;
            }
            if self.insert(message.clone()) {
                added += 1;
            }
        }
        added
    }

    /// The server repeats an edit for a few polls: an older state never wins.
    pub fn edits(&mut self, edited: &[ChatMessage]) -> bool {
        let mut changed = false;
        for fresh in edited {
            if let Some(held) = self.messages.iter_mut().find(|m| m.id == fresh.id)
                && (fresh.edited_at > held.edited_at || (fresh.edited_at == held.edited_at && fresh.delivery_state != held.delivery_state))
            {
                *held = fresh.clone();
                changed = true;
            }
        }
        changed
    }

    /// The server's copy of what was just sent, or of an edit.
    pub fn put(&mut self, message: ChatMessage) {
        self.loaded = true;
        if let Some(held) = self.messages.iter_mut().find(|m| m.id == message.id) {
            *held = message;
        } else {
            self.insert(message);
        }
    }

    pub fn add_transfers(&mut self, list: TransactionList) {
        self.transfer_pages += 1;
        for transfer in list.transactions {
            if matches!(transfer.type_id.as_str(), "SEND" | "RECEIVE") && !self.transfers.iter().any(|t| t.id == transfer.id) {
                self.transfers.push(transfer);
            }
        }
        self.transfers.sort_by_key(|t| parse_date(&t.balance_date));
        self.transfers_have_more = self.transfer_pages * TRANSFER_PAGE < list.balance.count;
    }

    fn oldest_message_at(&self) -> Option<DateTime<Local>> {
        self.messages.first().map(|m| parse_date(&m.created_at))
    }

    fn oldest_transfer_at(&self) -> Option<DateTime<Local>> {
        self.transfers.first().map(|t| parse_date(&t.balance_date))
    }

    /// Nothing older than this is shown: a source that still has older pages
    /// would otherwise leave a hole in the middle of the other one.
    fn horizon(&self) -> Option<DateTime<Local>> {
        let messages = if self.has_more { self.oldest_message_at() } else { None };
        let transfers = if self.transfers_have_more { self.oldest_transfer_at() } else { None };
        messages.max(transfers)
    }

    pub fn can_load_older(&self) -> bool {
        self.has_more || self.transfers_have_more
    }

    /// "Load older" asks the source whose oldest item is the newer one.
    pub fn older_is_transfers(&self) -> bool {
        match (self.has_more, self.transfers_have_more) {
            (true, true) => self.oldest_transfer_at() > self.oldest_message_at(),
            (false, true) => true,
            _ => false,
        }
    }

    pub fn timeline(&self) -> Vec<Item<'_>> {
        let horizon = self.horizon();
        let mut items: Vec<Item> = self.messages.iter().map(Item::Message).collect();
        items.extend(self.transfers.iter().map(Item::Transfer));
        items.retain(|item| horizon.is_none_or(|horizon| item.at() >= horizon));
        // Stable: messages of the same instant keep their id order.
        items.sort_by_key(Item::at);
        items
    }

    /// Whether the member ever wrote here or sent a transfer: decides the
    /// "first message goes by e-mail" banner and the quick answers.
    pub fn i_wrote(&self) -> bool {
        self.messages.iter().any(|m| m.mine) || self.transfers.iter().any(|t| t.type_id == "SEND")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: i64, conversation: i64, mine: bool, sender: &str) -> ChatMessage {
        serde_json::from_value(serde_json::json!({
            "id": id, "messageUuid": format!("u{id}"), "conversationId": conversation, "groupUuid": null,
            "sender": { "communityUuid": "c1", "gradidoID": sender }, "senderUser": null,
            "announcement": false, "mine": mine, "subject": null, "body": format!("m{id}"),
            "createdAt": format!("2026-01-01T10:{:02}:00.000Z", id % 60), "editedAt": null,
            "deliveryState": null, "notify": null, "mailState": null, "images": [],
            "forwarded": false, "forwardedFrom": null, "replyTo": null
        }))
        .unwrap()
    }

    fn direct() -> Thread {
        let member = MemberRef {
            gradido_id: "bob".into(),
            community_uuid: None,
        };
        Thread::new(member.key("c1"), Target::Member(member))
    }

    fn page(messages: Vec<ChatMessage>, has_more: bool) -> ChatMessagePage {
        ChatMessagePage {
            messages,
            has_more,
            muted_by_me: false,
        }
    }

    #[test]
    fn an_empty_thread_learns_its_conversation_from_the_other_side() {
        let mut thread = direct();
        thread.add_page(page(vec![], false));
        assert_eq!(thread.arrivals(&[message(7, 3, false, "eve"), message(8, 4, false, "bob")], "c1"), 1);
        assert_eq!(thread.conversation_id, Some(4));
        assert_eq!(thread.arrivals(&[message(9, 4, true, "me"), message(10, 3, true, "me")], "c1"), 1);
    }

    #[test]
    fn nothing_twice_and_nothing_below_an_unloaded_page() {
        let mut thread = direct();
        thread.add_page(page(vec![message(20, 4, false, "bob"), message(21, 4, true, "me")], true));
        assert_eq!(
            thread.arrivals(&[message(21, 4, true, "me"), message(5, 4, false, "bob"), message(22, 4, false, "bob")], "c1"),
            1
        );
        assert_eq!(thread.messages.iter().map(|m| m.id).collect::<Vec<_>>(), vec![20, 21, 22]);
    }

    #[test]
    fn an_older_edit_never_wins() {
        let mut thread = direct();
        thread.add_page(page(vec![message(1, 4, true, "me")], false));
        let mut newer = message(1, 4, true, "me");
        newer.body = "new".into();
        newer.edited_at = Some("2026-01-01T11:00:00.000Z".into());
        let mut older = newer.clone();
        older.body = "old".into();
        older.edited_at = Some("2026-01-01T10:30:00.000Z".into());
        assert!(thread.edits(&[newer.clone()]));
        assert!(!thread.edits(&[older, newer]));
        assert_eq!(thread.messages[0].body, "new");
    }

    #[test]
    fn a_group_message_never_lands_in_a_direct_thread() {
        let mut thread = direct();
        thread.add_page(page(vec![message(1, 4, false, "bob")], false));
        let mut grouped = message(2, 4, false, "bob");
        grouped.group_uuid = Some("g".into());
        assert_eq!(thread.arrivals(&[grouped], "c1"), 0);
    }
}
