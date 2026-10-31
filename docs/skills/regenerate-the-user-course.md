---
id: regenerate-the-user-course
title: Re-run the course interview and regenerate the user course when an answer changes
status: active
date: 2026-10-31
tags: [docs, onboarding, video, ux, procedural]
---

# Procedure: Regenerate the user course

Use when the course (decision 0160) or its videos (decision 0161) must change because an answer below no longer holds, or when the course is rebuilt for another audience. It is the record of the interview of 2026-10-31 that produced both decisions: each question, the options that were on the table, what the owner chose, and what to touch if a different option is chosen now. Ask the questions again, in this order, with the owner's earlier answer marked as the current one; do not ask a question whose dependencies did not change.

Run it with `/go-getter:roadmap`, put each decision to the owner with the recommendation first, wait for each answer, and write only after one explicit go-ahead. A session never answers for the owner.

## How to re-run

1. Read decisions 0158, 0160 and 0161, the plan `docs/plans/user-course.md` and the guardrails `course-chapters-keep-the-fixed-anatomy-and-generated-steps` and `a-course-part-is-done-only-after-a-recorded-cold-read`.
2. Re-gather the facts the interview started from (below); a changed fact can change an answer.
3. Walk the ledger top to bottom. For a question whose answer is unchanged, say so and move on. For a changed answer, update the artifacts in its last column, then the plan steps they feed.
4. Re-render affected videos and send them through the review and pick path of decision 0158. Re-run the cold read (`run-a-cold-read-test`) when the shape, the chapters or the anatomy changed.

## Facts the interview started from

- The README and six reference pages in `docs/guide/` (about 400 lines) describe behaviour; no video, no order, no first step.
- The recorder's delays were uniform random ranges: 0.6 to 1.2 s after each change, 55 to 140 ms per key, 18 to 30 pointer steps with a 0.12 to 0.35 s settle. The first video, `recent-folder-phone`, ran 17 s and was a 1.8 MB GIF plus a 556 KB MP4.
- GitHub Markdown does not play a repository MP4 inline (fact 0068).
- The installer runs in CI in a clean container (native installer jobs), so its real output can be captured.
- Seven of the cases a full course needs did not exist; `recent-folder`, `add-server` and `resume-after-close` did.

## The ledger

| # | Question | Options considered | Chosen | Why | If it changes, update |
|---|---|---|---|---|---|
| 1 | What shape does the manual take? | **Guided path** (ordered course plus a task index); task cookbook; reference pages plus videos | Guided path | A newcomer needs an order and a working setup first; a cookbook lets them miss the core ideas, videos on reference pages still teach by describing features | 0160; plan phase 4; the README entry |
| 2 | How far does the course go, and what ships first? | **Start now, Master next**; both parts in one go; Start only | Start now, Master next | A complete path early; Master needs cases that do not exist yet; Start only ignores "master" | 0160 scope; the plan's done-when; a Part 2 plan |
| 3 | How do videos get human-like delays? | **Rhythm model**; just make 1.6x the default; replay a real person's take | Rhythm model | Uniform delays look like a robot at any speed; a take per chapter and per UI change does not scale | 0161; plan phase 1; `lib.mjs`; render hashes |
| 4 | What does each chapter look like? | **Fixed anatomy with one running story**; fixed anatomy, no story; free-form | Fixed anatomy with a story | A fixed shape keeps chapters short, checkable and consistent; the story gives each step a reason | 0160; the anatomy guardrail; `write-a-course-chapter`; the course check |
| 5 | Where does the course live, and what happens to the guide? | **New `docs/course`, guide stays**; rewrite the guide into the course; course on the website | New `docs/course`, guide stays | Nothing existing is rewritten, so it ships in pieces; the guide keeps firewall, backend and safety detail; the site is a static page with a strict CSP | 0160; the README; reference back-links; plan phase 4 |
| 6 | How is a video shown on the page? | **Still poster linking to the MP4**; keep the GIF poster; host outside the repo | Still poster | A chapter GIF is 5 to 10 MB; GitHub plays no MP4 inline; an external host adds a dependency outside the pull request | 0161; 0158; `render.mjs`, the review page, `docs-media.mjs`; fact 0068 |
| 7 | Which chapters make Part 1? | **Six** (install, connect, first workspace, talk to your agent, answer from the train, come back later); add a seventh "it won't connect"; merge to five | Six | Matches the story of starting at a desk and finishing on a train; "it won't connect" belongs in Master and in each chapter's trouble box | 0160 list; the plan's cases; the course index |
| 8 | How do the steps stay in sync with the video? | **Steps come from the case** (generated block, stale fails CI); hand-written and reviewed; generate the whole chapter | Steps from the case | A renamed button should break the build, not the reader; whole-chapter generation is flat and CI may not call a model | 0161; `scripts/docs-course.mjs`; the case format; the anatomy guardrail |
| 9 | What proves Part 1 helps a newcomer? | **Cold read on a real phone**; developer review only; usage analytics | Cold read | The author already knows the product; analytics are ruled out by the security guardrails | 0160; the cold-read guardrail; `run-a-cold-read-test` |
| 10 | How is the Install video recorded? | **Replay captured real installer output**; record the installer live; no video | Replay captured output | Faithful text, deterministic, cheap to redo; live runs vary in ports, timing and QR | 0161; plan step for the fixture; a CI capture step |
| 11 | Chapter list (asked a second time; first reply was "go back to the previous question") | the same three as #7 | Six, confirmed | Confirmed after re-asking #6 | as #7 |

Inherited from decision 0158 (asked earlier in the session): publishing path, **simplify** (picked videos land in `docs/media/` through a pull request) over the strict chain (orphan branch, sha256 manifest, gated site deploy). See 0158 for its alternatives.

## Re-asking well

- One question at a time, at most four options with the free-text slot, the recommended option first and marked, each option's trade-off against the others.
- A decision the owner answered stays answered: confirm it in one line instead of re-litigating it, unless a fact above changed.
- Write the new answer into this ledger (replace the row; keep the old one in the decision's "Not chosen") and bump the dates of the decisions it touches.
