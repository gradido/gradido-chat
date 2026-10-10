//! The program itself: what is held between the server and the windows.
//!
//! Everything here runs on the UI thread. Requests go through a [`Worker`]
//! and come back as a closure over `&mut App`.

#[cfg(debug_assertions)]
mod debug;
mod layers;
mod lists;
mod poll;
mod threads;
mod worker;

use crate::api::types::{ChatGroup, Contact, Me, User};
use crate::api::{ApiError, Client, documents};
use crate::core::i18n::{self, I18n};
use crate::platform::settings::{Scheme, Settings};
use crate::platform::{self, token_store};
use crate::{BuddyWindow, ChatWindow, Theme, Tr, Tray};
use serde_json::json;
use slint::{ComponentHandle, SharedString};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};
use worker::Worker;

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Runs `work` on the program's state. A call that arrives while the state is
/// in use (a window callback fired from inside another) waits its turn.
pub fn with_app(work: impl FnOnce(&mut App) + 'static) {
    APP.with(|cell| match cell.try_borrow_mut() {
        Ok(mut app) => {
            if let Some(app) = app.as_mut() {
                work(app);
            }
        }
        Err(_) => slint::Timer::single_shot(Duration::ZERO, move || with_app(work)),
    });
}

pub enum Picture {
    Loading,
    There(slint::Image),
    Missing,
}

pub struct App {
    pub(crate) buddy: BuddyWindow,
    pub(crate) chat: ChatWindow,
    pub(crate) tray: Option<Tray>,
    /// Apart from the rest: the windows ask for strings while the state is in use.
    pub(crate) i18n: Rc<RefCell<I18n>>,
    pub(crate) settings: Settings,

    pub(crate) session: u64,
    pub(crate) client: Option<Client>,
    /// Lists, threads, sending: in order.
    pub(crate) main: Option<Worker>,
    /// Pictures, so that a slow one never holds up a message.
    pub(crate) media: Option<Worker>,
    pub(crate) keep_token: bool,
    pub(crate) token_saved: Option<(String, Instant)>,

    pub(crate) me: Option<Me>,
    pub(crate) contacts: Vec<Contact>,
    pub(crate) groups: Vec<ChatGroup>,
    pub(crate) filter: String,

    pub(crate) tabs: Vec<threads::Tab>,
    pub(crate) active: Option<String>,
    pub(crate) jump_seq: i32,

    pub(crate) poll: poll::Poll,
    /// Small member pictures by member key, with the date they are of.
    pub(crate) avatars: HashMap<String, (String, slint::Image)>,
    pub(crate) avatars_asked: HashSet<String>,
    /// Message pictures by image uuid; transfer pictures by `greeting/…`, `transfer/…`.
    pub(crate) pictures: HashMap<String, Picture>,
    pub(crate) layer: layers::Layer,
    pub(crate) toast_timer: slint::Timer,
}

impl App {
    pub(crate) fn t(&self, key: &str) -> String {
        self.i18n.borrow().t(key)
    }

    pub(crate) fn t_with(&self, key: &str, args: &[(&str, &str)]) -> String {
        self.i18n.borrow().with(key, args)
    }

    pub(crate) fn own_community(&self) -> &str {
        self.me.as_ref().map_or("", |me| me.community_uuid.as_str())
    }

    pub(crate) fn user_key(&self, user: &User) -> String {
        user.member().key(self.own_community())
    }

