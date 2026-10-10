//! The open conversations: tabs, what the active one shows, writing,
//! answering, editing, searching.

use super::lists::{Face, decode_picture};
use super::{App, Picture, with_app};
use crate::api::types::{ChatMessage, ChatMessagePage, MemberRef, Transaction, TransactionList, User, parse_date};
use crate::api::{ApiError, ApiResult, documents};
use crate::core::search;
use crate::core::text;
use crate::core::thread::{Item, PAGE, TRANSFER_PAGE, Target, Thread};
use crate::{Bubble, ChatWindow, FileCard, TabInfo};
use serde_json::json;
use slint::{ComponentHandle, ModelRc, SharedString, StyledText, VecModel};
use std::time::Duration;

pub const MESSAGE_MAX_CHARS: usize = 2000;
/// Below this many left, the counter shows.
const REMAINING_SHOWN_BELOW: usize = 200;
/// A quoted message is looked for this many pages back.
const QUOTE_PAGES_MAX: u8 = 20;
const MARK_COLOR: &str = "#d97706";

pub enum Scroll {
    Stay,
    Bottom,
    /// Older messages came in above: what is on screen stays where it is.
    Held,
    Jump(i64),
}

pub struct Tab {
    pub thread: Thread,
    /// Member conversations: who it is with. Not everybody is a contact yet.
    pub who: Option<User>,
    pub draft: String,
    /// What was being written when an edit began.
    pub parked: Option<String>,
    pub reply_to: Option<i64>,
    pub editing: Option<i64>,
    pub check: bool,
    pub error: String,
    pub sending: bool,
    pub unread: bool,
    pub loading_older: bool,
    /// A quoted message not loaded yet, and how many pages may still be fetched for it.
    pub seek: Option<(i64, u8)>,
    pub search_open: bool,
    pub search_raw: String,
    pub needle: String,
    pub hits: Vec<i64>,
    pub hit: usize,
    pub ring: Option<i64>,
}

impl Tab {
    fn new(thread: Thread, who: Option<User>) -> Self {
        Self {
            thread,
            who,
            draft: String::new(),
            parked: None,
            reply_to: None,
            editing: None,
            check: false,
            error: String::new(),
            sending: false,
            unread: false,
            loading_older: false,
            seek: None,
            search_open: false,
            search_raw: String::new(),
            needle: String::new(),
            hits: Vec::new(),
            hit: 0,
            ring: None,
        }
    }

    fn find_hits(&mut self) {
        let needle = &self.needle;
        self.hits = if needle.is_empty() {
            Vec::new()
        } else {
            self.thread
                .messages
                .iter()
                .filter(|m| search::matches(&m.body, needle) || m.subject.as_deref().is_some_and(|s| search::matches(s, needle)))
                .map(|m| m.id)
                .collect()
        };
    }
}

pub fn group_key(uuid: &str) -> String {
    format!("group/{uuid}")
}

