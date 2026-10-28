---
id: 0088-the-start-screen-resumes-the-last-session
title: The start screen resumes the last session
status: superseded
superseded-by: 0100-home-is-a-decision-first-board
accepted-by: Sergey Strebulaev <s@vivantel.dev>
date: 2026-10-14
track: product
tags: [client, mobile]
fitness-functions: [none yet; resume is untested in e2e]
---

## Decision (draft: needs the owner's acceptance)

When the app opens it attaches the session the device had open, once its server is connected, unless the user already chose something or went to Servers. Detaching or closing that session forgets it. The id is kept per device in local storage.

## Why

"Choose a session to open its terminal" told the user nothing. Where they left off is the likely target.
