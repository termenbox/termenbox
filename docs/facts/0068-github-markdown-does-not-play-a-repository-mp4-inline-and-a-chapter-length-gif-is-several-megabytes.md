---
id: 0068
title: GitHub Markdown does not play a repository MP4 inline, and a chapter-length GIF is several megabytes
status: active
kind: observation
governed-by: 0161-course-videos-use-a-human-rhythm-model-a-still-poster-and-steps-generated-from-the-case
tags: [docs, video, github]
date: 2026-10-31
last-verified: 2026-10-31
---

# How a video shows in a repository Markdown page

## What is true

- **Measured.** The first rendered video, `recent-folder-phone` (17.4 s), is a 556 451-byte MP4 and a 1 779 640-byte GIF (docs/media on PR #427). The GIF is three times the MP4.
- **Extrapolated, not measured.** At the same rate a 45 to 90 s chapter would be a GIF of about 5 to 10 MB. Re-measure on the first real chapter render.
- **GitHub's documented behaviour, not re-tested here.** A relative link to an `.mp4` in a Markdown file opens the file's page; only videos attached through GitHub's uploader play inline in a README. Confirm on the first live course page and update this fact if it differs.

## Why it matters

A chapter's poster must be an image that links to the MP4 (decision 0161); an autoplaying GIF would weigh down the page and the repository on every re-render.