fn styled(markdown: &str) -> StyledText {
    if markdown.is_empty() {
        // An empty line still takes a line's height.
        return StyledText::from_plain_text("\u{a0}");
    }
    StyledText::from_markdown(markdown).unwrap_or_else(|_| StyledText::from_plain_text(markdown))
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl App {
    pub(crate) fn tab(&self, key: &str) -> Option<&Tab> {
        self.tabs.iter().find(|tab| tab.thread.key == key)
    }

    pub(crate) fn tab_mut(&mut self, key: &str) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|tab| tab.thread.key == key)
    }

    pub(crate) fn active_tab(&self) -> Option<&Tab> {
        self.active.as_deref().and_then(|key| self.tab(key))
    }

    pub(crate) fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        let key = self.active.clone()?;
        self.tab_mut(&key)
    }

    /// The conversation the member is looking at right now, if any: the
    /// window is there and has the keyboard.
    pub(crate) fn watched_key(&self) -> Option<String> {
        if self.chat.window().is_visible() && self.chat.get_has_keyboard() {
            self.active.clone()
        } else {
            None
        }
    }

    /// Which conversation a message from the poll belongs to.
    pub(crate) fn conversation_key(&self, message: &ChatMessage) -> Option<String> {
        if let Some(uuid) = &message.group_uuid {
            return Some(group_key(uuid));
        }
        if message.mine {
            return self
                .tabs
                .iter()
                .find(|t| t.thread.conversation_id == Some(message.conversation_id))
                .map(|t| t.thread.key.clone());
        }
        Some(message.sender.key(self.own_community()))
    }

    // ------------------------------------------------------------------ tabs

    pub(crate) fn open_by_key(&mut self, key: &str) {
        if let Some(uuid) = key.strip_prefix("group/") {
            let thread = Thread::new(key.to_string(), Target::Group(uuid.to_string()));
            return self.open_tab(thread, None);
        }
        let own = self.own_community().to_string();
        if let Some(user) = self.contacts.iter().find(|c| c.user.member().key(&own) == key).map(|c| c.user.clone()) {
            self.open_member(user);
        }
    }

    pub(crate) fn open_member(&mut self, user: User) {
        let key = self.user_key(&user);
        // Writing to oneself is not a conversation.
        if self.me.as_ref().is_some_and(|me| me.gradido_id.eq_ignore_ascii_case(&user.gradido_id)) {
            return;
        }
        self.want_avatars(std::slice::from_ref(&user));
        let thread = Thread::new(key, Target::Member(user.member()));
        self.open_tab(thread, Some(user));
    }

    fn open_tab(&mut self, thread: Thread, who: Option<User>) {
        let key = thread.key.clone();
        if self.tab(&key).is_none() {
            self.tabs.push(Tab::new(thread, who));
            self.load_page(&key);
            self.load_transfers(&key);
        }
        self.activate(&key);
        let _ = self.chat.show();
        self.chat.invoke_focus_compose();
    }

    /// What the window holds of the tab being left goes back into the tab.
    fn keep_active(&mut self) {
        let draft = self.chat.get_draft().to_string();
        let check = self.chat.get_check();
        let search_open = self.chat.get_search_open();
        if let Some(tab) = self.active_tab_mut() {
            tab.draft = draft;
            tab.check = check;
            tab.search_open = search_open;
        }
    }

    pub(crate) fn activate(&mut self, key: &str) {
        // Also when it is the active one already: what stands in the window is the newest.
        self.keep_active();
        self.active = Some(key.to_string());
        let Some(tab) = self.active_tab() else { return };
        let (draft, check, search_open, search_raw) = (tab.draft.clone(), tab.check, tab.search_open, tab.search_raw.clone());
        self.chat.set_draft(draft.into());
        self.chat.set_check(check);
        self.chat.set_search_open(search_open);
        self.chat.set_search_text(search_raw.into());
        self.show_tabs();
        self.show_head();
        self.show_compose();
        self.show_search();
        self.show_thread(Scroll::Bottom);
        self.mark_read_if_watched();
    }

    pub(crate) fn close_tab(&mut self, key: &str) {
        let Some(at) = self.tabs.iter().position(|tab| tab.thread.key == key) else {
            return;
        };
        let was_active = self.active.as_deref() == Some(key);
        if was_active {
            // Its draft goes with it.
            self.active = None;
        }
        self.tabs.remove(at);
        if self.tabs.is_empty() {
            self.chat.set_draft(SharedString::new());
            self.show_tabs();
            let _ = self.chat.hide();
        } else if was_active {
            let next = self.tabs[at.min(self.tabs.len() - 1)].thread.key.clone();
            self.activate(&next);
        } else {
            self.show_tabs();
        }
    }

    fn tab_face(&self, tab: &Tab) -> (String, Face) {
        match (&tab.thread.target, &tab.who) {
            (Target::Group(uuid), _) => {
                let title = self.groups.iter().find(|g| &g.group_uuid == uuid).map_or(String::new(), |g| g.title.clone());
                let face = self.group_face(&title);
                (title, face)
            }
            (Target::Member(_), Some(user)) => (user.name().to_string(), self.member_face(user)),
            (Target::Member(member), None) => (member.gradido_id.clone(), self.group_face("?")),
        }
    }

    pub(crate) fn show_tabs(&mut self) {
        let tabs: Vec<TabInfo> = self
            .tabs
            .iter()
            .map(|tab| {
                let (title, face) = self.tab_face(tab);
                TabInfo {
                    key: tab.thread.key.as_str().into(),
                    title: title.into(),
                    letters: face.letters.into(),
                    color: face.color,
                    has_avatar: face.avatar.is_some(),
                    avatar: face.avatar.unwrap_or_default(),
                    group: tab.thread.is_group(),
                    unread: tab.unread,
                }
            })
            .collect();
        self.chat.set_tabs(ModelRc::new(VecModel::from(tabs)));
        self.chat.set_active_key(self.active.clone().unwrap_or_default().into());
    }

    pub(crate) fn show_head(&mut self) {
        let Some(tab) = self.active_tab() else { return };
        let i18n = self.i18n.borrow();
        let (title, face) = self.tab_face(tab);
        let own = self.own_community();
        let (mut sub, mut meta) = (String::new(), String::new());
        let (mut favorite, mut muted, mut can_mute) = (false, tab.thread.muted, tab.thread.conversation_id.is_some());
        match &tab.thread.target {
            Target::Group(uuid) => {
                if let Some(group) = self.groups.iter().find(|g| &g.group_uuid == uuid) {
                    let role = match group.role.as_str() {
                        "OWNER" => "chatGroup.youAreOwner",
                        "MODERATOR" => "chatGroup.youAreModerator",
                        _ => "chatGroup.youAreMember",
                    };
                    sub = format!("{} · {}", i18n.plural("chatGroup.memberCount", group.member_count, &[]), i18n.t(role));
                    let opened = match &group.created_by {
                        Some(user) if self.me.as_ref().is_some_and(|me| me.gradido_id == user.gradido_id) => i18n.t("chatGroup.openedByYou"),
                        Some(user) => i18n.with("chatGroup.openedBy", &[("name", user.name())]),
                        None => String::new(),
                    };
                    meta = i18n.with("chatGroup.since", &[("date", &i18n.month_and_year(parse_date(&group.created_at)))]);
                    if !opened.is_empty() {
                        meta = format!("{meta} · {opened}");
                    }
                    muted = group.muted_by_me;
                    can_mute = true;
                }
            }
            Target::Member(member) => {
                sub = tab.who.as_ref().and_then(|u| u.community_name.clone()).unwrap_or_default();
                if let Some(contact) = self.contacts.iter().find(|c| c.user.member().key(own) == member.key(own)) {
                    favorite = contact.favorite;
                    meta = format!(
                        "{} · {}",
                        i18n.with("contacts.since", &[("date", &i18n.month_and_year(parse_date(&contact.first_at)))]),
                        i18n.plural("contacts.bookings", contact.bookings, &[])
                    );
                }
            }
        }
        let is_group = tab.thread.is_group();
        drop(i18n);
        self.chat.set_head_title(title.into());
        self.chat.set_head_sub(sub.into());
        self.chat.set_head_meta(meta.into());
        self.chat.set_head_letters(face.letters.into());
        self.chat.set_head_color(face.color);
        self.chat.set_head_has_avatar(face.avatar.is_some());
        self.chat.set_head_avatar(face.avatar.unwrap_or_default());
        self.chat.set_is_group(is_group);
        self.chat.set_favorite(favorite);
        self.chat.set_muted(muted);
        self.chat.set_can_mute(can_mute);
        self.chat.set_can_send_gradido(!is_group);
    }

    // --------------------------------------------------------------- loading

    /// The first page, or the next older one.
    fn load_page(&mut self, key: &str) {
        let Some(tab) = self.tab_mut(key) else { return };
        if tab.thread.loading {
            return;
        }
        tab.thread.loading = true;
        let before = tab.thread.oldest_id();
        let target = tab.thread.target.clone();
        let key = key.to_string();
        self.ask(
            move |client| -> ApiResult<ChatMessagePage> {
                match target {
                    Target::Member(member) => client.call(
                        documents::MESSAGES_WITH_MEMBER,
                        json!({ "ref": member, "before": before, "limit": PAGE }),
                        "chatMessagesWithMember",
                    ),
                    Target::Group(uuid) => client.call(
                        documents::GROUP_MESSAGES,
                        json!({ "groupUuid": uuid, "before": before, "limit": PAGE }),
                        "chatGroupMessages",
                    ),
                }
            },
            move |app, result| app.page_loaded(&key, before.is_none(), result),
        );
    }

    fn page_loaded(&mut self, key: &str, first: bool, result: ApiResult<ChatMessagePage>) {
        if matches!(result, Err(ApiError::SessionExpired)) {
            return self.session_expired();
        }
        let Some(tab) = self.tab_mut(key) else { return };
        tab.loading_older = false;
        match result {
            Ok(page) => {
                let users: Vec<User> = page.messages.iter().filter_map(|m| m.sender_user.clone()).collect();
                tab.thread.add_page(page);
                tab.find_hits();
                self.want_avatars(&users);
            }
            Err(_) => {
                tab.thread.loading = false;
                tab.thread.failed = !tab.thread.loaded;
                tab.seek = None;
            }
        }
        if self.active.as_deref() != Some(key) {
            return;
        }
        self.show_head();
        self.show_compose();
        self.show_search();
        self.show_thread(if first { Scroll::Bottom } else { Scroll::Held });
        self.mark_read_if_watched();
        self.continue_seeking(key);
    }

    /// After an older page: is the quoted message there now, does the search want more?
    fn continue_seeking(&mut self, key: &str) {
        let Some(tab) = self.tab_mut(key) else { return };
        if let Some((id, pages)) = tab.seek {
            if tab.thread.messages.iter().any(|m| m.id == id) {
                tab.seek = None;
                return self.jump_to(id);
            }
            if pages == 0 || !tab.thread.has_more {
                tab.seek = None;
                let text = self.t("chatThread.quoteNotShown");
                return self.toast(&text);
            }
            tab.seek = Some((id, pages - 1));
            return self.load_page(key);
        }
        if !tab.needle.is_empty() && tab.thread.has_more && tab.thread.messages.len() < search::SEARCH_MAX_ITEMS {
            self.load_page(key);
        }
    }

    fn load_transfers(&mut self, key: &str) {
        if self.me.as_ref().is_some_and(|me| me.transfers_in_chat == Some(false)) {
            return;
        }
        let Some(tab) = self.tab(key) else { return };
        let Target::Member(member) = tab.thread.target.clone() else { return };
        let page = tab.thread.transfer_pages + 1;
        let key = key.to_string();
        self.ask(
            move |client| -> ApiResult<TransactionList> {
                client.call(
                    documents::TRANSFERS,
                    json!({ "currentPage": page, "pageSize": TRANSFER_PAGE, "counterparty": member }),
                    "transactionList",
                )
            },
            move |app, result| {
                let Some(tab) = app.tab_mut(&key) else { return };
                tab.loading_older = false;
                // Without the transfers the conversation is still whole.
                let Ok(list) = result else { return };
                let first = tab.thread.transfer_pages == 0;
                tab.thread.add_transfers(list);
                if app.active.as_deref() == Some(key.as_str()) {
                    app.show_compose();
                    app.show_thread(if first { Scroll::Bottom } else { Scroll::Held });
                }
            },
        );
    }

    fn load_older(&mut self) {
        let Some(tab) = self.active_tab_mut() else { return };
        if tab.loading_older {
            return;
        }
        tab.loading_older = true;
        let key = tab.thread.key.clone();
        let transfers = tab.thread.older_is_transfers();
        self.chat.set_loading_older(true);
        if transfers { self.load_transfers(&key) } else { self.load_page(&key) }
    }

    /// Asks for the pictures the thread shows and does not hold yet.
    fn want_pictures(&mut self, key: &str) {
        let Some(tab) = self.tab(key) else { return };
        let mut wanted: Vec<(String, &'static str, &'static str, serde_json::Value)> = Vec::new();
        for message in &tab.thread.messages {
            if let Some(image) = message.images.first() {
                wanted.push((
                    image.image_uuid.clone(),
                    documents::MESSAGE_IMAGE,
                    "chatMessageImage",
                    json!({ "imageUuid": image.image_uuid }),
                ));
            }
        }
        for transfer in &tab.thread.transfers {
            if let Some(name) = transfer_picture(transfer) {
                match transfer.link_id.filter(|_| transfer.greeting.is_some()) {
                    Some(link) => wanted.push((name, documents::GREETING_PICTURE, "thankYouGreetingPicture", json!({ "linkId": link }))),
                    None => wanted.push((
                        name,
                        documents::TRANSACTION_PICTURE,
                        "transactionPicture",
                        json!({ "transactionId": transfer.id }),
                    )),
                }
            }
        }
        wanted.retain(|(name, ..)| !self.pictures.contains_key(name));
        for (name, document, field, variables) in wanted {
            self.pictures.insert(name.clone(), Picture::Loading);
            let Some(media) = &self.media else { return };
            media.ask(
                move |client| {
                    client
                        .call::<Option<String>>(document, variables, field)
                        .ok()
                        .flatten()
                        .and_then(|data| decode_picture(&data))
                },
                move |app, buffer| {
                    let picture = buffer.map_or(Picture::Missing, |buffer| Picture::There(slint::Image::from_rgba8(buffer)));
                    app.pictures.insert(name, picture);
                    app.show_thread(Scroll::Stay);
                },
            );
        }
    }

    // --------------------------------------------------------------- showing

    fn bubble_for_message(&self, tab: &Tab, message: &ChatMessage, show_writer: bool) -> Bubble {
        let i18n = self.i18n.borrow();
        let mark = if tab.needle.is_empty() { None } else { Some(MARK_COLOR) };
        let rendered = text::render(&message.body, true, mark, &tab.needle);
        let at = parse_date(&message.created_at);
        let other = tab.who.as_ref().map_or("", |u| u.name());

        let mut bubble = Bubble {
            kind: 1,
            id: message.id as i32,
            mine: message.mine,
            announcement: message.announcement,
            subject: message.subject.clone().unwrap_or_default().into(),
            time: i18n.time(at).into(),
            edited: message.edited_at.is_some(),
            // TEMPORARY, see `COMMIT_WITHOUT_REPLIES` in src/api/mod.rs: back to `true`.
            can_reply: self.client.as_ref().is_none_or(|client| client.replies()),
            can_edit: message.mine && !message.forwarded,
            hit: tab.hits.get(tab.hit) == Some(&message.id) && !tab.needle.is_empty(),
            ring: tab.ring == Some(message.id) || tab.reply_to == Some(message.id) || tab.editing == Some(message.id),
            indent: tab.thread.is_group() && !message.mine,
            ..Default::default()
        };
        // An empty body under a picture is no line at all.
        if !(message.body.is_empty() && !message.images.is_empty()) {
            bubble.lines = ModelRc::new(VecModel::from(rendered.lines.iter().map(|line| styled(line)).collect::<Vec<_>>()));
        }
        bubble.files = ModelRc::new(VecModel::from(
            rendered
                .files
                .into_iter()
                .map(|f| FileCard {
                    service: f.service.into(),
                    label: f.label.into(),
                    url: f.url.into(),
                })
                .collect::<Vec<_>>(),
        ));

        if let Some(user) = &message.sender_user {
            let face = self.member_face(user);
            bubble.writer = user.name().into();
            bubble.writer_key = self.user_key(user).into();
            bubble.show_writer = show_writer;
            bubble.letters = face.letters.into();
            bubble.color = face.color;
            bubble.has_avatar = face.avatar.is_some();
            bubble.avatar = face.avatar.unwrap_or_default();
        }
        if message.forwarded {
            bubble.forwarded = match &message.forwarded_from {
                Some(user) => i18n.with("chatThread.forwardedFrom", &[("name", user.name())]),
                None => i18n.t("chatThread.forwarded"),
            }
            .into();
        }
        if let Some(quote) = &message.reply_to {
            // The message itself when it is loaded: an edit shows in the quote too.
            let live = tab.thread.messages.iter().find(|m| m.id == quote.id);
            bubble.has_quote = true;
            bubble.quote_id = quote.id as i32;
            bubble.quote_name = if quote.mine {
                i18n.t("chatThread.you")
            } else {
                quote.sender_user.as_ref().map_or(other.to_string(), |u| u.name().to_string())
            }
            .into();
            bubble.quote_text = one_line(live.map_or(quote.excerpt.as_str(), |m| m.body.as_str())).into();
            bubble.quote_picture = quote.has_image;
        }
        if let Some(image) = message.images.first() {
            bubble.image_ratio = image.height as f32 / image.width.max(1) as f32;
            match self.pictures.get(&image.image_uuid) {
                Some(Picture::There(picture)) => {
                    bubble.image_state = 2;
                    bubble.image = picture.clone();
                }
                Some(Picture::Missing) => bubble.image_state = 3,
                _ => bubble.image_state = 1,
            }
        }
        if message.mine {
            let (delivery, mail) = (message.delivery_state.as_deref(), message.mail_state.as_deref());
            let stuck = matches!(delivery, Some("PENDING" | "FAILED"));
            bubble.mailed = mail == Some("MAILED") || (mail.is_none() && message.notify.as_deref() == Some("EMAIL") && !stuck);
            bubble.note = match (delivery, mail) {
                (Some("PENDING"), _) => i18n.t("chatThread.pending"),
                (Some("FAILED"), _) => i18n.t("chatThread.failed"),
                (_, Some("MUTED")) => i18n.with("chatThread.notMailedMuted", &[("name", other)]),
                _ => String::new(),
            }
            .into();
        }
        bubble
    }

    fn bubble_for_transfer(&self, tab: &Tab, transfer: &Transaction) -> Bubble {
        let i18n = self.i18n.borrow();
        let other = tab.who.as_ref().map_or("", |u| u.name());
        let mine = transfer.type_id == "SEND";
        let amount = i18n.amount(transfer.amount());
        let subject = i18n.with(
            if mine { "chatThread.transferSent" } else { "chatThread.transferReceived" },
            &[("name", other), ("amount", &amount)],
        );
        let mut memo = transfer.memo.clone().unwrap_or_default();
        let mut bubble = Bubble {
            kind: 2,
            id: -(transfer.id as i32),
            mine,
            subject: subject.into(),
            time: i18n.time(parse_date(&transfer.balance_date)).into(),
            ..Default::default()
        };
        // A greeting's line is the first line of the memo: shown once, in its own hand.
        if let Some(line) = transfer.greeting.as_ref().and_then(|g| g.line.clone()).filter(|line| !line.is_empty()) {
            memo = memo
                .strip_prefix(line.as_str())
                .map(|rest| rest.trim_start_matches('\n').to_string())
                .unwrap_or(memo);
            bubble.greeting = line.into();
        }
        if let Some(name) = transfer_picture(transfer) {
            match self.pictures.get(&name) {
                Some(Picture::There(picture)) => {
                    bubble.image_state = 2;
                    bubble.image = picture.clone();
                    bubble.image_ratio = picture.size().height as f32 / picture.size().width.max(1) as f32;
                }
                Some(Picture::Missing) => {}
                _ => {
                    bubble.image_state = 1;
                    bubble.image_ratio = 25.0 / 36.0;
                }
            }
        } else if let Some(motif) = transfer
            .greeting
            .as_ref()
            .and_then(|g| g.motif.clone())
            .or_else(|| transfer.picture.as_ref().and_then(|p| p.motif.clone()))
        {
            bubble.motif = motif.into();
        }
        let rendered = text::render(&memo, false, None, "");
        if !memo.trim().is_empty() {
            bubble.lines = ModelRc::new(VecModel::from(rendered.lines.iter().map(|line| styled(line)).collect::<Vec<_>>()));
        }
        bubble
    }

    /// The active conversation, as the window draws it.
    pub(crate) fn show_thread(&mut self, scroll: Scroll) {
        let Some(key) = self.active.clone() else { return };
        self.want_pictures(&key);
        let Some(tab) = self.tab(&key) else { return };
        let state = if tab.thread.failed {
            1
        } else if !tab.thread.loaded {
            0
        } else if tab.thread.messages.is_empty() && tab.thread.transfers.is_empty() {
            2
        } else {
            3
        };

        let i18n = self.i18n.clone();
        let today = chrono::Local::now().date_naive();
        let mut bubbles = Vec::new();
        let mut day = None;
        let mut last_writer: Option<String> = None;
        for item in tab.thread.timeline() {
            let at = item.at();
            if day != Some(at.date_naive()) {
                day = Some(at.date_naive());
                last_writer = None;
                let i18n = i18n.borrow();
                let label = match (today - at.date_naive()).num_days() {
                    0 => i18n.t("app.today"),
                    1 => i18n.t("app.yesterday"),
                    _ => i18n.date(at),
                };
                bubbles.push(Bubble {
                    kind: 0,
                    day: label.into(),
                    ..Default::default()
                });
            }
            match item {
                Item::Message(message) => {
                    let writer = if message.mine { None } else { Some(message.sender.key(self.own_community())) };
                    let show_writer = tab.thread.is_group() && writer.is_some() && writer != last_writer;
                    last_writer = writer;
                    bubbles.push(self.bubble_for_message(tab, message, show_writer));
                }
                Item::Transfer(transfer) => {
                    last_writer = None;
                    bubbles.push(self.bubble_for_transfer(tab, transfer));
                }
            }
        }
        let (can_load_older, loading_older) = (tab.thread.can_load_older(), tab.loading_older);

        if matches!(scroll, Scroll::Held) {
            self.chat.invoke_hold_position();
        }
        self.chat.set_thread_state(state);
        self.chat.set_can_load_older(can_load_older);
        self.chat.set_loading_older(loading_older);
        self.chat.set_bubbles(ModelRc::new(VecModel::from(bubbles)));
        match scroll {
            Scroll::Stay => {}
            Scroll::Bottom => {
                self.chat.invoke_scroll_to_bottom();
                // Once more when the new bubbles have their sizes.
                slint::Timer::single_shot(Duration::from_millis(40), || with_app(|app| app.chat.invoke_scroll_to_bottom()));
            }
            Scroll::Held => {
                self.chat.invoke_restore_position();
                slint::Timer::single_shot(Duration::from_millis(40), || with_app(|app| app.chat.invoke_restore_position()));
            }
            Scroll::Jump(id) => {
                // The bubble answers a rising `jump-seq` with its place; it has to exist first.
                slint::Timer::single_shot(Duration::from_millis(40), move || {
                    with_app(move |app| {
                        app.jump_seq += 1;
                        app.chat.set_jump_id(id as i32);
                        app.chat.set_jump_seq(app.jump_seq);
                    })
                });
            }
        }
    }

    /// Scrolls to a message and rings it for a moment.
    fn jump_to(&mut self, id: i64) {
        let Some(tab) = self.active_tab_mut() else { return };
        tab.ring = Some(id);
        self.show_thread(Scroll::Jump(id));
        slint::Timer::single_shot(Duration::from_secs(2), move || {
            with_app(move |app| {
                if let Some(tab) = app.active_tab_mut()
                    && tab.ring == Some(id)
                {
                    tab.ring = None;
                    app.show_thread(Scroll::Stay);
                }
            })
        });
    }

    fn quote_clicked(&mut self, id: i64) {
        let Some(tab) = self.active_tab_mut() else { return };
        if tab.thread.messages.iter().any(|m| m.id == id) {
            return self.jump_to(id);
        }
        if !tab.thread.has_more {
            let text = self.t("chatThread.quoteNotShown");
            return self.toast(&text);
        }
        tab.seek = Some((id, QUOTE_PAGES_MAX));
        let key = tab.thread.key.clone();
        self.load_page(&key);
    }

    // ------------------------------------------------------------------ read

    /// Tells the server how far the member has read — only while somebody is looking.
    pub(crate) fn mark_read_if_watched(&mut self) {
        let Some(key) = self.watched_key() else { return };
        let own = self.own_community().to_string();
        let Some(tab) = self.tab_mut(&key) else { return };
        tab.unread = false;
        let Some(up_to) = tab.thread.newest_foreign_id().filter(|id| *id > tab.thread.read_up_to) else {
            return self.show_tabs();
        };
        tab.thread.read_up_to = up_to;
        let target = tab.thread.target.clone();
        // The mark goes at once; the server's count follows with the next lists.
        match &target {
            Target::Member(member) => {
                if let Some(contact) = self.contacts.iter_mut().find(|c| c.user.member().key(&own) == member.key(&own)) {
                    contact.unread_chat_messages = 0;
                }
            }
            Target::Group(uuid) => {
                if let Some(group) = self.groups.iter_mut().find(|g| &g.group_uuid == uuid) {
                    group.unread_messages = 0;
                }
            }
        }
        self.show_tabs();
        self.show_lists();
        self.ask(
            move |client| -> ApiResult<bool> {
                match target {
                    Target::Member(member) => client.call(
                        documents::MARK_CONVERSATION_READ,
                        json!({ "ref": member, "upToMessageId": up_to }),
                        "markChatConversationRead",
                    ),
                    Target::Group(uuid) => client.call(
                        documents::MARK_GROUP_READ,
                        json!({ "groupUuid": uuid, "upToMessageId": up_to }),
                        "markChatGroupRead",
                    ),
                }
            },
            |app, _| app.poll_now(),
        );
    }

    // --------------------------------------------------------------- writing

    pub(crate) fn show_compose(&mut self) {
        let Some(tab) = self.active_tab() else { return };
        let i18n = self.i18n.borrow();
        let (title, _) = self.tab_face(tab);
        let direct = !tab.thread.is_group();
        let settled = tab.thread.loaded && !tab.thread.has_more;
        let first = direct && settled && !tab.thread.messages.iter().any(|m| m.mine);
        let other_wrote = tab.thread.messages.iter().any(|m| !m.mine);

        let (mut strip, mut strip_title, mut strip_text) = (0, String::new(), String::new());
        if let Some(message) = tab.editing.and_then(|id| tab.thread.messages.iter().find(|m| m.id == id)) {
            strip = 2;
            strip_title = i18n.t("chatThread.editing");
            strip_text = one_line(&message.body);
        } else if let Some(message) = tab.reply_to.and_then(|id| tab.thread.messages.iter().find(|m| m.id == id)) {
            strip = 1;
            strip_title = if message.mine {
                i18n.t("chatThread.replyingToOwn")
            } else {
                let name = message.sender_user.as_ref().map_or(title.as_str(), |u| u.name());
                i18n.with("chatThread.replyingTo", &[("name", name)])
            };
            strip_text = if message.body.is_empty() {
                i18n.t("app.picture")
            } else {
                one_line(&message.body)
            };
        }

        let check_label = match &tab.thread.target {
            Target::Member(_) if !first && tab.thread.loaded => i18n.with("chatThread.alsoByEmailTo", &[("name", &title)]),
            Target::Group(uuid) => match self.groups.iter().find(|g| &g.group_uuid == uuid) {
                Some(group) if group.may_manage() => i18n.plural("chatGroup.announceTo", group.member_count - 1, &[]),
                _ => String::new(),
            },
            _ => String::new(),
        };
        let banner = if first {
            i18n.with("chatThread.firstGoesByEmail", &[("name", &title)])
        } else {
            String::new()
        };
        let placeholder = i18n.with(if direct { "chatThread.placeholder" } else { "chatGroup.placeholder" }, &[("name", &title)]);
        let left = MESSAGE_MAX_CHARS as i64 - self.chat.get_draft().chars().count() as i64;
        let remaining = if left < REMAINING_SHOWN_BELOW as i64 {
            i18n.with("chatThread.remaining", &[("n", &left.to_string())])
        } else {
            String::new()
        };
        let quick = direct && settled && other_wrote && !tab.thread.i_wrote() && strip == 0;
        let (error, sending) = (tab.error.clone(), tab.sending);
        drop(i18n);

        self.chat.set_placeholder(placeholder.into());
        self.chat.set_strip(strip);
        self.chat.set_strip_title(strip_title.into());
        self.chat.set_strip_text(strip_text.into());
        self.chat.set_check_label(check_label.into());
        self.chat.set_banner(banner.into());
        self.chat.set_quick_answers(quick);
        self.chat.set_compose_error(error.into());
        self.chat.set_sending(sending);
        self.chat.set_remaining(remaining.into());
    }

    fn start_reply(&mut self, id: i64) {
        self.cancel_strip();
        let Some(tab) = self.active_tab_mut() else { return };
        tab.reply_to = Some(id);
        self.show_compose();
        self.show_thread(Scroll::Stay);
        self.chat.invoke_focus_compose();
    }

    fn start_edit(&mut self, id: i64) {
        self.cancel_strip();
        let draft = self.chat.get_draft().to_string();
        let Some(tab) = self.active_tab_mut() else { return };
        let Some(body) = tab
            .thread
            .messages
            .iter()
            .find(|m| m.id == id && m.mine && !m.forwarded)
            .map(|m| m.body.clone())
        else {
            return;
        };
        tab.parked = Some(draft);
        tab.editing = Some(id);
        self.chat.set_draft(body.into());
        self.show_compose();
        self.show_thread(Scroll::Stay);
        self.chat.invoke_focus_compose();
    }

    /// Ends answering or editing; an edit gives back what was being written before.
    fn cancel_strip(&mut self) {
        let Some(tab) = self.active_tab_mut() else { return };
        let was = tab.reply_to.take().is_some();
        let parked = if tab.editing.take().is_some() {
            Some(tab.parked.take().unwrap_or_default())
        } else {
            None
        };
        tab.error.clear();
        if let Some(parked) = &parked {
            self.chat.set_draft(parked.as_str().into());
        }
        if was || parked.is_some() {
            self.show_compose();
            self.show_thread(Scroll::Stay);
        }
    }

    fn send(&mut self) {
        let body = self.chat.get_draft().trim().to_string();
        let check = self.chat.get_check();
        let Some(tab) = self.active_tab_mut() else { return };
        if body.is_empty() || tab.sending || body.chars().count() > MESSAGE_MAX_CHARS {
            return;
        }
        tab.sending = true;
        tab.error.clear();
        let key = tab.thread.key.clone();
        let editing = tab
            .editing
            .and_then(|id| tab.thread.messages.iter().find(|m| m.id == id))
            .map(|m| m.message_uuid.clone());
        let reply = tab
            .reply_to
            .and_then(|id| tab.thread.messages.iter().find(|m| m.id == id))
            .map(|m| m.message_uuid.clone());
        let target = tab.thread.target.clone();
        let was_edit = editing.is_some();
        self.show_compose();
        self.ask(
            move |client| -> ApiResult<ChatMessage> {
                if let Some(uuid) = editing {
                    return client.call(documents::EDIT_MESSAGE, json!({ "messageUuid": uuid, "body": body }), "editChatMessage");
                }
                match target {
                    Target::Member(member) => {
                        // The server mails the very first message whatever is asked here.
                        let notify = if check { "EMAIL" } else { "NONE" };
                        client.call(
                            documents::SEND_MESSAGE,
                            json!({ "ref": member, "body": body, "notify": notify, "replyTo": reply }),
                            "sendChatMessage",
                        )
                    }
                    Target::Group(uuid) => client.call(
                        documents::SEND_GROUP_MESSAGE,
                        json!({ "groupUuid": uuid, "body": body, "announce": check, "replyTo": reply }),
                        "sendChatGroupMessage",
                    ),
                }
            },
            move |app, result| app.sent(&key, was_edit, result),
        );
    }

    fn sent(&mut self, key: &str, was_edit: bool, result: ApiResult<ChatMessage>) {
        if matches!(result, Err(ApiError::SessionExpired)) {
            return self.session_expired();
        }
        let is_active = self.active.as_deref() == Some(key);
        let other = self.tab(key).and_then(|t| t.who.as_ref()).map_or(String::new(), |u| u.name().to_string());
        let error = match &result {
            Ok(_) => String::new(),
            Err(e) if was_edit && e.reason() == "NOT_CONFIRMED" => self.t_with("chatThread.editNotConfirmed", &[("name", &other)]),
            Err(e) if was_edit && e.reason() == "PENDING" => self.t("chatThread.editPending"),
            Err(_) if was_edit => self.t("chatThread.editNotSaved"),
            Err(e) if e.reason() == "NOT_ALLOWED" => self.t("chatGroup.refusedNotAllowed"),
            Err(e) if e.is("CHAT_GROUP_NOT_FOUND") => self.t("chatGroup.refusedNotFound"),
            Err(_) => self.t("chatThread.notSent"),
        };
        let Some(tab) = self.tab_mut(key) else { return };
        tab.sending = false;
        tab.error = error;
        let Ok(message) = result else {
            if is_active {
                self.show_compose();
            }
            return;
        };
        let was_new = tab.thread.conversation_id.is_none();
        tab.thread.put(message);
        tab.reply_to = None;
        tab.check = false;
        let draft = if tab.editing.take().is_some() {
            tab.parked.take().unwrap_or_default()
        } else {
            String::new()
        };
        tab.draft = draft.clone();
        tab.find_hits();
        if is_active {
            self.chat.set_draft(draft.into());
            self.chat.set_check(false);
            self.show_head();
            self.show_compose();
            self.show_thread(if was_edit { Scroll::Stay } else { Scroll::Bottom });
        }
        // A first message makes a contact, and the order by last exchange changed.
        if was_new || !was_edit {
            self.load_lists();
        }
    }

    fn toggle_mute(&mut self) {
        let Some(tab) = self.active_tab_mut() else { return };
        let target = tab.thread.target.clone();
        let key = tab.thread.key.clone();
        let (title, _) = {
            let tab = self.active_tab().unwrap();
            self.tab_face(tab)
        };
        let muted = !self.chat.get_muted();
        self.set_muted(&key, muted);
        let hint = match (&target, muted) {
            (Target::Member(_), true) => "chatThread.mutedHint",
            (Target::Member(_), false) => "chatThread.unmutedHint",
            (Target::Group(_), true) => "chatGroup.mutedHint",
            (Target::Group(_), false) => "chatGroup.unmutedHint",
        };
        let hint = self.t_with(hint, &[("name", &title)]);
        self.ask(
            move |client| -> ApiResult<bool> {
                match target {
                    Target::Member(member) => client.call(
                        documents::SET_CONVERSATION_MUTED,
                        json!({ "ref": member, "muted": muted }),
                        "setChatConversationMuted",
                    ),
                    Target::Group(uuid) => client.call(documents::SET_GROUP_MUTED, json!({ "groupUuid": uuid, "muted": muted }), "setChatGroupMuted"),
                }
            },
            move |app, result| match result {
                Ok(_) => app.toast(&hint),
                // Taken back: the bell must not say what the server does not do.
                Err(_) => app.set_muted(&key, !muted),
            },
        );
    }

    fn set_muted(&mut self, key: &str, muted: bool) {
        let Some(tab) = self.tab_mut(key) else { return };
        tab.thread.muted = muted;
        if let Target::Group(uuid) = tab.thread.target.clone() {
            if let Some(group) = self.groups.iter_mut().find(|g| g.group_uuid == uuid) {
                group.muted_by_me = muted;
            }
            self.show_lists();
        }
        self.show_head();
    }

    fn copy_text(&mut self, id: i64) {
        let Some(tab) = self.active_tab() else { return };
        let Some(message) = tab.thread.messages.iter().find(|m| m.id == id) else {
            return;
        };
        let text = match &message.subject {
            Some(subject) => format!("{subject}\n{}", message.body),
            None => message.body.clone(),
        };
        self.chat.invoke_copy_text(text.into());
        let done = self.t("chatThread.textCopied");
        self.toast(&done);
    }

    fn send_gradido(&mut self) {
        let Some(Target::Member(member)) = self.active_tab().map(|t| t.thread.target.clone()) else {
            return;
        };
        let site = crate::api::endpoint(&self.settings.server);
        let site = site.trim_end_matches("/graphql");
        let community = member.community_uuid.as_deref().unwrap_or(self.own_community());
        crate::platform::open_link(&format!("{site}/send/{community}/{}", member.gradido_id));
    }

    fn image_clicked(&mut self, id: i64) {
        let Some(tab) = self.active_tab() else { return };
        let (name, caption) = if id < 0 {
            let Some(transfer) = tab.thread.transfers.iter().find(|t| t.id == -id) else {
                return;
            };
            let Some(name) = transfer_picture(transfer) else { return };
            (name, String::new())
        } else {
            let Some(message) = tab.thread.messages.iter().find(|m| m.id == id) else {
                return;
            };
            let Some(image) = message.images.first() else { return };
            let who = if message.mine {
                self.t("chatThread.you")
            } else {
                message
                    .sender_user
                    .as_ref()
                    .or(tab.who.as_ref())
                    .map_or(String::new(), |u| u.name().to_string())
            };
            let i18n = self.i18n.borrow();
            let at = parse_date(&message.created_at);
            (image.image_uuid.clone(), format!("{who} · {} {}", i18n.date(at), i18n.time(at)))
        };
        let Some(Picture::There(picture)) = self.pictures.get(&name) else { return };
        self.chat.set_view_picture(picture.clone());
        self.chat.set_view_caption(caption.into());
        self.layer = super::layers::Layer::Picture;
        self.chat.set_layer(4);
    }

    // ---------------------------------------------------------------- search

    fn show_search(&mut self) {
        let Some(tab) = self.active_tab() else { return };
        let i18n = self.i18n.borrow();
        let count = if tab.needle.is_empty() {
            String::new()
        } else if tab.hits.is_empty() {
            if tab.thread.loading {
                i18n.t("chatSearch.loading")
            } else {
                i18n.t("chatSearch.none")
            }
        } else {
            // Counted from the newest, as the member steps: 1 is the latest hit.
            i18n.with(
                "chatSearch.count",
                &[("current", &(tab.hits.len() - tab.hit).to_string()), ("count", &tab.hits.len().to_string())],
            )
        };
        drop(i18n);
        self.chat.set_search_count(count.into());
    }

    fn search_edited(&mut self, raw: String) {
        let Some(tab) = self.active_tab_mut() else { return };
        tab.search_raw = raw.clone();
        let needle = search::needle(&raw);
        if needle == tab.needle {
            return;
        }
        tab.needle = needle;
        tab.find_hits();
        tab.hit = tab.hits.len().saturating_sub(1);
        let key = tab.thread.key.clone();
        let newest = tab.hits.last().copied();
        self.show_search();
        match newest {
            Some(id) => self.show_thread(Scroll::Jump(id)),
            None => self.show_thread(Scroll::Stay),
        }
        // A search looks further back than what happens to be loaded.
        self.continue_seeking(&key);
    }

    fn search_step(&mut self, older: bool) {
        let Some(tab) = self.active_tab_mut() else { return };
        if tab.hits.is_empty() {
            return;
        }
        tab.hit = if older {
            tab.hit.checked_sub(1).unwrap_or(tab.hits.len() - 1)
        } else {
            (tab.hit + 1) % tab.hits.len()
        };
        let id = tab.hits[tab.hit];
        self.show_search();
        self.show_thread(Scroll::Jump(id));
    }
}