    pub(crate) fn ask<T: Send + 'static>(&self, work: impl FnOnce(&Client) -> T + Send + 'static, done: impl FnOnce(&mut App, T) + Send + 'static) {
        if let Some(worker) = &self.main {
            worker.ask(work, done);
        }
    }

    pub(crate) fn toast(&mut self, text: &str) {
        self.chat.set_toast(text.into());
        self.toast_timer.start(slint::TimerMode::SingleShot, Duration::from_secs(3), || {
            with_app(|app| app.chat.set_toast(SharedString::new()));
        });
    }

    fn set_language(&mut self, lang: &str) {
        *self.i18n.borrow_mut() = I18n::new(lang);
        let lang: SharedString = self.i18n.borrow().lang().into();
        self.buddy.global::<Tr>().set_lang(lang.clone());
        self.chat.global::<Tr>().set_lang(lang.clone());
        if let Some(tray) = &self.tray {
            tray.global::<Tr>().set_lang(lang);
        }
    }

    // ---------------------------------------------------------------- session

    /// From the first page, or at the start with what the keyring held.
    fn connect(&mut self, server: String, token: String) {
        self.buddy.set_connecting(true);
        self.buddy.set_setup_error(SharedString::new());
        self.session += 1;
        let session = self.session;
        let client = Client::new(&server, &token);
        self.client = Some(client.clone());
        self.main = Some(Worker::new(client.clone(), session));
        self.media = Some(Worker::new(client, session));
        self.settings.server = server;
        self.ask(
            |client| {
                // TEMPORARY, see `COMMIT_WITHOUT_REPLIES` in src/api/mod.rs.
                client.ask_version();
                client.call::<Me>(documents::VERIFY_LOGIN, json!({}), "verifyLogin")
            },
            |app, result| match result {
                Ok(me) => app.signed_in(me),
                Err(error) => app.not_signed_in(error),
            },
        );
    }

    fn signed_in(&mut self, me: Me) {
        self.settings.save();
        self.set_language(me.language.as_deref().unwrap_or("en"));
        let name = me.name();
        self.buddy.set_me_name(name.as_str().into());
        self.buddy.set_me_letters(crate::core::avatar::member_letters(&name).into());
        self.buddy.set_me_color(lists::color(crate::core::avatar::color(me.avatar_color_index, &name)));
        let picture = me.avatar.as_deref().and_then(lists::decode_picture).map(slint::Image::from_rgba8);
        self.buddy.set_me_has_avatar(picture.is_some());
        self.buddy.set_me_avatar(picture.unwrap_or_default());
        self.buddy.set_status(self.t_with("app.signedInAs", &[("name", &name)]).into());
        self.buddy.set_offline(false);
        self.me = Some(me);
        self.buddy.set_connecting(false);
        self.buddy.set_token(SharedString::new());
        self.buddy.set_page(1);
        self.save_token(true);
        self.load_lists();
        self.poll_start();
    }

    fn not_signed_in(&mut self, error: ApiError) {
        let text = match &error {
            ApiError::Network(_) => self.t("app.setup.failedNetwork"),
            ApiError::SessionExpired => self.t("app.setup.failedToken"),
            ApiError::Refused(message) if message.starts_with("401") => self.t("app.setup.failedToken"),
            ApiError::Refused(message) => self.t_with("app.setup.failed", &[("reason", message)]),
        };
        // A token that no longer counts is not worth keeping.
        if !matches!(error, ApiError::Network(_)) {
            token_store::forget(&self.settings.server);
        }
        self.leave_session();
        self.buddy.set_setup_error(text.into());
    }

    /// Back to the first page; what was loaded is gone.
    fn leave_session(&mut self) {
        self.session += 1;
        self.client = None;
        self.main = None;
        self.media = None;
        self.me = None;
        self.contacts.clear();
        self.groups.clear();
        self.tabs.clear();
        self.active = None;
        self.poll = poll::Poll::default();
        self.avatars.clear();
        self.avatars_asked.clear();
        self.pictures.clear();
        self.layer = layers::Layer::None;
        self.token_saved = None;
        let _ = self.chat.hide();
        self.show_tabs();
        self.chat.set_layer(0);
        self.buddy.set_connecting(false);
        self.buddy.set_page(0);
        self.buddy.set_server(self.settings.server.as_str().into());
        self.set_unread_mark(false);
    }

    fn sign_out(&mut self) {
        token_store::forget(&self.settings.server);
        self.leave_session();
    }

    /// The session ended on the server's side: only a fresh token helps.
    pub(crate) fn session_expired(&mut self) {
        token_store::forget(&self.settings.server);
        self.leave_session();
        self.buddy.set_setup_error(self.t("app.setup.failedToken").into());
        let _ = self.buddy.show();
    }

    /// Every answer brings a fresh token. The keyring gets it at most once a
    /// minute — and at the end, which is the one that matters for the next start.
    pub(crate) fn save_token(&mut self, now: bool) {
        if !self.keep_token {
            return;
        }
        let Some(client) = &self.client else { return };
        let token = client.token();
        let due = match &self.token_saved {
            Some((saved, at)) => *saved != token && (now || at.elapsed() > Duration::from_secs(60)),
            None => true,
        };
        if due && token_store::save(&self.settings.server, &token) {
            self.token_saved = Some((token, Instant::now()));
        }
    }

    fn quit(&mut self) {
        self.save_token(true);
        self.settings.save();
        let _ = slint::quit_event_loop();
    }

    pub(crate) fn set_unread_mark(&mut self, unread: bool) {
        if let Some(tray) = &self.tray {
            tray.set_unread(unread);
        }
    }
}

