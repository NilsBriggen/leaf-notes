# Leaf Notes

A notetaking app for Linux, Windows and Android, built for students: write with a pen or type, mix in maths, pictures and recordings, and study from flashcards. Everything works offline; AI and sync are optional.

![Leaf Notes](packaging/leafnotes.png)

## What it does

- **Notebooks › sections › pages**, with subpages, favourites, recent pages, page links (`[[Page name]]`) and backlinks, tags, a task list across all notes, search, and a quick switcher (Ctrl+K).
- **Free-form pages**: click anywhere to type. Notes can be text with Markdown-style formatting, checklists, tables, code, LaTeX equations, pictures, files and links. Type `/` for the insert menu.
- **Pen and ink**: pen, pencil, highlighter, eraser, lasso, and shapes that snap into place. Supports pressure and smoothing. Choose a background: plain, ruled, grid, dots, Cornell or music staves.
- **Handwriting to text and to LaTeX**, offline (a small built-in recognizer), or with your AI if you prefer.
- **PDF printouts**: put a PDF's pages on a page and write on them, then export as a PDF with your notes on top.
- **Audio recording linked to notes**: tap something you wrote to hear what was being said at that moment.
- **Flashcard decks** as their own page type, with spaced repetition. Make a deck by hand, from your notes, or with AI.
- **AI with your own key** (Anthropic, or any OpenAI-compatible server, including local ones): chat about a page, summarise, rewrite, make cards.
- **Sync** through Google Drive, a synced folder (Dropbox, OneDrive, Syncthing…) or a WebDAV server (Nextcloud…). Changes merge note by note; a page changed on two devices at once is kept in both versions.

## Download

Every push to `main` builds three files under **Actions ▸ Build ▸ Artifacts**:

| Platform | File |
|---|---|
| Linux | `LeafNotes-x86_64.AppImage`: make it executable (`chmod +x`) and run it |
| Windows | `LeafNotes-x86_64.msi`: installer |
| Android (arm64) | `LeafNotes-arm64.apk`: allow installing from this source |

Push a version tag (`git tag v0.1.0 && git push --tags`) to publish the three files as a GitHub release.

## Build from source

You need [Rust](https://rustup.rs). On Linux, also the ALSA headers (`libasound2-dev` / `alsa-lib-devel`).

```sh
cargo run --release
cargo test --release
cargo run --release -- --screenshot shot.png   # renders a preview picture and exits
```

Android (needs the Android SDK, NDK 27 and `cargo install cargo-apk`):

```sh
rustup target add aarch64-linux-android
export ANDROID_HOME=~/Android/Sdk ANDROID_NDK_ROOT=~/Android/Sdk/ndk/27.1.12297006
cargo apk build --release --lib
```

## Credentials (not in the repository)

| What | Locally | In GitHub Actions (repository secret) |
|---|---|---|
| Google Drive sign-in: the client secret of the Desktop OAuth client | `.cargo/config.toml` → `[env] LEAFNOTES_GOOGLE_SECRET = "…"` | `LEAFNOTES_GOOGLE_SECRET` |
| Android signing key | `android/dev.keystore` (password in `Cargo.toml`) | `ANDROID_KEYSTORE_B64`: the keystore, base64-encoded |

Without the Google secret, the app builds fine but its Google Drive option says it isn't set up. Without the keystore secret, CI signs the APK with a throwaway key, so it can't update a copy signed with your own key.

```sh
gh secret set LEAFNOTES_GOOGLE_SECRET            # paste the GOCSPX-… value
base64 -w0 android/dev.keystore | gh secret set ANDROID_KEYSTORE_B64
```

For Google Drive, the Google Cloud project also needs the **Google Drive API** turned on. While its consent screen is in "Testing", Google ends sign-ins after 7 days; publish the app (the `drive.file` scope needs no review) to avoid that.

## Where your notes live

- Linux: `~/.local/share/LeafNotes`
- Windows: `%APPDATA%\LeafNotes`
- Android: the app's private storage

Notes are `notebooks.json` (written safely: a temporary file, then a rename, with a `.backup` copy from before each session), plus `files/` for pictures, PDFs and recordings. Settings ▸ Your data can save a full backup.

Your AI key, WebDAV password and Google sign-in are kept in the system keyring: Windows Credential Manager, or the Secret Service on Linux (GNOME Keyring, KWallet). The settings files keep an empty field. If no keyring is running, and on Android (which has no keyring this app can use), they stay in the app's own folder; on Android no other app can read that folder.

[Privacy policy](https://leafnotes.briggen.dev/privacy) · [Terms](https://leafnotes.briggen.dev/terms)

## Project layout

```
src/main.rs      start-up and --screenshot
src/lib.rs       the app: state, saving, AI jobs, recording, sync glue
src/chrome.rs    title bar, toolbar tabs, side panels, settings, dialogs
src/canvas.rs    the page: paper, notes, ink, deck editor
src/model.rs     notebooks, pages, notes, flashcards, saving
src/rich.rs      text formatting      src/math.rs   LaTeX rendering
src/hw.rs        offline handwriting recognition (models in assets/)
src/sync.rs      sync and merging     src/ai.rs     AI providers
src/audio.rs     recording/playback   src/export.rs PDF export
tools/handwriting/train.py   trains the handwriting models
```

## License

Leaf Notes is free software, licensed under the [GNU General Public License v3.0](LICENSE).
