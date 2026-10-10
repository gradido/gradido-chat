# Gradido Chat

A lean native desktop client for the chat of a Gradido community: Rust and
[Slint](https://slint.dev), one binary, no browser engine. It talks to the
same GraphQL API as the web frontend's chat and looks like the messengers of
old — a narrow contact list, conversations in tabs — in the colours of the
web chat.

## Start

```sh
cargo run --release
```

1. Sign in to your community in the browser.
2. Copy your token: developer tools → console →
   `JSON.parse(localStorage['gradido-frontend']).token`
3. Enter the community's address (e.g. `https://gradido.example`) and the
   token in the first page.

The token of a Gradido session is short-lived (ten minutes by default). Every
answer of the server brings a fresh one; the program takes it each time, so it
stays signed in while it runs, and keeps the newest one in the system's
keyring for the next start. After a longer pause the token has run out and a
fresh one has to be copied.

## What it does

Contacts, favourites and groups with unread marks · direct and group
conversations in tabs · sending, answering, editing, forwarding, copying ·
"also by e-mail" and group announcements · search in a conversation · pictures
and file links others sent · transfers and thank-you greetings in the thread ·
groups: create, members, moderators, rename, leave · mute · tray icon, sound
and system notification on a new message · ten languages · light and dark.

Not yet: sending pictures (and the picture editor), starting video calls,
signing in with e-mail and password. A video link somebody sent opens in the
browser.

Enter sends, Shift+Enter breaks the line.

The cog in the contact list opens the settings: light, dark or as the system
says; sound and notification; and hardware acceleration. That one is off by
default — the program then draws by itself, without the graphics driver and
its dozen threads — and applies from the next start. `SLINT_BACKEND`, where
set, overrules it.

## Server

The program follows the chat of Gradido's current `master`. Against the
release v3.1.0, which cannot answer a particular message yet, it asks
`/api/version`, recognises the build and leaves answering out; that exception
goes when the release with the feature is out (`COMMIT_WITHOUT_REPLIES` in
`src/api/mod.rs`).

## Platforms

Windows and Linux. Linux needs a Secret Service (GNOME Keyring, KWallet) for
the keyring and a StatusNotifierItem host for the tray icon; without them the
program runs, the token then lasts only as long as it does.

macOS, iOS and Android are prepared, not built: everything system-specific is
in `src/platform/`, each file with the arm to add; the credential stores are
listed in `Cargo.toml`.

Build requirements on Debian/Ubuntu: `libfontconfig-dev libxkbcommon-dev
libasound2-dev libdbus-1-dev`.

## Layout

| Path | What |
| --- | --- |
| `src/api/` | the GraphQL endpoint, the documents, the token |
| `src/core/` | chat logic without a window: threads, text, search, strings |
| `src/platform/` | keyring, notification, sound, settings, opening links |
| `src/app/` | the state between server and windows |
| `ui/` | the windows (Slint) |
| `locales/`, `assets/` | copied from the Gradido repository by `tools/sync-assets.py` (needs ImageMagick's `convert` for the app icon) |

`cargo test` runs the tests of `src/core/` and `src/api/`.

A debug build takes commands on standard input (`open first`, `type …`,
`send`, …; see `src/app/debug.rs`), and starts a session directly when
`GRADIDO_CHAT_SERVER` and `GRADIDO_CHAT_TOKEN` are set.

## License

GPL-3.0-only, see `LICENSE` and `NOTICE`.
