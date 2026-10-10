//! What lies over a conversation for a moment: choosing people (new group,
//! new members, forwarding), a group's members, a question, a picture.

use super::threads::{MESSAGE_MAX_CHARS, group_key};
use super::{App, with_app};
use crate::api::types::{ChatGroup, ChatGroupMember, MemberRef};
use crate::api::{ApiError, ApiResult, documents};
use crate::core::thread::Target;
use crate::{ChatWindow, MemberRow, PickRow};
use serde_json::json;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

/// The server takes no more targets for one forward.
const FORWARD_MAX_TARGETS: usize = 5;
const GROUP_MEMBERS_MAX: usize = 99;
const MODERATORS_MAX: i64 = 5;

pub enum Pick {
    NewGroup,
    AddMembers(String),
    /// The uuid of the message to forward.
    Forward(String),
}

pub enum Ask {
    Rename(String),
    Leave(String),
    Remove(String, MemberRef),
}

pub enum Layer {
    None,
    Picker { purpose: Pick, chosen: Vec<String>, already: Vec<String> },
    Members { group: String, members: Vec<ChatGroupMember> },
    Ask(Ask),
    Picture,
}

impl App {
    fn close_layer(&mut self) {
        self.layer = Layer::None;
        self.chat.set_layer(0);
        self.chat.set_layer_error(SharedString::new());
        self.chat.set_layer_busy(false);
        // A group opened only to carry a dialog leaves no empty window behind.
        if self.tabs.is_empty() {
            let _ = self.chat.hide();
        }
    }

    /// The server's `CODE: REASON`, in the member's words.
    fn refusal(&self, error: &ApiError) -> String {
        let key = match error.reason() {
            "FULL" => "chatGroup.refusedFull",
            "NOT_A_CONTACT" => "chatGroup.refusedNotAContact",
            "NOT_A_MEMBER" => "chatGroup.refusedNotAMember",
            "NOT_ALLOWED" => "chatGroup.refusedNotAllowed",
            "OTHER_COMMUNITY" => "chatGroup.refusedOtherCommunity",
            "TITLE" => "chatGroup.refusedTitle",
            "UNKNOWN_MEMBER" => "chatGroup.refusedUnknownMember",
            "TOO_MANY_MODERATORS" => {
                return self
                    .i18n
                    .borrow()
                    .with("chatGroup.refusedTooManyModerators", &[("n", &MODERATORS_MAX.to_string())]);
            }
            _ if error.is("CHAT_GROUP_NOT_FOUND") => "chatGroup.refusedNotFound",
            _ => "chatGroup.refused",
        };
        self.t(key)
    }

    fn layer_failed(&mut self, error: ApiError) {
        if error == ApiError::SessionExpired {
            return self.session_expired();
        }
        self.chat.set_layer_busy(false);
        self.chat.set_layer_error(self.refusal(&error).into());
    }

    // ---------------------------------------------------------------- picker

    fn open_picker(&mut self, purpose: Pick, already: Vec<String>) {
        let (title, go, hint) = match &purpose {
            Pick::NewGroup => (self.t("chatGroup.newTitle"), self.t("chatGroup.create"), self.t("chatGroup.newHint")),
            Pick::AddMembers(_) => (self.t("chatGroup.addMembers"), self.t("chatGroup.addGo"), self.t("chatGroup.addHint")),
            Pick::Forward(_) => (self.t("chatForward.title"), self.t("chatForward.go"), self.t("chatForward.to")),
        };
        self.chat.set_layer_title(title.into());
        self.chat.set_pick_go(go.into());
        self.chat.set_pick_hint(hint.into());
        self.chat.set_pick_with_name(matches!(purpose, Pick::NewGroup));
        self.chat.set_pick_with_words(matches!(purpose, Pick::Forward(_)));
        self.chat.set_pick_name(SharedString::new());
        self.chat.set_pick_words(SharedString::new());
        self.chat.set_pick_search(SharedString::new());
        self.chat.set_pick_check(false);
        self.chat.set_layer_error(SharedString::new());
        self.chat.set_layer_busy(false);
        self.layer = Layer::Picker {
            purpose,
            chosen: Vec::new(),
            already,
        };
        self.show_picker();
        self.chat.set_layer(1);
        let _ = self.chat.show();
    }

