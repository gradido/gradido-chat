//! The contact list: groups, favourites, everybody else — in the server's
//! order, which is by last exchange. And the small pictures that go with them.

use super::App;
use crate::BuddyRow;
use crate::api::types::{ChatGroup, ContactList, MemberAvatar, MemberRef, User, parse_date};
use crate::api::{ApiResult, documents};
use crate::core::avatar;
use base64::Engine;
use serde_json::json;
use slint::{ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};

/// One request for all: searching then happens here, as on the web's contact page.
const CONTACTS_MAX: i64 = 1000;
const AVATARS_PER_REQUEST: usize = 100;

pub fn color(rgb: u32) -> slint::Color {
    slint::Color::from_rgb_u8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// A JPEG as the server sends it, base64 with or without a data-URI head.
/// Decoded off the UI thread; the buffer may cross threads, an `Image` may not.
pub fn decode_picture(base64_jpeg: &str) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let data = base64_jpeg.rsplit(',').next()?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(data.trim()).ok()?;
    let picture = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg).ok()?.to_rgba8();
    Some(SharedPixelBuffer::clone_from_slice(picture.as_raw(), picture.width(), picture.height()))
}

/// How somebody looks in a row, a tab or a head.
pub struct Face {
    pub letters: String,
    pub color: slint::Color,
    pub avatar: Option<slint::Image>,
}

impl App {
    pub(crate) fn member_face(&self, user: &User) -> Face {
        let name = user.name();
        Face {
            letters: avatar::member_letters(name),
            color: color(avatar::color(user.avatar_color_index, name)),
            avatar: self.avatars.get(&self.user_key(user)).map(|(_, picture)| picture.clone()),
        }
    }

    pub(crate) fn group_face(&self, title: &str) -> Face {
        let letters = avatar::group_letters(title);
        Face {
            color: color(avatar::color(None, &letters)),
            letters,
            avatar: None,
        }
    }

    pub(crate) fn load_lists(&mut self) {
        self.ask(
            |client| {
                let contacts: ApiResult<ContactList> = client.call(documents::CONTACTS, json!({ "pageSize": CONTACTS_MAX }), "contactList");
                let groups: ApiResult<Vec<ChatGroup>> = client.call(documents::GROUPS, json!({}), "chatGroups");
                (contacts, groups)
            },
            |app, (contacts, groups)| {
                // A list that did not come leaves the one on screen: the next beat asks again.
                if let Ok(list) = contacts {
                    app.contacts = list.contacts;
                }
                if let Ok(groups) = groups {
                    app.groups = groups;
                }
                let users: Vec<User> = app.contacts.iter().map(|c| c.user.clone()).collect();
                app.want_avatars(&users);
                app.show_lists();
                app.show_tabs();
                app.show_head();
            },
        );
    }

    pub(crate) fn show_lists(&mut self) {
        let i18n = self.i18n.borrow();
        let needle = self.filter.trim().to_lowercase();
        let fits = |text: &str| needle.is_empty() || text.to_lowercase().contains(&needle);
        let mut rows = Vec::new();
        let section = |key: &str, title: String| BuddyRow {
            key: key.into(),
            kind: 0,
            title: title.to_uppercase().into(),
            ..Default::default()
        };

        rows.push(section("groups", format!("{} · {}", i18n.t("chatGroup.heading"), self.groups.len())));
        for group in self.groups.iter().filter(|g| fits(&g.title)) {
            let face = self.group_face(&group.title);
            let last = group.last_message_at.as_deref().map(|at| i18n.date(parse_date(at)));
            rows.push(BuddyRow {
                key: format!("group/{}", group.group_uuid).into(),
                kind: 2,
                title: group.title.as_str().into(),
                line2: i18n.plural("chatGroup.memberCount", group.member_count, &[]).into(),
                line3: last.unwrap_or_default().into(),
                letters: face.letters.into(),
                color: face.color,
                unread: group.unread_messages > 0,
                muted: group.muted_by_me,
                ..Default::default()
            });
        }

        let shown: Vec<_> = self.contacts.iter().filter(|c| fits(c.user.name()) || fits(&c.user.gradido_id)).collect();
        for favorites in [true, false] {
            let part: Vec<_> = shown.iter().filter(|c| c.favorite == favorites).collect();
            if part.is_empty() {
                continue;
            }
            let title = if favorites {
                i18n.t("contacts.favorites")
            } else {
                i18n.t("contacts.allShort")
            };
            rows.push(section(if favorites { "favorites" } else { "all" }, format!("{title} · {}", part.len())));
            for contact in part {
                let face = self.member_face(&contact.user);
                rows.push(BuddyRow {
                    key: self.user_key(&contact.user).into(),
                    kind: 1,
                    title: contact.user.name().into(),
                    line2: contact.user.community_name.clone().unwrap_or_default().into(),
                    line3: i18n.date(parse_date(&contact.last_at)).into(),
                    letters: face.letters.into(),
                    color: face.color,
                    has_avatar: face.avatar.is_some(),
                    avatar: face.avatar.unwrap_or_default(),
                    unread: contact.unread_chat_messages > 0,
                    favorite: contact.favorite,
                    ..Default::default()
                });
            }
        }
        drop(i18n);
        self.buddy.set_rows(ModelRc::new(VecModel::from(rows)));
    }

