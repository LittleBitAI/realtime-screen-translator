---
scope: project
severity: preference
triggers: []
domain: ''
title: "Created the repository wiki for the agent for the first time (no changes in behavior)"
pr: 2
merged: 2026-11-18
branch: "wiki-bootstrap"
---

# Created the repository wiki for the agent for the first time (no changes in behavior)

What. Added an overview document summarizing in one page that the purpose of the repository, the current files, and the methods for building, running, and testing have not yet been decided. - Location: `.wiki/project.md` - Transferred the product requirements written in the introduction document into five rules. …

Why. This repository is a place to build a program that translates Japanese manga visible on mobile and PC browser screens into Korean in real time and overlays it. There is not a single line of program code yet, only an introduction document. The wiki is a collection of short documents that the coding agent working in this repository reads before starting a task. By writing down the product requirements and the fact that there are no inspection commands yet before the code is created, it reduces the risk of the agent missing requirements such as duplicate translation or sound effect translation, or reporting that non-existent tests have passed.

Source. PR #2 · `wiki-bootstrap`