    fn show_picker(&mut self) {
        let Layer::Picker { purpose, chosen, already } = &self.layer else { return };
        let i18n = self.i18n.borrow();
        let needle = self.chat.get_pick_search().trim().to_lowercase();
        let fits = |text: &str| needle.is_empty() || text.to_lowercase().contains(&needle);
        let forward = matches!(purpose, Pick::Forward(_));
        let full = forward && chosen.len() >= FORWARD_MAX_TARGETS;
        let mut rows = Vec::new();
        let section = |title: String| PickRow {
            kind: 0,
            title: title.to_uppercase().into(),
            enabled: true,
            ..Default::default()
        };

        if forward && !self.groups.is_empty() {
            rows.push(section(i18n.t("chatGroup.pickGroups")));
            for group in self.groups.iter().filter(|g| fits(&g.title)) {
                let key = group_key(&group.group_uuid);
                let face = self.group_face(&group.title);
                let checked = chosen.contains(&key);
                rows.push(PickRow {
                    kind: 2,
                    title: group.title.as_str().into(),
                    sub: i18n.plural("chatGroup.memberCount", group.member_count, &[]).into(),
                    letters: face.letters.into(),
                    color: face.color,
                    checked,
                    enabled: checked || !full,
                    key: key.into(),
                    ..Default::default()
                });
            }
            rows.push(section(i18n.t("chatGroup.pickContacts")));
        }
        for contact in self.contacts.iter().filter(|c| fits(c.user.name())) {
            let key = self.user_key(&contact.user);
            // Whoever is in the group already is not offered again.
            if already.contains(&key) {
                continue;
            }
            let face = self.member_face(&contact.user);
            let checked = chosen.contains(&key);
            let community = contact.user.community_name.clone().unwrap_or_default();
            rows.push(PickRow {
                kind: 1,
                title: contact.user.name().into(),
                // Groups and forwards do not cross communities yet.
                sub: if contact.home_community {
                    community
                } else {
                    i18n.with("chatGroup.pickLater", &[("community", &community)])
                }
                .into(),
                letters: face.letters.into(),
                color: face.color,
                has_avatar: face.avatar.is_some(),
                avatar: face.avatar.unwrap_or_default(),
                checked,
                enabled: contact.home_community && (checked || !full),
                key: key.into(),
            });
        }
        let members = chosen.iter().filter(|key| !key.starts_with("group/")).count();
        let (check, go) = match purpose {
            Pick::Forward(_) if members == 1 => {
                let name = self
                    .contacts
                    .iter()
                    .find(|c| chosen.contains(&self.user_key(&c.user)))
                    .map_or("", |c| c.user.name());
                (i18n.with("chatForward.byEmailOne", &[("name", name)]), i18n.t("chatForward.go"))
            }
            Pick::Forward(_) if members > 1 => (
                i18n.with("chatForward.byEmailMany", &[("count", &members.to_string())]),
                i18n.with("chatForward.goTo", &[("count", &chosen.len().to_string())]),
            ),
            Pick::Forward(_) => (
                String::new(),
                if chosen.len() > 1 {
                    i18n.with("chatForward.goTo", &[("count", &chosen.len().to_string())])
                } else {
                    i18n.t("chatForward.go")
                },
            ),
            Pick::NewGroup => (String::new(), i18n.t("chatGroup.create")),
            Pick::AddMembers(_) => (String::new(), i18n.t("chatGroup.addGo")),
        };
        // A group may be opened alone and filled later; the others need somebody.
        let enabled = matches!(purpose, Pick::NewGroup) || !chosen.is_empty();
        drop(i18n);
        self.chat.set_pick_rows(ModelRc::new(VecModel::from(rows)));
        self.chat.set_pick_check_label(check.into());
        self.chat.set_pick_go(go.into());
        self.chat.set_pick_go_enabled(enabled);
    }

