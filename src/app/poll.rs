//! The beat: one question to the server every fifteen seconds, for the whole
//! program. There is no push; this is how a new message is noticed.
//! Same rules as the web's useChatUpdates.js — except that it does not pause
//! while the window is out of sight, since noticing then is the point.

use super::{App, with_app};
use crate::api::types::{ChatMessage, ChatUpdate};
use crate::api::{ApiError, ApiResult, Client, documents};
use crate::platform;
use serde_json::json;
use std::time::Duration;

const INTERVAL: Duration = Duration::from_secs(15);
const BACKOFF_MAX: Duration = Duration::from_secs(120);
/// Fetches in a row while the server says there is more.
const ROUNDS_MAX: usize = 5;
const PAGE: i64 = 50;
/// More notifications than this at once would only be noise.
const NOTES_MAX: usize = 3;

pub struct Poll {
    /// None until the first answer told where the server stands.
    after_id: Option<i64>,
    cursor: Option<String>,
    pub unread: i32,
    in_flight: bool,
    /// Somebody asked while a request was out: exactly one more follows.
    again: bool,
    delay: Duration,
    timer: slint::Timer,
}

impl Default for Poll {
    fn default() -> Self {
        Self {
            after_id: None,
            cursor: None,
            unread: 0,
            in_flight: false,
            again: false,
            delay: INTERVAL,
            timer: slint::Timer::default(),
        }
    }
}

struct Beat {
    first: bool,
    latest: i64,
    cursor: String,
    unread: i32,
    messages: Vec<ChatMessage>,
    edited: Vec<ChatMessage>,
}

fn beat(client: &Client, mut after_id: Option<i64>, mut cursor: Option<String>) -> ApiResult<Beat> {
    let first = after_id.is_none();
    let mut all = Beat {
        first,
        latest: 0,
        cursor: String::new(),
        unread: 0,
        messages: Vec::new(),
        edited: Vec::new(),
    };
    for _ in 0..ROUNDS_MAX {
        let update: ChatUpdate = client.call(
            documents::NEW_MESSAGES_SINCE,
            json!({ "afterId": after_id, "limit": PAGE, "editedCursor": cursor }),
            "newChatMessagesSince",
        )?;
        all.latest = update.latest_id;
        all.unread = update.unread_conversations;
        all.cursor = update.edited_cursor.clone();
        all.messages.extend(update.messages);
        all.edited.extend(update.edited.unwrap_or_default());
        if first || !update.has_more {
            break;
        }
        after_id = Some(update.latest_id);
        cursor = Some(update.edited_cursor);
    }
    Ok(all)
}

impl App {
    pub(crate) fn poll_start(&mut self) {
        self.poll = Poll::default();
        self.poll_now();
    }

    pub(crate) fn poll_now(&mut self) {
        if self.poll.in_flight {
            self.poll.again = true;
            return;
        }
        self.poll.in_flight = true;
        self.poll.timer.stop();
        let (after_id, cursor) = (self.poll.after_id, self.poll.cursor.clone());
        self.ask(move |client| beat(client, after_id, cursor), |app, result| app.on_beat(result));
    }

    fn on_beat(&mut self, result: ApiResult<Beat>) {
        self.poll.in_flight = false;
        match result {
            Ok(beat) => {
                self.poll.delay = INTERVAL;
                self.buddy.set_offline(false);
                if let Some(me) = &self.me {
                    self.buddy.set_status(self.t_with("app.signedInAs", &[("name", &me.name())]).into());
                }
                let unread_changed = beat.unread != self.poll.unread;
                self.poll.after_id = Some(beat.latest);
                self.poll.cursor = Some(beat.cursor);
                self.poll.unread = beat.unread;
                self.set_unread_mark(beat.unread > 0);
                if !beat.messages.is_empty() || !beat.edited.is_empty() {
                    self.arrived(&beat.messages, &beat.edited, !beat.first);
                }
                // The lists carry the unread marks and the order by last exchange.
                if !beat.messages.is_empty() || unread_changed {
                    self.load_lists();
                }
                self.save_token(false);
            }
            Err(ApiError::SessionExpired) => return self.session_expired(),
            Err(ApiError::Refused(message)) if message.starts_with("401") => return self.session_expired(),
            Err(_) => {
                self.poll.delay = (self.poll.delay * 2).min(BACKOFF_MAX);
                self.buddy.set_offline(true);
                self.buddy.set_status(self.t("app.offline").into());
            }
        }
        if std::mem::take(&mut self.poll.again) {
            return self.poll_now();
        }
        self.poll
            .timer
            .start(slint::TimerMode::SingleShot, self.poll.delay, || with_app(|app| app.poll_now()));
    }

    /// Sorts what came into the open conversations and tells the member about
    /// what nobody is looking at.
    fn arrived(&mut self, messages: &[ChatMessage], edited: &[ChatMessage], announce: bool) {
        let own = self.own_community().to_string();
        let watched = self.watched_key();
        let mut active_changed = false;
        let mut active_grew = false;
        for tab in &mut self.tabs {
            let added = tab.thread.arrivals(messages, &own);
            let changed = tab.thread.edits(edited);
            let is_active = self.active.as_deref() == Some(tab.thread.key.as_str());
            if added > 0 && watched.as_deref() != Some(tab.thread.key.as_str()) {
                // Only what somebody else wrote is news.
                tab.unread |= tab.thread.messages.iter().rev().take(added).any(|m| !m.mine);
            }
            if is_active && (added > 0 || changed) {
                active_changed = true;
                active_grew |= added > 0;
            }
        }
        if active_changed {
            let follow = active_grew && self.chat.get_at_bottom();
            self.show_thread(if follow {
                super::threads::Scroll::Bottom
            } else {
                super::threads::Scroll::Stay
            });
            self.mark_read_if_watched();
        }
        self.show_tabs();

        if !announce {
            return;
        }
        let mut notes = 0;
        let mut any = false;
        for message in messages.iter().filter(|m| !m.mine) {
            let key = self.conversation_key(message);
            if watched.is_some() && watched == key {
                continue;
            }
            // A muted group stays quiet here too.
            if let Some(uuid) = &message.group_uuid
                && self.groups.iter().any(|g| &g.group_uuid == uuid && g.muted_by_me)
            {
                continue;
            }
            any = true;
            if self.settings.notifications && notes < NOTES_MAX {
                notes += 1;
                let (title, body) = self.note_for(message);
                platform::notify::show(&title, &body);
            }
        }
        if any && self.settings.sound {
            platform::sound::new_message();
        }
    }

    fn note_for(&self, message: &ChatMessage) -> (String, String) {
        let own = self.own_community();
        let sender = message
            .sender_user
            .as_ref()
            .map(|user| user.name().to_string())
            .or_else(|| {
                self.contacts
                    .iter()
                    .find(|c| c.user.member().key(own) == message.sender.key(own))
                    .map(|c| c.user.name().to_string())
            })
            .unwrap_or_default();
        let title = match message.group_uuid.as_ref().and_then(|uuid| self.groups.iter().find(|g| &g.group_uuid == uuid)) {
            Some(group) => self.t_with("app.newIn", &[("name", &sender), ("group", &group.title)]),
            None if sender.is_empty() => self.t("chatThread.unreadDot"),
            None => self.t_with("app.newFrom", &[("name", &sender)]),
        };
        let mut body: String = message.body.chars().take(140).collect();
        if body.is_empty() && !message.images.is_empty() {
            body = self.t("app.picture");
        }
        if let Some(subject) = &message.subject {
            body = format!("{subject}\n{body}");
        }
        (title, body)
    }
}
