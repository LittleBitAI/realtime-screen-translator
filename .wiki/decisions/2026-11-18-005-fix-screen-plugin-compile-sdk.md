---
scope: project
severity: contract
triggers: ["화면", "vision", "프레임", "스크린", "캡처", "공유"]
domain: vision
title: "The screen plugin is compiled with Android 36 to build a release APK."
pr: 5
merged: 2026-11-18
branch: "fix/screen-plugin-compile-sdk"
---

# The screen plugin is compiled with Android 36 to build a release APK.

What. Increase the `src-tauri/plugins/screen/android/build.gradle.kts` of `compileSdk` from 34 to 36. `minSdk` remains at 26, and the app's `compileSdk` also remains at 37.

Why. Until now, only debug APKs were built in this repository. The release build failed at the `checkReleaseAarMetadata` stage of Gradle. > Dependency ':tauri-android' requires libraries and applications that depend on it to compile against version 36 or later of the Android APIs. :tauri-plugin-rst-screen is currently compiled against android-34. This failure was found during the wiki-agent deployment stage 1 rehearsal (wiki-agent PR #383).

Source. PR #5 · `fix/screen-plugin-compile-sdk`
