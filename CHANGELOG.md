# Changes

Each release on GitHub shows its section from this file. Downloads are on the [releases page](https://github.com/NilsBriggen/leaf-notes/releases): `.AppImage` for Linux, `.msi` for Windows, `.apk` for Android.

## 0.3.2 — 2026-11-11

A big round of fixes for safety and reliability, and many smaller improvements.

**New since the third release candidate**
- Creating a new notebook no longer crashes the app.

**New since the second release candidate**
- Locking a section on one device while another still had unsynced changes in it no longer loses those changes: they're kept in "… (other device)" next to it.
- A lock that arrives through sync also clears the old plain text from this device's page history, search and backups; locking does the same for older backups. Flashcards written inside notes no longer keep their questions readable in the library.
- Notes are as tall on a phone as on a laptop, so pages written on one no longer overlap on the other.
- Undo: a recording you just stopped stays when you undo; typing in checklists, tables, code, maths and flashcards can be undone; Ctrl+Z skips steps that changed nothing. A recording whose Stop fails is recovered.
- Ctrl+C, Ctrl+X and Ctrl+V work on selected ink. Clicking into a note puts the cursor where you clicked.
- Formatting: list buttons keep sub-points and skip blank lines, italics inside bold work, numbered lists start at the number you typed, and ticking a box only ticks the box at the start of the line.
- Phones and small windows: the tour, the Home toolbar, the shortcuts list and the side panel fit; Android Back closes the password and microphone prompts; the crash report can wait ("Not now").
- "Delete forever" in Recently deleted asks twice; studying has an Undo button; the status bar says "Not saved" when nothing is saved; AI quick actions ask in your language.
- Fixed: half-copied pictures being synced, closing the app hanging on a slow connection, microphones below 16 kHz recording gaps of silence, and long AI answers being cut off.

**New since the first release candidate**
- The Draw toolbar keeps one height whatever is in hand: switching to the eraser with a pen's button no longer makes the page jump.
- Google Drive sign-in on Android finishes once you're back in Leaf Notes (it failed with "No address associated with hostname"). If the browser can't get back to the app, paste its address into Settings.
- Phones: page titles wrap, the page list uses the screen's width, Settings controls sit under their text, Ctrl+K stays above the keyboard, and the toolbar fits the screen.
- Names that don't fit end in "…"; dark ink and highlighted words are readable in dark mode; all windows look alike.
- Fixed: the eraser at low zoom erasing strokes it never touched; undo after nudging a note or adding a table row; Ctrl+Z while studying; Ctrl+K opening the wrong page; sync stopping after closing Settings with Esc; pages deleted before locking a section staying readable; memory growing with long PDFs.
- Android: the camera works on Android 8 and 9, a photo survives the app being stopped, and sharing big files no longer freezes the app.

**Your notes are safer**
- Closing the app while a locked section was open could overwrite that section with its lock screen. Fixed.
- Reopening the welcome tour from Help and choosing "Start with an empty notebook" no longer removes your notebooks (and on a first start the sample goes to Recently deleted).
- A missing or damaged notes file no longer leads to pages being deleted: the app uses its backup and gathers loose pages into "Recovered pages".
- Pages of locked sections stay encrypted in Recently deleted, in the safety copy made before joining sync, and in the search index.
- Only one Leaf Notes window saves at a time; a second one shows your notes without saving.
- A recording cut off by a crash or a flat battery is finished and put back on its page at the next start.
- Daily backups (the last 7) in the backups folder; Recently deleted asks before emptying, and restored items go back to their old place.
- Sync: one file that won't transfer no longer stops your notes from syncing, and the sync state is only saved once the notes are.

**Fixes**
- The eraser catches straight lines, boxes and fast strokes anywhere along them.
- Undo stays on the page you're looking at; moving, resizing, ticking and retagging notes can be undone.
- Highlighting works on top of printed PDF pages.
- Dates and times are in your time zone; Esc closes windows; the theme stays when the system switches between light and dark.
- Renaming a page keeps the links to it working; a copied deck has its own study progress; "$5 and $6" is no longer read as maths.
- Many smaller fixes to checklists, headings, two-finger taps, scribble-to-erase and shape snapping.

**Writing**
- Tap a printed PDF page to type on it; tick boxes in a note without opening it.
- Colour presets and recent colours, a pressure setting, and a tool menu in the status bar.
- Lasso: copy, cut and paste ink, also onto another page.
- Handwriting turned into text is shown first, so you can correct it.
- Lines snap level, Tab indents lists, maths previews as you type, and unknown LaTeX commands are marked.

**Organising and studying**
- Back and forward between pages; Ctrl+K also runs commands; pages reopen where you left them.
- Search can stay in one notebook or section and scrolls to the match.
- The task list shows notes tagged To Do and sorts by @dates.
- Review includes new cards, can undo a rating, shuffles, and says when cards come back.
- Make flashcards from any note, add many cards from text, and use page templates.
- Drag pages to reorder them or onto another section; recent pages in the side panel.

**Around the page**
- The assistant reads your handwriting, can be stopped, and can put answers below the note you're writing.
- Sync settings wait for "Connect"; the status bar says when changes are waiting.
- Locked sections close again after a while without use.
- Update notices show what's new and link the download directly.
- Android: recording keeps going with the screen off, and export works on Android 8 and 9.

## 0.3.1 — 2026-11-05

- The eraser has its own settings while it's in hand: a size slider, and "Part of a stroke" or "Whole stroke" as one clear switch (no more clicking it twice to switch).
- The highlighter has a size slider and can take any colour.
- Pens can be removed with a visible button in Pen settings, not only by right-click or long-press.
- Recording speed is picked from a short menu instead of tapping through the speeds.
- A visible ⌃ button hides the toolbar (clicking the open tab still works too).
- View ▸ Dark mode now sticks when the theme was set to follow the system.

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
