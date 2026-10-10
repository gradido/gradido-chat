//! Debug builds only: the program takes commands on standard input, one per
//! line, and does what a click would do. For trying things out from a script
//! (`echo "open first" > fifo`) without touching the mouse.
//!
//!   open <key>|first|<n>     open a conversation from the contact list
//!   tab <n>                  switch to the n-th tab
//!   close                    close the active tab
//!   type <text>              put text into the message field (\n breaks the line)
//!   send                     send it
//!   reply|edit|forward|copy <n>   n-th message from the end, 1 is the newest
//!   cancel                   end answering or editing
//!   search <text>            search in the thread; `search` alone ends it
//!   step older|newer
//!   older                    load older messages
//!   members | newgroup | closelayer | mute | heart
//!   pick <n>                 tick the n-th entry of the picker
//!   name <text> | go         fill the picker's name / press its button
//!   keys                     print the keys of the contact list
//!   dark [off|system]        choose the dark scheme, the light one, the system's
//!   settings [off]           open or close the settings
//!   connect <server> <token> | signout   the first page's button, and the way back

use super::{App, with_app};
use slint::ComponentHandle;
use slint::Model;

pub fn listen() {
    std::thread::spawn(|| {
        for line in std::io::stdin().lines().map_while(Result::ok) {
            let _ = slint::invoke_from_event_loop(move || with_app(move |app| app.command(line.trim())));
        }
    });
}

impl App {
    fn nth_message(&self, n: &str) -> Option<i64> {
        let n: usize = n.parse().ok()?;
        self.active_tab()?.thread.messages.iter().rev().nth(n.saturating_sub(1)).map(|m| m.id)
    }

    fn command(&mut self, line: &str) {
        let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
        let rows = self.buddy.get_rows();
        let keys: Vec<String> = rows.iter().filter(|r| r.kind != 0).map(|r| r.key.to_string()).collect();
        match verb {
            "keys" => println!("{keys:?}"),
            "open" => {
                let key = match rest.parse::<usize>() {
                    Ok(n) => keys.get(n.saturating_sub(1)).cloned(),
                    Err(_) if rest == "first" => keys.first().cloned(),
                    Err(_) => Some(rest.to_string()),
                };
                if let Some(key) = key {
                    self.open_by_key(&key);
                }
            }
            "tab" => {
                if let Some(key) = rest
                    .parse::<usize>()
                    .ok()
                    .and_then(|n| self.tabs.get(n.saturating_sub(1)))
                    .map(|t| t.thread.key.clone())
                {
                    self.activate(&key);
                }
            }
            "close" => {
                if let Some(key) = self.active.clone() {
                    self.close_tab(&key);
                }
            }
            "type" => {
                self.chat.set_draft(rest.replace("\\n", "\n").into());
                self.show_compose();
            }
            "check" => self.chat.set_check(rest != "off"),
            "send" => self.chat.invoke_send(),
            "reply" => self.nth_message(rest).into_iter().for_each(|id| self.chat.invoke_reply(id as i32)),
            "edit" => self.nth_message(rest).into_iter().for_each(|id| self.chat.invoke_edit(id as i32)),
            "forward" => self.nth_message(rest).into_iter().for_each(|id| self.chat.invoke_forward(id as i32)),
            "copy" => self.nth_message(rest).into_iter().for_each(|id| self.chat.invoke_copy(id as i32)),
            "quote" => self.nth_message(rest).into_iter().for_each(|id| self.chat.invoke_quote_clicked(id as i32)),
            "cancel" => self.chat.invoke_cancel_strip(),
            "search" => {
                self.chat.set_search_open(!rest.is_empty());
                self.chat.set_search_text(rest.into());
                self.chat.invoke_search_edited(rest.into());
            }
            "step" => self.chat.invoke_search_step(rest != "newer"),
            "older" => self.chat.invoke_load_older(),
            "members" => self.chat.invoke_open_members(),
            "newgroup" => self.buddy.invoke_new_group(),
            "closelayer" => self.chat.invoke_layer_closed(),
            "mute" => self.chat.invoke_toggle_mute(),
            "heart" => self.chat.invoke_toggle_favorite(),
            "pick" => {
                let rows = self.chat.get_pick_rows();
                let picks: Vec<_> = rows.iter().filter(|r| r.kind != 0).collect();
                if let Some(row) = rest.parse::<usize>().ok().and_then(|n| picks.get(n.saturating_sub(1))) {
                    self.chat.invoke_pick_toggle(row.key.clone());
                }
            }
            "name" => self.chat.set_pick_name(rest.into()),
            "words" => self.chat.set_pick_words(rest.into()),
            "asktext" => self.chat.set_ask_text(rest.into()),
            "go" => {
                if self.chat.get_layer() == 3 {
                    self.chat.invoke_ask_go_clicked()
                } else {
                    self.chat.invoke_pick_go_clicked()
                }
            }
            "leave" => self.chat.invoke_members_leave(),
            "rename" => self.chat.invoke_members_rename(),
            "filter" => {
                self.buddy.set_filter(rest.into());
                self.buddy.invoke_filter_edited(rest.into());
            }
            "connect" => {
                if let Some((server, token)) = rest.split_once(' ') {
                    self.buddy.set_server(server.into());
                    self.buddy.set_token(token.into());
                    self.buddy.invoke_connect();
                }
            }
            "signout" => self.buddy.invoke_sign_out(),
            "dark" => {
                use crate::platform::settings::Scheme;
                self.set_scheme(match rest {
                    "off" => Scheme::Light,
                    "system" => Scheme::System,
                    _ => Scheme::Dark,
                });
            }
            "settings" => self.buddy.set_settings_open(rest != "off"),
            "focus" => {
                let _ = self.chat.window();
                self.chat.invoke_focus_compose();
            }
            _ => eprintln!("unknown command: {line}"),
        }
    }
}
