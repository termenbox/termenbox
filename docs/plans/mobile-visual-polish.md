# Mobile visual polish

Status: issues agreed, implementation not started. Owner decisions recorded 2026-10-20.
Scope: the phone layout at 720 px and below. Desktop layout is out of scope.

This file records the visual problems seen on a phone and the direction agreed for
fixing them. It is not a commitment to a single pull request yet.

## Constraints

- Keep every key in the configured terminal key row. Do not solve the height
  problem by removing keys.
- Keep the current “Termenbox” bottom sheet. Do not replace it with a left drawer
  or a top sheet.
- Keep the 44 px touch target for controls, except the deliberate dense key-row
  height described below.
- `AppShell` remains the only owner of the app viewport. Use
  `visualViewport`, `--app-height` and the safe-area insets there; do not add a
  second viewport calculation in a screen.
- Never call a Chromium emulation run a phone check. Real-device checks are
  labelled by platform (`a-phone-check-names-its-platform`); Android still has no
  real-device check (fact 0039).

## The seven issues

### 1. Servers page is vertically centred and its cards crowd on a phone

**Observed.** `ServerList.svelte` centres the whole page vertically
(`justify-content: center`) and uses 48 px top/bottom padding. With one server the
heading and card float in the middle of the screen instead of starting below the
top bar. On a narrow screen the 48 px icon, the name/address/state block and the
action buttons share one flex row; the actions wrap, but the card still becomes
tall and its right side feels cramped.

**Agreed direction.**

- Top-align the page below the top bar. Keep the 600 px max width and horizontal
  centring; remove the vertical centring and reduce the top padding.
- Let the card actions wrap as one group on a narrow screen instead of squeezing
  the text column.
- Keep the 44 px minimum for Pause/Resume, Retry, share and remove.
- Keep the empty state readable; it does not need to be vertically centred on a
  phone either.

**Files.** `packages/client/src/ServerList.svelte`.

**Acceptance.** At 390 × 844 and 320 × 568 the heading is visible without
scrolling, the page has no horizontal overflow, every action is reachable, and
the server name/address still truncate instead of pushing the buttons off screen.

### 2. The dense terminal key row takes too much height

**Observed.** The main key row uses `h-11` (44 px) for every key. With the row’s
padding it takes about 50 px from the terminal. The owner wants the keys kept but
the row slightly shorter.

**Agreed direction.**

- Make the configured terminal row 36–38 px high.
- Keep the full set of keys; keys that do not fit still go under More.
- Keep the fixed ACP prompt row at 44 px. The compact height applies only to the
  dense terminal row, not to the 9-key ACP row.
- Keep the More panel and its Close button at 44 px.
- Keep labels readable and preserve the existing pressed/modifier states.

**Files.** `packages/client/src/KeyRow.svelte`; the ACP row is rendered from
`packages/client/src/AcpPrompt.svelte`.

**Acceptance.** At 390 px the same keys (or More) are present, no label is
clipped, the ACP prompt row is still 44 px, and the terminal gains the saved
height. The deliberate 36–38 px dense row is an exception to the 44 px height
rule, not a change to the touch target of other controls.

### 3. Top bar: Sessions button and connection indicator squeeze the tabs

**Observed.** The top bar contains the 44 px menu button, a flexible tab strip,
the connection indicator and the Sessions button with its count. On a narrow
phone the two right-hand controls take a large fixed share of the width, so the
tab strip has little room and the indicator is an unexplained dot between the
tabs and Sessions.

**Agreed direction.**

- Keep both controls in fixed slots on the right; let the tab strip take the
  remaining width and scroll horizontally.
- Make the Sessions button compact without dropping its accessible name or the
  needs-you count.
- Keep the connection indicator as a compact status control; its full text stays
  in the accessible label and the existing tap toast.
- Do not wrap the top bar onto a second row.

**Files.** `packages/client/src/NavigationBar.svelte`,
`packages/client/src/TabStrip.svelte`.

**Acceptance.** At 320 and 390 px the active tab is still reachable by horizontal
scroll, the Sessions count is not clipped, both right-hand controls keep a 44 px
target, and the top bar stays one row.

### 4. The “Termenbox” bottom sheet overflows the phone viewport

**Observed.** `MenuSheet.svelte` opens a bottom sheet with
`margin: auto auto 0` and `max-height: calc(var(--app-height, 100dvh) - 16px)`.
The inner menu has no safe-area bottom padding and the title row scrolls away
with the content. With Settings expanded, or with the soft keyboard open, the
last actions can sit under the home indicator or outside the visible area.

**Agreed direction.**

- Keep the existing bottom-sheet pattern and the “Termenbox” title.
- Constrain the sheet to the visible viewport, including the safe-area insets.
- Make the sheet body the scroll container; keep the title/close row sticky.
- Add bottom padding for `env(safe-area-inset-bottom)` so Log out remains
  reachable.
- Keep the 44 px rows and the existing focus return when the sheet closes.

**Files.** `packages/client/src/MenuSheet.svelte`; viewport values from
`packages/client/src/AppShell.svelte`.

**Acceptance.** At 390 × 844 and 320 × 568, with Settings expanded, the header
stays visible while the body scrolls, the last action can be reached, there is no
horizontal overflow, and closing the sheet returns focus to the terminal/menu
button as before.

### 5. Decision view: the Screen block and composer compete for height

**Observed.** `SessionView.svelte` makes the whole middle area scroll, gives the
Screen `<pre>` a `40vh` maximum, and keeps the composer outside the scroll. On a
phone a long decision plus the screen preview can push the reply field and the
“Open full terminal” button far down; the screen preview can dominate the view.

