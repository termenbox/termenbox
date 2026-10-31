---
id: course-chapters-keep-the-fixed-anatomy-and-generated-steps
title: Every course chapter has the fixed parts, at most 250 words of prose and a numbered-steps block generated from its recorded case
status: active
governed-by: 0160-the-user-manual-is-a-guided-course-with-one-video-per-chapter-and-the-guide-stays-as-reference
grounded-in: [0160-the-user-manual-is-a-guided-course-with-one-video-per-chapter-and-the-guide-stays-as-reference, 0161-course-videos-use-a-human-rhythm-model-a-still-poster-and-steps-generated-from-the-case, ux-scenarios-assert-a-state-before-they-capture-it]
derivation-note: A newcomer follows the steps literally, so a label that differs from the screen stops them; and a chapter that grows past a screen or two is no longer a chapter. Both are mechanical to check, so they are checked instead of reviewed.
tags: [docs, onboarding, video, ci]
---

# Guardrail: A chapter keeps its shape

## Rule
Every file in `docs/course/` that is a chapter **MUST** contain, in order: a one-line promise with a time, the video poster block, the generated steps block, an "If it doesn't work" box with two entries, a "You can now" check and a Next link; its prose outside the generated block **MUST NOT** exceed 250 words.
The steps block **MUST** be generated from the chapter's recorded case between `<!-- course-steps:<case> -->` markers and **MUST** match the case's current step list; CI **MUST** fail on a stale block, a missing part, a chapter over the limit, a broken relative link, or a poster whose video or still is not in `docs/media/`.
A chapter **MUST NOT** name a control by any label other than the one on the screen in its video.