    /// Asks for the pictures not held yet, or held in an older state.
    pub(crate) fn want_avatars(&mut self, users: &[User]) {
        let mut refs: Vec<MemberRef> = Vec::new();
        for user in users {
            let key = self.user_key(user);
            match &user.avatar_updated_at {
                // No date means no picture: one held from before is out of date.
                None => {
                    self.avatars.remove(&key);
                }
                Some(stamp) => {
                    let held = self.avatars.get(&key).is_some_and(|(of, _)| of == stamp);
                    if !held && self.avatars_asked.insert(format!("{key}@{stamp}")) {
                        refs.push(user.member());
                    }
                }
            }
        }
        let own = self.own_community().to_string();
        let Some(media) = &self.media else { return };
        for chunk in refs.chunks(AVATARS_PER_REQUEST) {
            let chunk = chunk.to_vec();
            let own = own.clone();
            media.ask(
                move |client| {
                    let answer: ApiResult<Vec<MemberAvatar>> = client.call(documents::MEMBER_AVATARS, json!({ "refs": chunk }), "memberAvatars");
                    answer
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|a| {
                            let key = MemberRef {
                                gradido_id: a.gradido_id,
                                community_uuid: a.community_uuid,
                            }
                            .key(&own);
                            Some((key, a.avatar_updated_at, decode_picture(&a.avatar)?))
                        })
                        .collect::<Vec<_>>()
                },
                |app, pictures| {
                    if pictures.is_empty() {
                        return;
                    }
                    for (key, stamp, buffer) in pictures {
                        app.avatars.insert(key, (stamp, slint::Image::from_rgba8(buffer)));
                    }
                    app.show_lists();
                    app.show_tabs();
                    app.show_head();
                    app.show_thread(super::threads::Scroll::Stay);
                },
            );
        }
    }

    pub(crate) fn toggle_favorite(&mut self, key: &str) {
        let own = self.own_community().to_string();
        let Some(contact) = self.contacts.iter_mut().find(|c| c.user.member().key(&own) == key) else {
            return;
        };
        // Shown at once, taken back if the server says no.
        contact.favorite = !contact.favorite;
        let (member, on) = (contact.user.member(), contact.favorite);
        self.show_lists();
        self.show_head();
        let key = key.to_string();
        self.ask(
            move |client| {
                let document = if on { documents::ADD_FAVORITE } else { documents::REMOVE_FAVORITE };
                client.call::<bool>(document, json!({ "ref": member }), if on { "addFavorite" } else { "removeFavorite" })
            },
            move |app, result| {
                if result.is_err() {
                    let own = app.own_community().to_string();
                    if let Some(contact) = app.contacts.iter_mut().find(|c| c.user.member().key(&own) == key) {
                        contact.favorite = !on;
                    }
                    app.show_lists();
                    app.show_head();
                }
            },
        );
    }
}