    fn pick_toggle(&mut self, key: String) {
        let Layer::Picker { purpose, chosen, .. } = &mut self.layer else { return };
        let max = if matches!(purpose, Pick::Forward(_)) {
            FORWARD_MAX_TARGETS
        } else {
            GROUP_MEMBERS_MAX
        };
        match chosen.iter().position(|k| *k == key) {
            Some(at) => {
                chosen.remove(at);
            }
            None if chosen.len() < max => chosen.push(key),
            None => {}
        }
        self.show_picker();
    }

    fn pick_go(&mut self) {
        let Layer::Picker { purpose, chosen, .. } = &self.layer else { return };
        let members: Vec<MemberRef> = chosen.iter().filter_map(|key| self.member_of_key(key)).collect();
        let groups: Vec<String> = chosen.iter().filter_map(|key| key.strip_prefix("group/").map(str::to_string)).collect();
        self.chat.set_layer_busy(true);
        self.chat.set_layer_error(SharedString::new());
        match purpose {
            Pick::NewGroup => {
                let title = self.chat.get_pick_name().split_whitespace().collect::<Vec<_>>().join(" ");
                self.ask(
                    move |client| client.call::<ChatGroup>(documents::CREATE_GROUP, json!({ "title": title, "members": members }), "createChatGroup"),
                    |app, result| match result {
                        Ok(group) => {
                            let key = group_key(&group.group_uuid);
                            app.groups.insert(0, group);
                            app.close_layer();
                            app.show_lists();
                            app.open_by_key(&key);
                            app.load_lists();
                        }
                        Err(error) => app.layer_failed(error),
                    },
                );
            }
            Pick::AddMembers(uuid) => {
                let uuid = uuid.clone();
                self.ask(
                    move |client| {
                        client.call::<ChatGroup>(
                            documents::ADD_GROUP_MEMBERS,
                            json!({ "groupUuid": uuid, "members": members }),
                            "addChatGroupMembers",
                        )
                    },
                    |app, result| match result {
                        Ok(group) => {
                            let uuid = group.group_uuid.clone();
                            app.put_group(group);
                            app.open_members_of(&uuid);
                        }
                        Err(error) => app.layer_failed(error),
                    },
                );
            }
            Pick::Forward(message) => {
                let message = message.clone();
                let words: String = self.chat.get_pick_words().trim().chars().take(MESSAGE_MAX_CHARS).collect();
                let words = if words.is_empty() { None } else { Some(words) };
                let by_email = self.chat.get_pick_check();
                let wanted = chosen.len();
                let names = chosen.iter().filter_map(|key| self.name_of_key(key)).collect::<Vec<_>>().join(", ");
                self.ask(
                    move |client| {
                        client.call::<Vec<serde_json::Value>>(
                            documents::FORWARD_MESSAGE,
                            json!({ "messageUuid": message, "groupUuids": groups, "members": members, "words": words, "alsoByEmail": by_email }),
                            "forwardChatMessage",
                        )
                    },
                    move |app, result| match result {
                        Ok(copies) => {
                            app.close_layer();
                            // Words travel as a message of their own: count the forwards, not the copies.
                            let done = copies.len().min(wanted);
                            let text = if done >= wanted || copies.len() >= wanted {
                                app.t_with("chatForward.done", &[("names", &names)])
                            } else {
                                app.t_with("chatForward.partly", &[("done", &done.to_string()), ("count", &wanted.to_string())])
                            };
                            app.toast(&text);
                            app.poll_now();
                        }
                        Err(ApiError::SessionExpired) => app.session_expired(),
                        Err(_) => {
                            app.chat.set_layer_busy(false);
                            app.chat.set_layer_error(app.t("chatForward.failed").into());
                        }
                    },
                );
            }
        }
    }

    fn name_of_key(&self, key: &str) -> Option<String> {
        if let Some(uuid) = key.strip_prefix("group/") {
            return self.groups.iter().find(|g| g.group_uuid == uuid).map(|g| g.title.clone());
        }
        self.contacts.iter().find(|c| self.user_key(&c.user) == key).map(|c| c.user.name().to_string())
    }

