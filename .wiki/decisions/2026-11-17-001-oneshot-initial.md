---
scope: project
severity: contract
triggers: ["화면", "vision", "프레임", "스크린", "캡처", "공유"]
domain: vision
title: "I first added an app that reads Japanese manga screens in real-time and overlays Korean translations on top of speech bubbles."
pr: 1
merged: 2026-11-17
branch: "oneshot-initial"
---

# I first added an app that reads Japanese manga screens in real-time and overlays Korean translations on top of speech bubbles.

What. By comparing the current reading screen with the previous screen as a small black-and-white thumbnail, it determines whether the user is scrolling vertically or flipping pages forward and backward. If scrolling, it stacks the translated version as a single image stitched from the screens passed; if flipping, it stacks them as individual page images. A translated version is created each time translation is turned on and is stored only within the device. - Location: `src-tauri/src/tracker.rs`'s `best_shift`, `src-tauri/src/backend.rs`'s `frame`, `src-tauri/src/backend. …

Why. This app continuously reads the screen while reading Japanese manga on a mobile phone (based on Galaxy S21) or Windows PC, covers the Japanese in speech bubbles with a white background, and draws the Korean translation on top. Users can turn translation on and off on the app screen, export the collected translated versions as image files while reading, and enter their own Gemini API key to be used for translation. It allows people reading foreign language manga to read dialogue in Korean on the spot without switching between apps, while preventing increased requests and costs from repeatedly sending the same speech bubbles, and avoiding the issue of covering illustrations by translating sound effects as well. The screen colors followed the 'ink-colored paper' palette (ink-colored buttons, light paper-colored background, blue accent color) chosen by the owner during the design review.

Source. PR #1 · `oneshot-initial`
