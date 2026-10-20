<script lang="ts">
  import { onMount } from 'svelte';
  import { Terminal as XTermTerminal } from 'xterm';
  import { FitAddon } from 'xterm-addon-fit';
  import { WebglAddon } from 'xterm-addon-webgl';
  import { SerializeAddon } from 'xterm-addon-serialize';
  import 'xterm/css/xterm.css';

  import type { HubConnection, ServerState } from './lib/hub';
  import { b64ToBytes, textToB64 } from './lib/connection';
  import { applyMods, bufferText, clampFont, FONT_DEFAULT, newlineKeys, prefixBytes, prefixLabel, shiftSeq, type KeyProfile, type Mods } from './lib/keys';
  import { beep, bellEnabled, installAudioUnlock, setBellEnabled, unlockAudio } from './lib/bell';
  import { copyText, lastCopiedText, pasteText } from './lib/clipboard';
  import { attachInputLogger } from './lib/inputlog';
  import { attachInputMirror, attachSwipeSupport, type InputMirror } from './lib/swipe';
  import { DEBUG_KEY, debugEnabled } from './lib/viewport';
  import { KeyboardMode } from './lib/terminal/keyboardMode';
  import { crumb } from './lib/breadcrumbs';
  import { dropColorReplies, setEnterKeyHint, silenceColorQueries } from './lib/terminal/inputGuards';
  import { WordTracker } from './lib/terminal/wordTracker';
  import { nextVoiceLang, parseVoiceLang, pickLanguage, recognizerFactory, Voice, type VoiceLang } from './lib/terminal/voice';
  import { linkAt } from './lib/terminal/links';
  import { cellAt } from './lib/selection';
  import { PinchZoom } from './lib/terminal/pinch';
  import { InputBatcher } from './lib/terminal/inputBatch';
  import { TouchScroll } from './lib/terminal/touchScroll';
  import { FONT_FAMILY, terminalTheme } from './lib/terminal/theme';
  import { TouchSelection, type Selection } from './lib/terminal/touchSelection';
  import { settings } from './lib/settings';
  import { agents } from './lib/agents';
  import KeyRow from './KeyRow.svelte';
  import FnKeyboard from './FnKeyboard.svelte';
  import InputLog from './InputLog.svelte';
  import SelectView from './SelectView.svelte';
  import PasteSheet from './PasteSheet.svelte';

  export let connection: HubConnection;
  export let activeSessionId: string | null = null;
  export let connState: ServerState = 'connected';
  /** Called when keys are sent to the session, so the app can tell the user answered a prompt by hand. */
  export let ontyped: (() => void) | undefined = undefined;
  /** `tmux` adds the Prefix key to the key row; `prefix` is tmux's own notation for it (`C-b`). */
  export let profile: KeyProfile = 'shell';
  export let prefix = 'C-b';
  /** The last screen seen for this session (a peek): drawn dimmed until the live repaint replaces it. */
  export let preview: string | null = null;

  const FONT_KEY = 'termenbox_font';
  let mods: Mods = { ctrl: false, alt: false };
  let fontSize = clampFont(Number(localStorage.getItem(FONT_KEY)) || FONT_DEFAULT);
  let prefixArmed = false;
  let previewing = false;
  let selectText: string | null = null;
  let bell = bellEnabled();
  let fnOpen = false;
  let sel: Selection | null = null;
  let selStatus = '';
  let debug = debugEnabled(location.search, localStorage.getItem(DEBUG_KEY));
  const syncDebug = () => (debug = debugEnabled(location.search, localStorage.getItem(DEBUG_KEY)));

  let terminalContainer: HTMLDivElement;
  let term: XTermTerminal | null = null;
  let fitAddon: FitAddon | null = null;
  let webgl: WebglAddon | null = null;
  /** Keeps the hidden input equal to the word being typed, but only after xterm has read the bytes (see swipe.ts). */
  let inputMirror: InputMirror | null = null;
  let disposed = false;
  /** Everything to undo on teardown, in the order it was set up. */
  const cleanups: (() => void)[] = [];

  // --- controllers: each owns one concern and is tested on its own (src/lib/terminal) -------------------
  const pinch = new PinchZoom();
  const words = new WordTracker();
  const keyboard = new KeyboardMode({
    field: () => term?.textarea ?? null,
    isFocused: () => document.activeElement === term?.textarea,
    isSystemKeyboardUp: () => !!window.visualViewport && window.innerHeight - window.visualViewport.height > 150,
    onFnChange: (open) => (fnOpen = open),
  });
  /** The next `touchend` follows a gesture handled here (a tap or a scroll): keep the browser's own click away. */
  let suppressClick = false;
  const scroll = new TouchScroll({
    rowHeight: () => (terminalContainer?.querySelector('.xterm-screen')?.getBoundingClientRect().height ?? 0) / (term?.rows || 1) || 18,
    wheel: (dy, x, y) => {
      suppressClick = true;
      term?.element?.dispatchEvent(new WheelEvent('wheel', { deltaY: dy, deltaMode: WheelEvent.DOM_DELTA_PIXEL, clientX: x, clientY: y, bubbles: true, cancelable: true }));
    },
    tap: (x, y) => tapAt(x, y),
  });
  const touch = new TouchSelection({
    term: () => term,
    screenRect: () => terminalContainer?.querySelector('.xterm-screen')?.getBoundingClientRect() ?? null,
    hostRect: () => terminalContainer?.getBoundingClientRect() ?? null,
    onChange: (s) => (sel = s),
    vibrate: () => navigator.vibrate?.(12),
  });

  /** Runs once: the WebGL addon goes before the terminal (the other order throws), and nothing here may throw. */
  function teardown() {
    if (disposed) return;
    disposed = true;
    for (const undo of cleanups.splice(0).reverse()) {
      try {
        undo();
      } catch {
        /* already gone */
      }
    }
    try {
      webgl?.dispose();
    } catch {
      /* the renderer may already be gone */
    }
    try {
      term?.dispose();
    } catch {
      /* xterm 5.3 can throw while tearing down a renderer */
    }
    webgl = null;
    term = null;
    fitAddon = null;
  }

  const batch = new InputBatcher((data) => {
    if (activeSessionId) { connection.send({ type: 'input', data: textToB64(data) }); ontyped?.(); }
  });
  /** Not connected (or the server is not answering): keys are not queued, so the user is told instead of typing into the void. */
  $: offline = connState !== 'connected';
  let inputRefused = false;
  let refusedTimer: ReturnType<typeof setTimeout> | undefined;
  function refuseInput() {
    inputRefused = true;
    clearTimeout(refusedTimer);
    refusedTimer = setTimeout(() => (inputRefused = false), 1500);
  }
  function sendInput(data: string) {
    if (offline) {
      inputMirror?.sync();
      return refuseInput();
    }
    batch.add(data);
    // Every send path mirrors after it has fed `words`; xterm has already consumed the textarea (Android reads it deferred).
    inputMirror?.sync();
  }

  /** The sticky Prefix key: the next key goes out behind tmux's prefix, once. */
  function withPrefix(data: string): string {
    if (!prefixArmed) return data;
    prefixArmed = false;
    return prefixBytes(prefix) + data;
  }

  $: newline = newlineKeys(activeSessionId ? $agents[activeSessionId]?.agent : undefined);

  function combo(key: string) {
    prefixArmed = false;
    sendInput(prefixBytes(prefix) + key);
  }

  /**
   * The last screen of each session visited, serialized with its colours. Coming back draws it in the same write that
   * clears the terminal (no blank frame between), and tmux's repaint on attach then lands on an identical screen, so
   * only the cells that really changed are repainted. Memory only, like the peeked screens (decision 0084).
   */
  const SCREEN_CAP = 8;
  const screens = new Map<string, string>();
  let serializer: SerializeAddon | null = null;
  let shownId: string | null = null;
  let restored = false;
  function switchScreen(id: string | null) {
    if (!term || !serializer || id === shownId) return;
    if (shownId) {
      screens.delete(shownId);
      screens.set(shownId, serializer.serialize());
      while (screens.size > SCREEN_CAP) screens.delete(screens.keys().next().value as string);
    }
    shownId = id;
    const saved = id ? screens.get(id) : undefined;
    restored = !!saved;
    if (saved) term.write('\x1b[3J\x1b[2J\x1b[H' + saved);
  }
  $: if (term) switchScreen(activeSessionId);

  /**
   * tmux's repaint on attach arrives in several chunks, the first of which clears the screen. Written as they come,
   * a frame can render between the clear and the content. While a screen is already showing, the chunks of the first
   * moments are joined into one write instead.
   */
  const REPAINT_HOLD_MS = 40;
  let held: Uint8Array[] | null = null;
  let holdTimer: ReturnType<typeof setTimeout> | undefined;
  function holdRepaint() {
    held = [];
    clearTimeout(holdTimer);
    holdTimer = setTimeout(releaseRepaint, REPAINT_HOLD_MS);
  }
  function receive(bytes: Uint8Array) {
    if (held) held.push(bytes);
    else term?.write(bytes);
  }
  function releaseRepaint() {
    clearTimeout(holdTimer);
    const chunks = held;
    held = null;
    if (!term) return;
    if (!chunks?.length) return void term.write('\x1b[?25h');
    const all = new Uint8Array(chunks.reduce((n, c) => n + c.length, 0));
    let at = 0;
    for (const c of chunks) (all.set(c, at), (at += c.length));
    term.write(all);
    term.write('\x1b[?25h'); // the preview hid the cursor; the repaint put it where it belongs
  }

  /** Draws a peeked screen at once, dimmed; the repaint after attach clears it (decision 0084). */
  function paintPreview(text: string) {
    if (!term || previewing || restored) return;
    previewing = true;
    term.reset();
    // A peeked screen has no cursor position: hide it rather than show it at the end and then jump.
    term.write('\x1b[?25l' + text.replace(/\r?\n/g, '\r\n'));
  }
  $: if (term && preview && !previewing) paintPreview(preview);
  $: if (!preview) previewing = false;

  /** Typed characters pass through the sticky Ctrl/Alt; key-row sequences are sent as they are. */
  function typed(raw: string) {
    const data = words.dropSpuriousSpace(dropColorReplies(raw));
    if (!data) return;
    // A word that came in whole (swipe typing, a tapped suggestion) is taken back whole; see WordTracker.
    const { out, used } = applyMods(data === '\x7f' ? '\x7f'.repeat(words.backspaceCount()) : data, mods);
    if (used) mods = { ctrl: false, alt: false, shift: false };
    words.feed(out);
    sendInput(withPrefix(out));
  }

  // Dictation (plan R53): the browser's own recognizer; final text is typed as if typed, never followed by Enter.
  const makeRecognizer = recognizerFactory();
  const VOICE_LANG_KEY = 'termenbox-voice-lang';
  let voiceLang: VoiceLang = 'auto';
  try {
    voiceLang = parseVoiceLang(localStorage.getItem(VOICE_LANG_KEY));
  } catch { /* storage can be blocked; the choice then lasts until reload */ }
  let spoken = ''; // the language of the running session, for the bar
  function cycleVoiceLang() {
    voiceLang = nextVoiceLang(voiceLang);
    try {
      localStorage.setItem(VOICE_LANG_KEY, voiceLang);
    } catch { /* see above */ }
  }
  let listening = false;
  let interim = '';
  let voiceError = '';
  let voiceErrorTimer: ReturnType<typeof setTimeout> | undefined;
  const voice = makeRecognizer
    ? new Voice(makeRecognizer, () => (spoken = pickLanguage(voiceLang, words.script, navigator.language)), {
        onText: (t) => typed(t),
        onInterim: (t) => (interim = t),
        onState: (on) => ((listening = on), on && (voiceError = '')),
        onError: (m) => {
          voiceError = m;
          clearTimeout(voiceErrorTimer);
          voiceErrorTimer = setTimeout(() => (voiceError = ''), 5000);
        },
      })
    : null;
  function toggleVoice() {
    voice?.toggle();
    term?.focus();
  }

  /** Key-row keys: sent as they are, but the word being typed must know about them. */
  function sendKey(raw: string) {
    let data = raw;
    const shifted = mods.shift ? shiftSeq(raw) : null;
    if (shifted) {
      data = shifted;
      mods = { ...mods, shift: false };
    }
    words.feed(data);
    sendInput(withPrefix(data));
  }

  /** Text the system clipboard would not give us: the user pastes it into a field of our own, then sends it. */
  let pasteDraft: string | null = null;

  async function paste() {
    const text = await pasteText();
    if (text) sendPaste(text);
    else pasteDraft = lastCopiedText();
  }
  function sendPaste(text: string) {
    term?.paste(text);
    words.clearInsert(); // a pasted word is not a swiped one
    inputMirror?.sync();
  }

  /** Copies the selection, or the visible screen when nothing is selected. */
  async function copy() {
    if (!term) return;
    let text = term.getSelection();
    if (!text) {
      const buf = term.buffer.active;
      const lines: string[] = [];
      for (let i = 0; i < term.rows; i++) lines.push(buf.getLine(buf.viewportY + i)?.translateToString(true) ?? '');
      text = lines.join('\n').replace(/\s+$/, '');
    }
    await copyText(text);
  }

  function openSelect() {
    if (!term) return;
    const buf = term.buffer.active;
    selectText = bufferText((i) => buf.getLine(i)?.translateToString(true), buf.length);
  }

  async function copySelection() {
    const text = touch.text();
    selStatus = text && (await copyText(text)) ? 'Copied' : 'Nothing to copy';
    setTimeout(() => (selStatus = ''), 1500);
    if (text) touch.clear();
  }

  function toggleBell() {
    bell = !bell;
    setBellEnabled(bell);
    if (bell) {
      unlockAudio();
      beep(); // doubles as a sound check
    }
  }

  function setFont(size: number) {
    fontSize = clampFont(size);
    localStorage.setItem(FONT_KEY, String(fontSize));
    if (term) {
      term.options.fontSize = fontSize;
      fitAddon?.fit();
    }
  }

  /**
   * A tap is sent as the app would see a mouse click: a move, a press, a short pause, a release. The browser's own
   * compatibility events press and release in the same millisecond, which mouse-first apps ignore.
   */
  function tapAt(x: number, y: number) {
    suppressClick = true;
    if (sel) {
      touch.clear();
      return;
    }
    if (openLinkAt(x, y)) return;
    const target = document.elementFromPoint(x, y) ?? terminalContainer;
    const fire = (type: string, buttons: number) =>
      target.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, view: window, clientX: x, clientY: y, button: 0, buttons }));
    fire('mousemove', 0);
    fire('mousedown', 1);
    setTimeout(() => fire('mouseup', 0), 40);
    keyboard.tap();
  }
  /** A tap on a web address opens it, unless the app in the terminal has asked for mouse clicks itself. */
  function openLinkAt(x: number, y: number): boolean {
    const rect = terminalContainer?.querySelector('.xterm-screen')?.getBoundingClientRect();
    if (!term || !rect || term.modes.mouseTrackingMode !== 'none') return false;
    const c = cellAt(x, y, rect, term.cols, term.rows, term.buffer.active.viewportY);
    const url = linkAt(term.buffer.active, term.cols, c.row, c.col);
    if (!url) return false;
    window.open(url, '_blank', 'noopener,noreferrer');
    return true;
  }
  function onPointerDown(e: PointerEvent) {
    pinch.down(e.pointerId, e.clientX, e.clientY, fontSize);
    touch.pointerDown(e.clientX, e.clientY, { touch: e.pointerType === 'touch', second: pinch.fingers > 1 });
    if (e.pointerType === 'touch') scroll.down(e.pointerId, e.clientX, e.clientY, pinch.fingers > 1 || !!sel);
  }
  function onPointerMove(e: PointerEvent) {
    touch.pointerMove(e.clientX, e.clientY);
    const next = pinch.move(e.pointerId, e.clientX, e.clientY);
    if (next !== null && next !== fontSize) setFont(next);
    if (e.pointerType === 'touch' && !touch.busy && !sel) scroll.move(e.pointerId, e.clientX, e.clientY);
  }
  function onPointerUp(e: PointerEvent) {
    touch.pointerUp();
    pinch.up(e.pointerId);
    if (e.pointerType === 'touch') scroll.up(e.pointerId, e.type === 'pointercancel');
  }
  function grabHandle(end: 'a' | 'b', e: PointerEvent) {
    e.stopPropagation();
    e.preventDefault();
    touch.grab(end);
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  /** The size the server last got. tmux repaints the whole screen on every resize, so an unchanged size is not sent again. */
  let sentSize = '';
  function sendResize(cols: number, rows: number) {
    if (`${cols}x${rows}` === sentSize) return;
    sentSize = `${cols}x${rows}`;
    crumb(`resize ${cols}x${rows}`);
    batch.flush(); // typed text goes out before the new size, as it was typed
    if (activeSessionId) connection.send({ type: 'resize', cols, rows });
  }

  /**
   * Fits the terminal to its box. While xterm swaps renderers (a lost WebGL context) the fit throws, and no later
   * resize comes to repeat it: the canvas stayed at its old size and sat shifted to the left (issue #99). So a
   * failed fit is tried again on the next frames.
   * Calls are batched to a single rAF to avoid duplicate layouts during tab switches.
   */
  let refitFrame = 0;
  let refitPending = false;
  function refit(attempt = 0) {
    if (attempt === 0) {
      if (refitPending) return;
      refitPending = true;
      refitFrame = requestAnimationFrame(() => {
        refitPending = false;
        doRefit(0);
      });
      return;
    }
    doRefit(attempt);
  }
  function doRefit(attempt: number) {
    cancelAnimationFrame(refitFrame);
    if (disposed || !term) return;
    try {
      fitAddon?.fit();
    } catch {
      if (attempt < 20) refitFrame = requestAnimationFrame(() => doRefit(attempt + 1));
      return;
    }
    sendResize(term.cols, term.rows);
  }

  // The countdown to the next attempt ticks once a second, and only while there is something to count.
  let now = Date.now();
  let tick: ReturnType<typeof setInterval> | undefined;
  $: if (connState === 'reconnecting' || connState === 'unreachable') tick ??= setInterval(() => (now = Date.now()), 1000);
  else if (tick) (clearInterval(tick), (tick = undefined));
  function connMessage(state: ServerState, at: number): { title: string; detail: string } {
    const retry = connection.retry ?? null;
    if (state === 'checking') return { title: 'No answer from the server', detail: 'Checking the connection. Input may not be arriving.' };
    if (state === 'connecting') return { title: 'Connecting…', detail: 'Input is unavailable until connected.' };
    if (state === 'closed') return { title: 'Connection closed', detail: 'Input is unavailable. Open Servers to reconnect.' };
    void at; // re-evaluated every second so the countdown moves
    let when = 'Reconnecting…';
    if (retry) when = retry.inMs == null ? `Reconnecting now (attempt ${retry.attempt})…` : `Next attempt in ${Math.max(1, Math.ceil(retry.inMs / 1000))} s (attempt ${retry.attempt})`;
    return { title: 'Disconnected', detail: `Input is paused. ${when}` };
  }
  $: connInfo = offline ? connMessage(connState, now) : null;

  $: if (term && $settings) term.options.theme = terminalTheme(document.documentElement.dataset.theme === 'light');

  onMount(() => {
    term = new XTermTerminal({ cursorBlink: true, fontSize, fontFamily: FONT_FAMILY, theme: terminalTheme(document.documentElement.dataset.theme === 'light') });
    fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    serializer = new SerializeAddon();
    term.loadAddon(serializer);
    term.open(terminalContainer);
    // WebGL is the fast renderer; where it is missing or its context is lost, xterm's DOM renderer takes over
    // instead of leaving a blank screen.
    crumb(`renderer ${$settings.renderer}`);
    if ($settings.renderer === 'webgl') try {
      const gl = new WebglAddon();
      gl.onContextLoss(() => {
        try {
          gl.dispose();
        } catch {
          /* xterm 5.3 can throw while swapping renderers; the screen keeps its last frame until the next redraw */
        }
        if (webgl === gl) webgl = null;
        crumb('renderer lost');
        refit();
      });
      term.loadAddon(gl);
      webgl = gl;
    } catch {
      /* DOM renderer */
    }
    fitAddon.fit();
    refit();

    cleanups.push(installAudioUnlock());
    cleanups.push(() => (clearInterval(tick), clearTimeout(refusedTimer), clearTimeout(holdTimer)));
    if (term.textarea) {
      cleanups.push(attachInputLogger(term.textarea));
      cleanups.push(attachSwipeSupport(term.textarea, sendInput, words)); // fact 0027
      // The field always shows the word being typed, but only once xterm has sent it (iOS suggestions, issue #250).
      inputMirror = attachInputMirror(term.textarea, words);
      cleanups.push(() => inputMirror?.dispose());
      setEnterKeyHint(term.textarea);
      // Hiding the keyboard (OK, or its own hide key) must not leave the terminal inactive.
      const onBlur = (e: FocusEvent) => !disposed && keyboard.onBlur({ to: e.relatedTarget });
      const onPageHide = () => voice?.stop();
      window.addEventListener('pagehide', onPageHide);
      cleanups.push(() => (window.removeEventListener('pagehide', onPageHide), voice?.stop()));
      term.textarea.addEventListener('blur', onBlur);
      const field = term.textarea;
      cleanups.push(() => field.removeEventListener('blur', onBlur));
    }
    window.addEventListener('termenbox-debug', syncDebug);
    cleanups.push(() => window.removeEventListener('termenbox-debug', syncDebug));

    // Ctrl+V and Ctrl+Shift+V would reach the program as ^V; let the browser paste instead (xterm handles the paste event).
    // Ctrl+Shift+C copies the selection to this machine's clipboard (plain Ctrl+C stays an interrupt for the program).
    term.attachCustomKeyEventHandler((ev) => {
      if (ev.type !== 'keydown' || !ev.ctrlKey || ev.altKey || ev.metaKey) return true;
      const key = ev.key.toLowerCase();
      if (key === 'v') return false;
      if (key === 'c' && ev.shiftKey) {
        ev.preventDefault(); // the browser's own Ctrl+Shift+C opens its inspector
        const text = term?.getSelection();
        if (text) void copyText(text);
        return false;
      }
      return true;
    });
    // Shift+right-click pastes, also while a program has the mouse (where the plain click belongs to it).
    const onContextMenu = (ev: MouseEvent) => {
      if (!ev.shiftKey) return;
      ev.preventDefault();
      ev.stopPropagation();
      void paste();
    };
    terminalContainer.addEventListener('contextmenu', onContextMenu, true);
    cleanups.push(() => terminalContainer.removeEventListener('contextmenu', onContextMenu, true));

    cleanups.push(silenceColorQueries(term.parser).dispose);
    term.onData((data) => typed(data));
    term.onBell(() => bell && beep());
    term.onScroll(() => touch.refresh()); // handles follow their text
    term.onResize(({ cols, rows }) => sendResize(cols, rows));
    // While selecting, the finger must not scroll the page or the terminal.
    const blockScroll = (ev: TouchEvent) => touch.busy && ev.preventDefault();
    terminalContainer.addEventListener('touchmove', blockScroll, { passive: false });
    cleanups.push(() => terminalContainer.removeEventListener('touchmove', blockScroll));
    // A tap or scroll handled above must not also arrive as the browser's own mouse events and click.
    const swallowClick = (ev: TouchEvent) => {
      if (!suppressClick) return;
      suppressClick = false;
      ev.preventDefault();
    };
    terminalContainer.addEventListener('touchend', swallowClick, { passive: false });
    cleanups.push(() => terminalContainer.removeEventListener('touchend', swallowClick));
    cleanups.push(() => scroll.stopFling());
    cleanups.push(() => cancelAnimationFrame(refitFrame));

    let lastWidth = 0;
    let lastHeight = 0;
    const resizeObserver = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const { width, height } = entry.contentRect;
        if (width !== lastWidth || height !== lastHeight) {
          lastWidth = width;
          lastHeight = height;
          refit();
          break;
        }
      }
    });
    resizeObserver.observe(terminalContainer);
    cleanups.push(() => resizeObserver.disconnect());

    cleanups.push(
      connection.subscribe((msg) => {
        if (msg.type === 'output') {
          receive(b64ToBytes(msg.data));
        } else if (msg.type === 'term') {
          receive(msg.bytes);
        } else if (msg.type === 'session_attached') {
          // tmux repaints the screen on attach; start clean so a re-attach never doubles the content.
          // If we just painted a preview, it already reset the terminal; skip the second reset to avoid a flash.
          const wasPreviewing = previewing;
          previewing = false;
          crumb('attached');
          if (restored || wasPreviewing) holdRepaint();
          sentSize = ''; // the new attachment has not been told the size yet
          if (!wasPreviewing && !restored) term?.reset();
          restored = false;
          refit();
          // Connecting lands in a live terminal. The page must not scroll to the field to do it.
          if (term?.textarea) term.textarea.focus({ preventScroll: true });
          else term?.focus();
        }
      }),
    );

    return teardown;
  });
