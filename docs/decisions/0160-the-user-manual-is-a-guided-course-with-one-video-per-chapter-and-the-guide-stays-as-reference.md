---
id: 0160-the-user-manual-is-a-guided-course-with-one-video-per-chapter-and-the-guide-stays-as-reference
title: The user manual is a guided course in docs/course with one video per chapter, Start before Master, and the guide pages stay as the reference
status: active
date: 2026-10-31
track: product
accepted-by: Sergey Strebulaev
tags: [docs, onboarding, video, ux]
---

## Decision

- **A course, not a feature list.** The manual a newcomer meets is an ordered course in `docs/course/` (`01-install.md` and on), linked first from the README ("New here? Start the course"). It teaches by getting someone to a working setup, one chapter at a time, with a task index ("How do I…") for returning users.
- **Start before Master.** Part 1, Start, has six chapters, about ten minutes in all: install, connect your phone, first workspace, talk to your agent, answer from the train, come back later. Part 2, Master (Terminal vs Agent view, scrollback search, the key row and soft keyboard, notifications, several servers, when it will not connect), follows as its own pull requests and is not part of Part 1's done-when.
- **One story.** A single story runs through the course: you start a long agent task at your desk, leave, and finish it from the train. Every step has a reason in that story; no chapter is a tour of controls.
- **Fixed anatomy.** Each chapter has the same parts and at most 250 words of prose: a one-line promise with a time ("In 2 minutes you will…"), the video, numbered steps that use the exact on-screen labels, an "If it doesn't work" box with the two likeliest failures and their fixes, a "You can now…" check, and a Next link. The guardrail `course-chapters-keep-the-fixed-anatomy-and-generated-steps` holds it.
- **The guide stays.** The six pages in `docs/guide/` remain the reference. Each chapter links to the reference section it summarises, and the reference pages link back to the chapter. Nothing is rewritten to make room.
- **Judged by a newcomer.** Part 1 is done only after a cold-read test: someone who has not seen Termenbox follows the course from the README on a real phone, with a stopwatch, and the stalls are recorded (`a-course-part-is-done-only-after-a-recorded-cold-read`). There is no telemetry.

## Why

Today the README and six reference pages describe behaviour and carry no video, no order and no first step. A newcomer has to know what a workspace and a tab are before the pages make sense, and nothing says what to do after the QR. A course gets them to a working phone first and explains the rest on the way. A fixed anatomy keeps chapters short and checkable; a story gives each step a reason, which is what a feature list lacks.

Not chosen: a flat task cookbook (no order, so the core ideas are missed), videos added to the reference pages (cheapest, but still teaches by describing features), rewriting the guide into the course (loses firewall, backend and safety detail that does not fit a path), a course on the website (a static landing page with a strict CSP and a separate deploy, so a much larger project), both parts before any release (a long wait and seven cases that do not exist yet).

## Consequences

- New `docs/course/` with an index, six chapters and a task index; the README gains a first-line entry; the reference pages gain back-links.
- Six recorded cases, one per chapter (`install`, `add-server`, `recent-folder`, `talk-to-agent`, `answer-from-the-train`, `resume-after-close`), each ending in an assertion of its final state (`ux-scenarios-assert-a-state-before-they-capture-it`).
- Videos reach the course through the review and pick path of decision 0158; the course adds no second path.
- Part 2 is planned separately when Part 1's cold read is in.