/// Builds the windows, wires them to the state and runs until the member quits.
pub fn run() -> Result<(), slint::PlatformError> {
    let settings = Settings::load();
    // Before the first window: the renderer cannot change while the program runs.
    // SLINT_BACKEND, where somebody sets it, has the say.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    if !settings.hardware_acceleration && std::env::var_os("SLINT_BACKEND").is_none() {
        let _ = slint::BackendSelector::new().renderer_name("software".into()).select();
    }
    let i18n = Rc::new(RefCell::new(I18n::new(&i18n::system_language())));
    let buddy = BuddyWindow::new()?;
    let chat = ChatWindow::new()?;
    let tray = if platform::HAS_TRAY { Tray::new().ok() } else { None };

    let lang: SharedString = i18n.borrow().lang().into();
    macro_rules! strings {
        ($component:expr) => {{
            let tr = $component.global::<Tr>();
            let i18n = i18n.clone();
            tr.on_lookup(move |key, _lang| i18n.borrow().t(&key).into());
            tr.set_lang(lang.clone());
        }};
    }
    strings!(buddy);
    strings!(chat);
    if let Some(tray) = &tray {
        strings!(tray);
    }

    buddy.set_server(settings.server.as_str().into());
    buddy.set_keyring_available(token_store::available());
    buddy.set_sound(settings.sound);
    buddy.set_notifications(settings.notifications);
    buddy.set_stay(settings.stay_in_tray);
    buddy.set_has_tray(tray.is_some());
    buddy.set_version(env!("CARGO_PKG_VERSION").into());
    buddy.set_hardware(settings.hardware_acceleration);
    buddy.set_scheme(settings.scheme.index());
    buddy.global::<Theme>().invoke_choose(settings.scheme.index());
    chat.global::<Theme>().invoke_choose(settings.scheme.index());
    if let Some(tray) = &tray {
        tray.set_sound(settings.sound);
        tray.set_notifications(settings.notifications);
        tray.set_stay(settings.stay_in_tray);
        tray.set_tip("Gradido Chat".into());
    }

    wire(&buddy, &chat, tray.as_ref());

    // For development: both set, the session starts without the first page
    // and without touching the keyring.
    let given = std::env::var("GRADIDO_CHAT_SERVER").ok().zip(std::env::var("GRADIDO_CHAT_TOKEN").ok());
    let from_keyring = given.is_none();
    let (server, stored) = match given {
        Some((server, token)) => (server, Some(token)),
        None => (
            settings.server.clone(),
            if settings.server.is_empty() {
                None
            } else {
                token_store::load(&settings.server)
            },
        ),
    };
    APP.with(|cell| {
        *cell.borrow_mut() = Some(App {
            buddy: buddy.clone_strong(),
            chat: chat.clone_strong(),
            tray: tray.as_ref().map(|tray| tray.clone_strong()),
            i18n,
            settings,
            session: 0,
            client: None,
            main: None,
            media: None,
            keep_token: from_keyring && token_store::available(),
            token_saved: None,
            me: None,
            contacts: Vec::new(),
            groups: Vec::new(),
            filter: String::new(),
            tabs: Vec::new(),
            active: None,
            jump_seq: 0,
            poll: poll::Poll::default(),
            avatars: HashMap::new(),
            avatars_asked: HashSet::new(),
            pictures: HashMap::new(),
            layer: layers::Layer::None,
            toast_timer: slint::Timer::default(),
        });
    });

    buddy.show()?;
    if let Some(tray) = &tray {
        // No tray host on this desktop is no reason not to start.
        let _ = tray.show();
    }
    if let Some(token) = stored {
        with_app(move |app| app.connect(server, token));
    }

    #[cfg(debug_assertions)]
    debug::listen();

    slint::run_event_loop_until_quit()?;
    APP.with(|cell| cell.borrow_mut().take());
    Ok(())
}