/// The key under which a transfer's own photo is held, when it has one.
fn transfer_picture(transfer: &Transaction) -> Option<String> {
    if let Some(greeting) = &transfer.greeting {
        if greeting.has_picture {
            return transfer.link_id.map(|link| format!("greeting/{link}"));
        }
        return None;
    }
    match &transfer.picture {
        Some(picture) if picture.has_picture => Some(format!("transfer/{}", transfer.id)),
        _ => None,
    }
}

pub fn wire(chat: &ChatWindow) {
    chat.on_tab_selected(|key| {
        let key = key.to_string();
        with_app(move |app| app.activate(&key));
    });
    chat.on_tab_closed(|key| {
        let key = key.to_string();
        with_app(move |app| app.close_tab(&key));
    });
    chat.on_send(|| with_app(|app| app.send()));
    chat.on_cancel_strip(|| with_app(|app| app.cancel_strip()));
    chat.on_quick(|text| {
        with_app(move |app| {
            app.chat.set_draft(text);
            app.send();
        })
    });
    chat.on_draft_edited(|| {
        with_app(|app| {
            if let Some(tab) = app.active_tab_mut()
                && !tab.error.is_empty()
            {
                tab.error.clear();
            }
            app.show_compose();
        })
    });
    chat.on_attention(|| with_app(|app| app.mark_read_if_watched()));
    chat.on_load_older(|| with_app(|app| app.load_older()));
    chat.on_reply(|id| with_app(move |app| app.start_reply(id as i64)));
    chat.on_edit(|id| with_app(move |app| app.start_edit(id as i64)));
    chat.on_forward(|id| with_app(move |app| app.forward(id as i64)));
    chat.on_copy(|id| with_app(move |app| app.copy_text(id as i64)));
    chat.on_quote_clicked(|id| with_app(move |app| app.quote_clicked(id as i64)));
    chat.on_link(|url| crate::platform::open_link(&url));
    chat.on_image_clicked(|id| with_app(move |app| app.image_clicked(id as i64)));
    chat.on_writer_clicked(|key| {
        let key = key.to_string();
        with_app(move |app| app.open_writer(&key));
    });
    chat.on_toggle_mute(|| with_app(|app| app.toggle_mute()));
    chat.on_toggle_favorite(|| {
        with_app(|app| {
            if let Some(key) = app.active.clone() {
                app.toggle_favorite(&key);
            }
        })
    });
    chat.on_send_gradido(|| with_app(|app| app.send_gradido()));
    chat.on_open_members(|| with_app(|app| app.open_members()));
    chat.on_search_edited(|text| {
        let text = text.to_string();
        with_app(move |app| app.search_edited(text));
    });
    chat.on_search_step(|older| with_app(move |app| app.search_step(older)));
    chat.window().on_close_requested(|| {
        with_app(|app| app.keep_active());
        slint::CloseRequestResponse::HideWindow
    });
}

impl App {
    /// A name in a group thread: the way to a conversation with that member alone.
    fn open_writer(&mut self, key: &str) {
        let own = self.own_community().to_string();
        let user = self
            .active_tab()
            .and_then(|tab| {
                tab.thread
                    .messages
                    .iter()
                    .filter_map(|m| m.sender_user.as_ref())
                    .find(|u| u.member().key(&own) == key)
            })
            .cloned();
        if let Some(user) = user {
            self.open_member(user);
        }
    }

    pub(crate) fn member_of_key(&self, key: &str) -> Option<MemberRef> {
        let own = self.own_community();
        self.contacts.iter().map(|c| c.user.member()).find(|m| m.key(own) == key)
    }
}
