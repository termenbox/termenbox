---
id: 0027
title: What iOS fires on a swipe-typing correction in xterm's textarea
status: active
tags: [client, mobile, ios, terminal]
date: 2026-10-12
---

# What iOS fires on a swipe-typing correction in xterm's textarea

Captured on the owner's iPhone with the X.1 recorder (swipe "hello", pick the suggestion "he'll", type a space):

```
+0    beforeinput insertText data="hello" val="" sel=0-0
+18   input       insertText data="hello" val="hello" sel=5-5
+6308 beforeinput insertReplacementText data="he'll" val="hello" sel=0-5
+6312 input       insertReplacementText data="he'll" val="he'll" sel=5-5
+6327 beforeinput insertText data=" " val="he'll" sel=5-5
+6328 input       insertText data=" " val="he'll " sel=6-6
```

- A swipe inserts the whole word with one `insertText` and no composition events; the textarea keeps the word.
- Choosing another suggestion fires `insertReplacementText` with the old word selected (`sel=0-5`) and `getTargetRanges()` empty; the selection is the only record of what is replaced.
- xterm.js 5.3 forwards `insertText` but ignores `insertReplacementText`, so the terminal kept "hello" and only the space arrived. Verdict: rollback is feasible (decision of plan step X.2: send one DEL per selected code point, then the replacement).
- @xterm/xterm 5.5.0, which the client uses since #406, still ignores `insertReplacementText` (no handler in its source).
- The recorder uses `?debug` or five taps on the title; it listens in the capture phase because xterm hides some events from later listeners.
