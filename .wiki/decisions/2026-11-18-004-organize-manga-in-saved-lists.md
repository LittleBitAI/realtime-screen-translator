---
scope: project
severity: preference
triggers: []
domain: ''
title: "You can find and add to your list by using the manga link and the imported episode as the title."
pr: 4
merged: 2026-11-18
branch: "organize-manga-in-saved-lists"
---

# You can find and add to your list by using the manga link and the imported episode as the title.

What. Save the manga site address with a title. The link and imported episode can be found by part of the title, and the link can also be found by part of the address. When you press read in the installed app, the corresponding site or the episode stored on the device opens. - Location: `src/screens/Library.svelte`, `src-tauri/src/library.rs`, `src-tauri/src/store.rs` - Opening edit allows you to create a list, rename it, or delete it. …

Why. The translation screen collects the pictures saved while reading manga and the episodes imported from the picture folder. Until now, it was a place to browse works in the order they were saved or to export pictures. Even if works accumulate, you can find them again without needing to remember the last time you read them or the exact address. Since the list records only the order in which works were added, the content already translated does not disappear even if you organize the reading list.

Source. PR #4 · `organize-manga-in-saved-lists`
