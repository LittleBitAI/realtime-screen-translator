---
scope: project
severity: preference
triggers: []
domain: ''
title: "You can install the comic app on Windows and open it from the Start menu."
pr: 7
merged: 2026-11-18
branch: "deliver-windows-installer-exe"
---

# You can install the comic app on Windows and open it from the Start menu.

What. It provides a 64-bit Windows installer containing features such as opening web comics, importing folders, a saved list, and a new desktop screen. Once installed, the app is registered in the Start menu. - Location: `e2e/receipts/windows-installer/verification.txt` - The build method has been documented so that the same installer can be recreated. The provided installer is version 0.1.0 and does not have a digital signature. …

Why. In the comic app, you can open comics via web addresses or import image folders as a chapter. You can add imported chapters to a list to read them again later. It eliminates the need for users who want to read comics to download source code or install development tools. It has been verified that chapters can be reopened from the saved list even after restarting the installed app.

Source. PR #7 · `deliver-windows-installer-exe`
