---
scope: project
severity: preference
triggers: []
domain: ''
title: "You can import an image folder as a single episode of a manga and read it in the app."
pr: 3
merged: 2026-11-18
branch: "import-folder-as-manga-chapter"
---

# You can import an image folder as a single episode of a manga and read it in the app.

What. It groups images directly inside the folder selected in the Windows and Android folder selection window as one episode. Numbers in file names are sorted by size, such as 1, 2, 10. - Location: `src/screens/Home.svelte`, `src-tauri/src/app.rs`'s `pick_chapter_folder`, `src-tauri/plugins/screen/android/src/main/java/ScreenPlugin. …

Why. This app displays Korean translations over speech bubbles while reading manga. Until now, the app read manga by opening web addresses. Keeping a copy on the device allows you to continue reading even after moving the original folder, and adding it to the list after the copy is finished prevents incomplete episodes from remaining when storage space is insufficient. Reusing previous translations reduces request costs and waiting time when reading the same image.

Source. PR #3 · `import-folder-as-manga-chapter`
