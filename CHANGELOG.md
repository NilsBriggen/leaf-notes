# Changes

Each release on GitHub shows its section from this file. Downloads are on the [releases page](https://github.com/NilsBriggen/leaf-notes/releases): `.AppImage` for Linux, `.msi` for Windows, `.apk` for Android.

## 0.3.0 — 2026-11-05

A big round for phones and tablets, plus new pen and recording features.

**Phones and tablets**
- Every screen fits a phone: Settings, the welcome tour, Study, the page finder and Help. The AI assistant fills the phone screen.
- Android's Back button closes whatever is open, then leaves the app.
- Pages zoom to fit the screen width when you first open them on a phone, and new notes are never wider than the paper.
- On a phone turned sideways the toolbar folds away, so the page gets the room.
- Palm rejection: a hand resting on the screen no longer stops the pen from drawing.
- Help explains the gestures and says "long-press" instead of "right-click" on touch screens.

**New**
- Tap with two fingers to undo, three to redo.
- Scribble back and forth over ink to erase it (can be turned off in Settings ▸ Pen & input).
- Take a photo straight onto the page (Android). Pictures are stored upright and at a sensible size.
- Selected ink has its own Delete and Duplicate buttons, so it works without a keyboard.
- Recordings play at 0.75× to 2× speed.
- Search and the page finder forgive typos, and search also finds words in your handwriting.
- Tapping just above or below a note edits it instead of putting a new note on top.

**Fixes**
- Saving is safer if the device switches off suddenly. If an earlier version left an empty notes file behind, the app starts normally again and brings back the pages it still finds.
- Exports (PDF, Markdown, backups) open the share menu on Android, and a copy is kept in Downloads/Leaf Notes.
- Smaller title bar, toolbar and page header on touch screens.

## 0.2.3 — 2026-11-05

- The title bar shows the same, more detailed logo as the app icon.

## 0.2.2 — 2026-11-05

- The paper fills the whole page card, and panning stops at its edge instead of showing a blank border.

## 0.2.1 — 2026-11-05

- More room to write: a slimmer page header with a clear edge where the paper starts.
- One set of pens: your pens, a highlighter group and the pen's settings, clearly labelled.
- Recordings can always be removed from their ⋯ menu, however narrow the window.
- Android: no more freezing when the screen rotates or dark mode switches.
- Typing with the on-screen keyboard keeps your note in view; tapping a page with a recording starts a note again.
- Many smaller layout, dark mode and wording fixes.

## 0.2.0 — 2026-11-04

- A short welcome tour, a help page and a feedback button.
- Page history: bring back an earlier version of any page (⋯ ▸ Page history).
- Lock a section with a password; its pages are encrypted.
- Restore a backup and import Markdown files and folders (Settings ▸ Your data).
- Pages changed on two devices can be compared side by side.
- Notes are stored one file per page: faster with big notebooks and safer.
- Search finds words in handwriting and PDF printouts.
- Recordings can be turned into text with your AI key.
- On phones the side panels fold away; better pen handling and accessibility.
- Android: pick files, "Share to Leaf Notes" from other apps, open attachments, and follow the system language.

## 0.1.2 — 2026-11-04

- The interface is available in English, German and French.
- Android: the app has its launcher icon, and the top of the app no longer sits under the status bar.

## 0.1.1 — 2026-11-04

- Android: the app keeps clear of the system bars.
- With finger drawing off, touching the page with a stylus picks up the pen by itself.

## 0.1.0 — 2026-11-04

The first beta.

- Notebooks, sections and pages, with favourites, tags, a task list, search and a quick page finder.
- Type anywhere on a page, or write with a pen: pressure, smoothing, highlighters, shapes that snap into place, and paper backgrounds.
- Handwriting to text and to LaTeX, offline.
- PDF printouts to write on, and PDF export.
- Audio recordings linked to what you wrote.
- Flashcard decks with spaced repetition.
- AI with your own key, and sync through Google Drive, a synced folder or WebDAV.