fn wire(buddy: &BuddyWindow, chat: &ChatWindow, tray: Option<&Tray>) {
    // --- contact list ---
    let weak = buddy.as_weak();
    buddy.on_connect(move || {
        let Some(buddy) = weak.upgrade() else { return };
        let (server, token, keep) = (buddy.get_server().to_string(), buddy.get_token().to_string(), buddy.get_keep_token());
        with_app(move |app| {
            app.keep_token = keep && token_store::available();
            app.connect(server, token);
        });
    });
    buddy.on_filter_edited(|text| {
        let text = text.to_string();
        with_app(move |app| {
            app.filter = text;
            app.show_lists();
        });
    });
    buddy.on_open(|key| {
        let key = key.to_string();
        with_app(move |app| app.open_by_key(&key));
    });
    buddy.on_heart(|key| {
        let key = key.to_string();
        with_app(move |app| app.toggle_favorite(&key));
    });
    buddy.on_new_group(|| with_app(|app| app.new_group()));
    buddy.on_sound_toggled(|on| with_app(move |app| app.set_sound(on)));
    buddy.on_notifications_toggled(|on| with_app(move |app| app.set_notifications(on)));
    buddy.on_stay_toggled(|on| with_app(move |app| app.set_stay(on)));
    buddy.on_scheme_chosen(|scheme| with_app(move |app| app.set_scheme(Scheme::from_index(scheme))));
    buddy.on_hardware_toggled(|on| {
        with_app(move |app| {
            app.settings.hardware_acceleration = on;
            app.settings.save();
        })
    });
    buddy.on_sign_out(|| with_app(|app| app.sign_out()));
    buddy.window().on_close_requested(|| {
        let mut stays = false;
        APP.with(|cell| {
            if let Ok(app) = cell.try_borrow() {
                stays = app.as_ref().is_some_and(|app| app.settings.stay_in_tray && app.tray.is_some());
            }
        });
        if !stays {
            with_app(|app| app.quit());
        }
        slint::CloseRequestResponse::HideWindow
    });

    // --- tray ---
    if let Some(tray) = tray {
        tray.on_show_contacts(|| with_app(|app| app.show_contacts()));
        tray.on_sound_toggled(|on| with_app(move |app| app.set_sound(on)));
        tray.on_notifications_toggled(|on| with_app(move |app| app.set_notifications(on)));
        tray.on_stay_toggled(|on| with_app(move |app| app.set_stay(on)));
        tray.on_quit(|| with_app(|app| app.quit()));
    }

    threads::wire(chat);
    layers::wire(chat);
}

impl App {
    fn show_contacts(&mut self) {
        let _ = self.buddy.show();
    }

    fn set_sound(&mut self, on: bool) {
        self.settings.sound = on;
        self.settings.save();
        self.buddy.set_sound(on);
        if let Some(tray) = &self.tray {
            tray.set_sound(on);
        }
        if on {
            platform::sound::new_message();
        }
    }

    fn set_stay(&mut self, on: bool) {
        self.settings.stay_in_tray = on;
        self.settings.save();
        self.buddy.set_stay(on);
        if let Some(tray) = &self.tray {
            tray.set_stay(on);
        }
    }

    pub(crate) fn set_scheme(&mut self, scheme: Scheme) {
        self.settings.scheme = scheme;
        self.settings.save();
        self.buddy.set_scheme(scheme.index());
        self.buddy.global::<Theme>().invoke_choose(scheme.index());
        self.chat.global::<Theme>().invoke_choose(scheme.index());
    }

    fn set_notifications(&mut self, on: bool) {
        self.settings.notifications = on;
        self.settings.save();
        self.buddy.set_notifications(on);
        if let Some(tray) = &self.tray {
            tray.set_notifications(on);
        }
    }
}
