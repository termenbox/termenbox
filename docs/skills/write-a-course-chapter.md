---
id: write-a-course-chapter
title: Write one course chapter from a recorded case, with its story, trouble box and generated steps
status: active
date: 2026-10-31
tags: [docs, onboarding, video, procedural]
---

# Procedure: Write a course chapter

Use for each chapter of `docs/course/` (decision 0160), after its case renders and passes. The shape is held by `course-chapters-keep-the-fixed-anatomy-and-generated-steps`; the interview behind it is in `regenerate-the-user-course`.

1. **Pick the moment in the story.** The course follows one person: a long agent task started at a desk, finished from a train. Say, in one sentence, what they need at this point and why. If a step has no reason in the story, cut the step.
2. **List the case's steps once.** In `packages/client/e2e/video/cases.mjs` give the case `steps`: for each, the caption shown on screen and the exact label tapped. Captions are plain words, one idea each, no jargon that the course has not yet introduced.
3. **Promise.** One line with a real time, from the video's length plus a margin ("In 2 minutes your phone shows your server, connected.").
4. **Prose around the steps.** Two to four short paragraphs at most, in the second person and present tense, concrete: name the thing on the screen, say what it means in one clause, say what to expect next. Vivid beats thorough: one picture ("the dot turns green") is worth three sentences. Leave detail to the linked reference section.
5. **If it doesn't work.** Two entries, the likeliest failures first, each as symptom, cause, fix in one or two lines. Take them from `docs/guide/networking.md`, `install.md` and the case's own failure states; do not invent a failure you have not seen.
6. **You can now.** One or two observable outcomes the reader can check on their screen ("The server shows as Connected").
7. **Generate and check.** Run the steps generator and the course check (plan steps 2.2 and 4.4); fix every finding. Place the poster block with the media marker so the pick branch fills it.
8. **Read it aloud.** If a sentence needs a second reading, shorten it. If a word is not on the screen or in the story, replace it.