    pub(crate) fn new_group(&mut self) {
        self.open_picker(Pick::NewGroup, Vec::new());
    }

    pub(crate) fn forward(&mut self, id: i64) {
        let Some(uuid) = self
            .active_tab()
            .and_then(|t| t.thread.messages.iter().find(|m| m.id == id))
            .map(|m| m.message_uuid.clone())
        else {
            return;
        };
        self.open_picker(Pick::Forward(uuid), Vec::new());
    }

    // --------------------------------------------------------------- members

    fn put_group(&mut self, group: ChatGroup) {
        match self.groups.iter_mut().find(|g| g.group_uuid == group.group_uuid) {
            Some(held) => *held = group,
            None => self.groups.insert(0, group),
        }
        self.show_lists();
        self.show_tabs();
        self.show_head();
        self.show_compose();
    }

    pub(crate) fn open_members(&mut self) {
        let Some(Target::Group(uuid)) = self.active_tab().map(|t| t.thread.target.clone()) else {
            return;
        };
        self.open_members_of(&uuid);
    }

    fn open_members_of(&mut self, uuid: &str) {
        self.layer = Layer::Members {
            group: uuid.to_string(),
            members: Vec::new(),
        };
        self.chat.set_layer_error(SharedString::new());
        self.chat.set_layer_busy(false);
        self.chat.set_members_loading(true);
        self.show_members();
        self.chat.set_layer(2);
        let uuid = uuid.to_string();
        self.ask(
            {
                let uuid = uuid.clone();
                move |client| client.call::<Vec<ChatGroupMember>>(documents::GROUP_MEMBERS, json!({ "groupUuid": uuid }), "chatGroupMembers")
            },
            move |app, result: ApiResult<Vec<ChatGroupMember>>| {
                let Layer::Members { group, members } = &mut app.layer else { return };
                if *group != uuid {
                    return;
                }
                app.chat.set_members_loading(false);
                match result {
                    Ok(list) => {
                        *members = list;
                        let users: Vec<_> = members.iter().map(|m| m.user.clone()).collect();
                        app.want_avatars(&users);
                        app.show_members();
                    }
                    Err(error) => app.layer_failed(error),
                }
            },
        );
    }

    pub(crate) fn show_members(&mut self) {
        let Layer::Members { group, members } = &self.layer else { return };
        let Some(held) = self.groups.iter().find(|g| &g.group_uuid == group) else {
            return;
        };
        let i18n = self.i18n.borrow();
        let me = self.me.as_ref().map_or("", |me| me.gradido_id.as_str());
        let i_own = held.role == "OWNER";
        let rows: Vec<MemberRow> = members
            .iter()
            .map(|member| {
                let face = self.member_face(&member.user);
                let is_me = member.user.gradido_id.eq_ignore_ascii_case(me);
                let plain = member.role == "MEMBER";
                MemberRow {
                    key: self.user_key(&member.user).into(),
                    title: if is_me {
                        i18n.with("chatGroup.memberYou", &[("name", member.user.name())])
                    } else {
                        member.user.name().to_string()
                    }
                    .into(),
                    sub: member.user.community_name.clone().unwrap_or_default().into(),
                    role: match member.role.as_str() {
                        "OWNER" => i18n.t("chatGroup.owner"),
                        "MODERATOR" => i18n.t("chatGroup.moderator"),
                        _ => String::new(),
                    }
                    .into(),
                    letters: face.letters.into(),
                    color: face.color,
                    has_avatar: face.avatar.is_some(),
                    avatar: face.avatar.unwrap_or_default(),
                    // The owner names the moderators; a moderator may take out plain members.
                    moderate: if !i_own || is_me || member.role == "OWNER" {
                        0
                    } else if plain {
                        1
                    } else {
                        2
                    },
                    removable: !is_me && member.role != "OWNER" && (i_own || (held.role == "MODERATOR" && plain)),
                    me: is_me,
                }
            })
            .collect();
        let title = i18n.with("chatGroup.membersTitle", &[("n", &held.member_count.max(members.len() as i64).to_string())]);
        let may_manage = held.may_manage();
        drop(i18n);
        self.chat.set_layer_title(title.into());
        self.chat.set_member_rows(ModelRc::new(VecModel::from(rows)));
        self.chat.set_may_manage(may_manage);
    }