**Agreed direction.**

- Keep the decision first: the question and its options must be visible before
  the screen preview.
- Make the Screen block a compact preview on a phone with its own scroll, rather
  than a 40vh block that competes with the composer.
- Keep the composer docked at the bottom and keep “Open full terminal” reachable.
- Do not add a nested full-page scroll or a second fixed viewport.

**Files.** `packages/client/src/SessionView.svelte`,
`packages/client/src/DecisionPanel.svelte`.

**Acceptance.** At 390 × 844 a long decision shows its options before the screen,
the composer stays visible without scrolling the page, and the screen preview
still scrolls on its own. The full terminal remains one tap away.

### 6. The input dock is too tall

**Observed.** The single-line composer uses a 48 px field (`h-12`), a 44 px send
button, form padding and dock padding with the safe-area inset. Together with the
key row this takes a large part of the terminal on a phone.

**Agreed direction.**

- Compact the single-line dock by reducing its vertical padding and field height,
  while keeping the send control at least 44 px in its tap dimension.
- Keep the 16 px input text size so iOS does not zoom into the field
  (fact 0032).
- Keep the safe-area bottom padding when the keyboard is closed; the shell
  already replaces it with the keyboard area when the keyboard is open.
- The key-row height decision above is separate; do not count it twice.

**Files.** `packages/client/src/lib/ds/Composer.svelte`; ACP multiline prompt
layout in the same component.

**Acceptance.** The terminal gains vertical space at 390 × 844, the send button
and field remain easy to hit, iOS does not zoom, and the dock stays above the key
row with the keyboard open and closed.

### 7. Terminal touch scrolling is too fast and jumpy

**Observed.** The terminal uses xterm’s defaults: `smoothScrollDuration: 0` and
`scrollSensitivity: 1`. `TouchScroll` turns a drag into wheel events and adds a
fling with a fixed 0.94 decay, with no cap on the initial velocity. On a phone a
short flick can jump much further than the finger moved.

**Agreed direction.**

- Set xterm `smoothScrollDuration` to about 100–150 ms.
- Set `scrollSensitivity` to about 0.5.
- Tune or cap the fling so a flick does not overshoot; keep the existing tap to
  click, selection and pinch behaviour.
- Do not change the alternate-screen/mouse-reporting path; the terminal still
  sends wheel events to apps that ask for them.

**Files.** `packages/client/src/Terminal.svelte`,
`packages/client/src/lib/terminal/touchScroll.ts`, and the touch e2e in
`packages/client/e2e/touch.mjs`.

**Acceptance.** A one-row drag moves about one row; a short flick does not fly
past the intended text; tap-to-click and long-press selection still work. The
browser e2e covers the gesture path, but the feel must be checked on a real
iPhone and a real Android phone before this is called done.

## Cross-cutting dependency: keyboard detection and the bottom bar

This is not a separate visual item, but it affects items 3, 5 and 6.

**Observed.** `App.svelte` sets
`keyboardOpen = innerHeight - visualViewport.height > 150`. The viewport meta uses
`interactive-widget=resizes-content`; on Chrome Android that can shrink both
`innerHeight` and the visual viewport, so the difference can stay below 150 and
the keyboard is reported closed. `AppFrame` then keeps the bottom bar visible
inside a session, and `App.svelte` keeps the pane switch visible when it should
give the space to the terminal.

**Direction.**

- Use one keyboard signal derived from the box `AppShell` already measures; do
  not keep a second independent calculation in `App.svelte`.
- Treat the keyboard as open when the visible box has shrunk from its resting
  height and an editable element has focus, not only when the two viewport
  heights differ.
- Re-measure on `visualViewport` `resize` and `scroll`, window `resize`, focus
  and blur, and orientation change.
- Keep the existing guardrail: the key row and composer never go under the
  keyboard (`soft-keyboard-never-hides-input`).

**Files.** `packages/client/src/App.svelte`,
`packages/client/src/shell/AppFrame.svelte`,
`packages/client/src/AppShell.svelte`,
`packages/client/src/lib/viewport.ts`,
`packages/client/index.html`.

**Acceptance.** A browser e2e that shrinks the visual viewport inside a session
hides the bottom bar and keeps the key row/composer visible; the same shrink
outside a session does not move the page under the finger. This is still not a
real-device keyboard check.

## Proposed pull-request split (not agreed)

Three changes were proposed; the owner has not chosen a split yet.

1. **Chrome:** issues 1, 3 and 4, plus the bottom-bar visibility part of the
   keyboard dependency.
2. **Keyboard and input:** issues 2 and 6, plus keyboard detection.
3. **Terminal scroll:** issue 7.

The alternative is one pull request. Do not start the split until the owner
chooses; the existing PR #251 stays on the soft-keyboard input fix and must not
carry visual-polish work.

## Verification

- Unit tests for the keyboard signal, the key-row mode/height and the composer
  layout where the logic is testable.
- Chromium e2e at 390 × 844 and 320 × 568 for the layout, the menu scroll and the
  terminal touch path. State clearly that this is emulation.
- Real-device pass on iPhone Safari/PWA and Android Chrome before calling the
  visual work done. Record the platform with the result.
- Before/after screenshots on the same viewport for each issue.

## Out of scope

- Removing keys or changing the default key set.
- A left drawer or a top sheet.
- Native apps or paid services.
- Desktop layout and the desktop split pane.
- A version bump or release tag.