</script>

<div class="wrap">
  {#if debug}<InputLog />{/if}
  <div class="screenbox" class:offline>
    <div
      class="screen"
      class:previewing
      bind:this={terminalContainer}
      on:click={() => (sel ? touch.clear() : keyboard.tap())}
      on:pointerdown={onPointerDown}
      on:pointermove={onPointerMove}
      on:pointerup={onPointerUp}
      on:pointercancel={onPointerUp}
    ></div>
    {#if sel && !sel.dragging}
      {@const a = (sel, touch.handlePosition('a'))}
      {@const b = (sel, touch.handlePosition('b'))}
      <div class="selbar">
        <button type="button" on:mousedown|preventDefault on:click={copySelection}>Copy</button>
        <button type="button" class="secondary" on:mousedown|preventDefault on:click={() => touch.clear()}>Done</button>
      </div>
      {#if a}<button type="button" class="handle" aria-label="Selection start" style="left: {a.x}px; top: {a.y}px;" on:pointerdown={(e) => grabHandle('a', e)} on:pointermove={onPointerMove} on:pointerup={onPointerUp}></button>{/if}
      {#if b}<button type="button" class="handle" aria-label="Selection end" style="left: {b.x}px; top: {b.y}px;" on:pointerdown={(e) => grabHandle('b', e)} on:pointermove={onPointerMove} on:pointerup={onPointerUp}></button>{/if}
    {/if}
    {#if selStatus}<div class="selstatus">{selStatus}</div>{/if}
    {#if listening || voiceError}<div class="voicebar" role="status">{voiceError || interim || `Listening (${spoken})…`}</div>{/if}
    {#if connInfo}
      <div class="connbar" class:soft={connState === 'checking'} class:refused={inputRefused} role="status" aria-live="polite">
        <span class="dot" aria-hidden="true"></span>
        <span class="msg"><strong>{connInfo.title}</strong> {connInfo.detail}</span>
        {#if connState !== 'checking' && connState !== 'closed'}<button on:click={() => connection.retryNow?.()}>Retry now</button>{/if}
      </div>
    {/if}
  </div>
  <KeyRow bind:mods bind:prefixArmed {profile} prefixLabel={prefixLabel(prefix)} onnewline={() => sendKey(newline.seq)} newlineHint={newline.hint} onkey={sendKey} {fnOpen}
    onfn={() => keyboard.toggleFn()} onkeyboard={() => keyboard.toggleSystem()} voiceSupported={!!voice} {listening} onvoice={toggleVoice} {voiceLang} onvoicelang={cycleVoiceLang} onfont={(d) => setFont(fontSize + d)}
    onpaste={paste} oncopy={copy} onselect={openSelect} {bell} onbell={toggleBell} />
  {#if fnOpen}<FnKeyboard bind:mods onkey={typed} onback={() => keyboard.closeFn()} {profile} {bell} onbell={toggleBell} oncombo={combo} onpaste={paste} oncopy={copy} onselect={openSelect} />{/if}
  {#if pasteDraft !== null}<PasteSheet text={pasteDraft} onsend={(t) => { pasteDraft = null; sendPaste(t); term?.focus(); }} onclose={() => { pasteDraft = null; term?.focus(); }} />{/if}
  {#if selectText !== null}<SelectView text={selectText} onclose={() => { selectText = null; term?.focus(); }} />{/if}
</div>

<style>
  .wrap {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .screenbox {
    position: relative; /* the selection bar and handles are placed against the screen */
    flex: 1;
    min-height: 0;
  }
  .screen {
    height: 100%;
    touch-action: none; /* one-finger drag and tap, and the two-finger pinch, are all ours (TouchScroll, PinchZoom) */
  }
  .screen :global(.xterm-viewport),
  .screen :global(.xterm-screen) {
    touch-action: none;
  }
  .selbar {
    position: absolute;
    top: 6px;
    right: 6px;
    z-index: 5;
    display: flex;
    gap: 6px;
  }
  .selbar button {
    min-height: 34px;
    padding: 4px 14px;
  }
  .voicebar {
    position: absolute;
    left: 6px;
    right: 6px;
    bottom: 6px;
    z-index: 5;
    padding: 4px 10px;
    font-size: 0.85rem;
    background: var(--bg-secondary);
    color: var(--fg);
    border: 1px solid var(--accent);
    pointer-events: none;
  }
  .connbar {
    position: absolute;
    left: 6px;
    right: 6px;
    bottom: 6px;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-radius: 8px;
    border: 1px solid var(--danger);
    background: var(--bg-secondary);
    color: var(--text-primary, inherit);
    font-size: 0.8rem;
  }
  .connbar.soft {
    border-color: var(--warning, var(--danger));
  }
  .connbar.refused {
    animation: refused 0.3s;
  }
  .connbar .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--danger);
    animation: pulse 1.2s ease-in-out infinite;
  }
  .connbar.soft .dot {
    background: var(--warning, var(--danger));
  }
  .connbar .msg {
    flex: 1;
    min-width: 0;
  }
  .connbar button {
    flex: none;
    padding: 2px 10px;
    font-size: 0.8rem;
  }
  .screenbox.offline .screen {
    opacity: 0.55;
  }
  @keyframes pulse {
    50% {
      opacity: 0.3;
    }
  }
  @keyframes refused {
    25% {
      transform: translateX(-4px);
    }
    75% {
      transform: translateX(4px);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .connbar .dot,
    .connbar.refused {
      animation: none;
    }
  }
  .selstatus {
    position: absolute;
    top: 6px;
    left: 6px;
    z-index: 5;
    padding: 4px 10px;
    border-radius: 6px;
    background: var(--bg-secondary);
    color: var(--fg);
    font-size: 0.8rem;
  }
  .handle {
    position: absolute;
    z-index: 5;
    width: 28px;
    height: 28px;
    margin: -4px 0 0 -14px;
    padding: 0;
    border-radius: 50%;
    border: 2px solid var(--bg);
    background: var(--accent);
    touch-action: none;
  }
  .screen.previewing {
    opacity: 0.55;
  }
  :global(.xterm) {
    height: 100% !important;
  }
  :global(.xterm-helper-textarea) {
    opacity: 0 !important;
    font-size: 16px !important; /* iOS zooms the whole page into a focused field under 16px; pinching back was the only cure (#99) */
  }
</style>