    fn members_group(&self) -> Option<String> {
        match &self.layer {
            Layer::Members { group, .. } => Some(group.clone()),
            _ => None,
        }
    }

    fn member_in_layer(&self, key: &str) -> Option<ChatGroupMember> {
        let Layer::Members { members, .. } = &self.layer else { return None };
        members.iter().find(|m| self.user_key(&m.user) == key).cloned()
    }

    fn members_add(&mut self) {
        let Layer::Members { group, members } = &self.layer else { return };
        let already = members.iter().map(|m| self.user_key(&m.user)).collect();
        self.open_picker(Pick::AddMembers(group.clone()), already);
    }

    fn members_moderator(&mut self, key: &str, on: bool) {
        let (Some(uuid), Some(member)) = (self.members_group(), self.member_in_layer(key)) else {
            return;
        };
        let member = member.user.member();
        self.chat.set_layer_error(SharedString::new());
        self.ask(
            {
                let uuid = uuid.clone();
                move |client| {
                    client.call::<bool>(
                        documents::SET_GROUP_MODERATOR,
                        json!({ "groupUuid": uuid, "member": member, "moderator": on }),
                        "setChatGroupModerator",
                    )
                }
            },
            move |app, result| match result {
                Ok(_) => app.open_members_of(&uuid),
                Err(error) => app.layer_failed(error),
            },
        );
    }

    // -------------------------------------------------------------- question

    fn open_ask(&mut self, ask: Ask, title: String, body: String, text: Option<String>, go: String, danger: bool) {
        self.chat.set_layer_title(title.into());
        self.chat.set_ask_body(body.into());
        self.chat.set_ask_with_text(text.is_some());
        self.chat.set_ask_text(text.unwrap_or_default().into());
        self.chat.set_ask_go(go.into());
        self.chat.set_ask_danger(danger);
        self.chat.set_layer_error(SharedString::new());
        self.chat.set_layer_busy(false);
        self.layer = Layer::Ask(ask);
        self.chat.set_layer(3);
    }

    fn members_rename(&mut self) {
        let Some(uuid) = self.members_group() else { return };
        let title = self.groups.iter().find(|g| g.group_uuid == uuid).map_or(String::new(), |g| g.title.clone());
        let (heading, go) = (self.t("chatGroup.renameTitle"), self.t("chatGroup.renameGo"));
        self.open_ask(Ask::Rename(uuid), heading, String::new(), Some(title), go, false);
    }

    fn members_leave(&mut self) {
        let Layer::Members { group, members } = &self.layer else { return };
        let uuid = group.clone();
        let Some(held) = self.groups.iter().find(|g| g.group_uuid == uuid) else {
            return;
        };
        let title = self.t_with("chatGroup.leaveTitle", &[("name", &held.title)]);
        let me = self.me.as_ref().map_or("", |me| me.gradido_id.as_str());
        // An owner who leaves hands the group on: to the longest-serving moderator, else member.
        let heir = members
            .iter()
            .filter(|m| !m.user.gradido_id.eq_ignore_ascii_case(me))
            .min_by_key(|m| (m.role != "MODERATOR", m.joined_at.clone()))
            .map(|m| m.user.name().to_string());
        let body = match heir {
            Some(name) if held.role == "OWNER" => self.t_with("chatGroup.leaveBodyOwner", &[("name", &name)]),
            _ => self.t("chatGroup.leaveBody"),
        };
        let go = self.t("chatGroup.leave");
        self.open_ask(Ask::Leave(uuid), title, body, None, go, true);
    }

    fn members_remove(&mut self, key: &str) {
        let (Some(uuid), Some(member)) = (self.members_group(), self.member_in_layer(key)) else {
            return;
        };
        let name = member.user.name().to_string();
        let (title, body, go) = (
            self.t_with("chatGroup.removeTitle", &[("name", &name)]),
            self.t_with("chatGroup.removeBody", &[("name", &name)]),
            self.t("chatGroup.remove"),
        );
        self.open_ask(Ask::Remove(uuid, member.user.member()), title, body, None, go, true);
    }

