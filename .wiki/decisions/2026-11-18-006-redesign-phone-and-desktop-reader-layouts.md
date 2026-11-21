---
scope: project
severity: preference
triggers: []
domain: ''
title: "Find and reopen comics saved on mobile phones and Windows more easily."
pr: 6
merged: 2026-11-18
branch: "redesign-phone-and-desktop-reader-layouts"
---

# Find and reopen comics saved on mobile phones and Windows more easily.

What. On mobile phones, tapping a list name expands the work directly below it. In the wide window on Windows, changing the list on the left displays the corresponding work on the right, while in a narrow window, it switches to a single column. - Location: `src/screens/Library.svelte`, `src-tauri/src/app.rs` - Buttons for opening comics, translations, and settings are placed at the bottom of the screen on mobile phones, and at the top in the wide window on Windows. When scrolling the screen, margins are provided so that this button bar does not obscure the items being manipulated. ...

Why. You can read comics in the app by entering the comic page address or importing an image folder, and translations are displayed above speech bubbles. On the translation screen, you can organize saved works into lists and reopen them by searching by title or address. Previously, you had to reopen the selection box every time you moved to a different list while browsing works. By displaying list names on the screen and placing the address input and open buttons close together, the operations needed at the moment of finding a work and starting to read it are immediately visible.

Source. PR #6 · `redesign-phone-and-desktop-reader-layouts`