    fn ask_go(&mut self) {
        let Layer::Ask(ask) = &self.layer else { return };
        self.chat.set_layer_busy(true);
        match ask {
            Ask::Rename(uuid) => {
                let uuid = uuid.clone();
                let title = self.chat.get_ask_text().split_whitespace().collect::<Vec<_>>().join(" ");
                self.ask(
                    move |client| client.call::<ChatGroup>(documents::RENAME_GROUP, json!({ "groupUuid": uuid, "title": title }), "renameChatGroup"),
                    |app, result| match result {
                        Ok(group) => {
                            let uuid = group.group_uuid.clone();
                            app.put_group(group);
                            app.open_members_of(&uuid);
                        }
                        Err(error) => app.layer_failed(error),
                    },
                );
            }
            Ask::Leave(uuid) => {
                let uuid = uuid.clone();
                self.ask(
                    {
                        let uuid = uuid.clone();
                        move |client| client.call::<bool>(documents::LEAVE_GROUP, json!({ "groupUuid": uuid }), "leaveChatGroup")
                    },
                    move |app, result| match result {
                        Ok(_) => {
                            app.groups.retain(|g| g.group_uuid != uuid);
                            app.close_layer();
                            app.close_tab(&group_key(&uuid));
                            app.show_lists();
                        }
                        Err(error) => app.layer_failed(error),
                    },
                );
            }
            Ask::Remove(uuid, member) => {
                let (uuid, member) = (uuid.clone(), member.clone());
                self.ask(
                    {
                        let uuid = uuid.clone();
                        move |client| {
                            client.call::<bool>(
                                documents::REMOVE_GROUP_MEMBER,
                                json!({ "groupUuid": uuid, "member": member }),
                                "removeChatGroupMember",
                            )
                        }
                    },
                    move |app, result| match result {
                        Ok(_) => {
                            app.open_members_of(&uuid);
                            app.load_lists();
                        }
                        Err(error) => app.layer_failed(error),
                    },
                );
            }
        }
    }

    /// Closing a question about a group leads back to its members.
    fn layer_closed(&mut self) {
        match &self.layer {
            Layer::Ask(Ask::Rename(uuid) | Ask::Leave(uuid) | Ask::Remove(uuid, _))
            | Layer::Picker {
                purpose: Pick::AddMembers(uuid),
                ..
            } => {
                let uuid = uuid.clone();
                self.open_members_of(&uuid);
            }
            _ => self.close_layer(),
        }
    }

    /// A member in the list: the way to a conversation with that member alone.
    fn members_open(&mut self, key: &str) {
        let Some(member) = self.member_in_layer(key) else { return };
        self.close_layer();
        self.open_member(member.user);
    }
}

pub fn wire(chat: &ChatWindow) {
    chat.on_layer_closed(|| with_app(|app| app.layer_closed()));
    chat.on_pick_toggle(|key| {
        let key = key.to_string();
        with_app(move |app| app.pick_toggle(key));
    });
    chat.on_pick_search_edited(|_| with_app(|app| app.show_picker()));
    chat.on_pick_go_clicked(|| with_app(|app| app.pick_go()));
    chat.on_members_add(|| with_app(|app| app.members_add()));
    chat.on_members_rename(|| with_app(|app| app.members_rename()));
    chat.on_members_leave(|| with_app(|app| app.members_leave()));
    chat.on_members_moderator(|key, on| {
        let key = key.to_string();
        with_app(move |app| app.members_moderator(&key, on));
    });
    chat.on_members_remove(|key| {
        let key = key.to_string();
        with_app(move |app| app.members_remove(&key));
    });
    chat.on_members_open(|key| {
        let key = key.to_string();
        with_app(move |app| app.members_open(&key));
    });
    chat.on_ask_go_clicked(|| with_app(|app| app.ask_go()));
}
