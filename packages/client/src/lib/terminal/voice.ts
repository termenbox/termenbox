// Voice input (plan R53): the browser's own speech recognizer, nothing of ours in between. No audio goes to our
// servers and no paid speech service is used; the browser vendor may process the audio (privacy notice, R08).

/** The part of the Web Speech API we use. */
export interface RecognizerResult {
  readonly isFinal: boolean;
  readonly 0: { readonly transcript: string };
}
export interface RecognizerEvent {
  readonly resultIndex: number;
  readonly results: { readonly length: number; readonly [i: number]: RecognizerResult };
}
export interface Recognizer {
  lang: string;
  continuous: boolean;
  interimResults: boolean;
  onresult: ((e: RecognizerEvent) => void) | null;
  onerror: ((e: { error: string }) => void) | null;
  onend: (() => void) | null;
  start(): void;
  stop(): void;
}
export type RecognizerFactory = () => Recognizer;

/** The recognizer the browser offers, or null when there is none (the key is then hidden). */
export function recognizerFactory(scope: Record<string, unknown> = globalThis as Record<string, unknown>): RecognizerFactory | null {
  const ctor = (scope.SpeechRecognition ?? scope.webkitSpeechRecognition) as (new () => Recognizer) | undefined;
  return ctor ? () => new ctor() : null;
}

/** What a recognizer event adds: text to type now, and the words still being guessed. */
export interface Mapped {
  /** Final text, spaced so that it follows whatever was typed before it in this workspace. */
  typed: string;
  interim: string;
}

/**
 * Maps one event to text. Final results are typed once, trimmed, with one space before every phrase after the first
 * (browsers disagree on whether they supply it). Interim results are only shown. `started` says whether this
 * workspace has typed anything yet.
 */
export function mapResults(e: RecognizerEvent, started: boolean): Mapped {
  let typed = '';
  let interim = '';
  for (let i = e.resultIndex; i < e.results.length; i++) {
    const r = e.results[i];
    const text = r[0].transcript.trim();
    if (!text) continue;
    if (r.isFinal) {
      typed += (started || typed ? ' ' : '') + text;
    } else {
      interim += (interim ? ' ' : '') + text;
    }
  }
  return { typed, interim };
}

/** Why a workspace ended with an error, in words for the person; null when it is not worth saying. */
export function errorMessage(code: string): string | null {
  switch (code) {
    case 'not-allowed':
    case 'service-not-allowed':
      return `Microphone is blocked. Allow it in the browser settings to dictate. (${code})`;
    case 'audio-capture':
      return 'No microphone found.';
    case 'not-readable':
      return 'The microphone is in use by another app.';
    case 'network':
      return 'Speech recognition needs a connection in this browser.';
    case 'language-not-supported':
      return 'This language is not supported for dictation.';
    case 'no-speech':
    case 'aborted':
      return null;
    default:
      return `Dictation failed (${code}).`;
  }
}

/** What the phone says about itself; an iPad that asks for desktop sites reports as a Mac with a touch screen. */
export function isIOS(nav: { userAgent?: string; platform?: string; maxTouchPoints?: number } | undefined = (globalThis as { navigator?: Navigator }).navigator): boolean {
  if (!nav) return false;
  return /iPad|iPhone|iPod/.test(nav.userAgent ?? '') || (nav.platform === 'MacIntel' && (nav.maxTouchPoints ?? 0) > 1);
}

/** The words for a phone whose recognizer refuses the page while the keyboard's own dictation works. */
export const IOS_KEYBOARD_HINT = "Dictation from this key isn't available on this phone. Use the mic on the keyboard.";

/** The browser pieces the permission check reads; a browser without them is handled below. */
export interface MicScope {
  mediaDevices?: Pick<MediaDevices, 'getUserMedia'>;
  permissions?: Pick<Permissions, 'query'>;
  ios?: boolean;
}

/**
 * Asks for the microphone before the recognizer starts, so the browser's own prompt appears on the first tap.
 * Returns null when dictation may go ahead, or a message for the person. A permission already granted is reused
 * without a prompt; the device is released at once because the recognizer opens it itself.
 */
export async function askMicrophone(
  scope: MicScope = micScope(),
): Promise<string | null> {
  const devices = scope.mediaDevices;
  if (!devices?.getUserMedia) return null; // no MediaDevices (an old browser): let the recognizer ask its own way
  try {
    const stream = await devices.getUserMedia({ audio: true });
    stream.getTracks().forEach((t) => t.stop());
    return null;
  } catch (e) {
    const code = micErrorCode(e);
    if (!code) return null; // not a denial: let the recognizer ask for the microphone itself
    const base = errorMessage(code) ?? '';
    if (code !== 'not-allowed') return base;
    // Which refusal this is decides the fix, and the browser's words for it are all we have on a phone: the
    // exception's name and what the site's permission reads as (denied, or never asked).
    const name = (e as { name?: string } | null)?.name || 'unknown';
    let state = '';
    try {
      state = (await scope.permissions?.query({ name: 'microphone' as PermissionName }))?.state ?? '';
    } catch { /* the Permissions API may not know the microphone */ }
    const detail = ` [${name}${state ? `, permission ${state}` : ''}]`;
    return base + detail + (scope.ios ? ' On iPhone: Settings > Safari > Microphone, or use the mic on the keyboard.' : '');
  }
}

function micScope(): MicScope {
  const nav = (globalThis as { navigator?: Navigator }).navigator;
  return { mediaDevices: nav?.mediaDevices, permissions: nav?.permissions, ios: isIOS() };
}

/**
 * Maps a getUserMedia rejection to the wording `errorMessage` already has; null for any other failure. iOS throws
 * InvalidStateError here although nothing was denied, so an unknown failure must not block dictation: the recognizer
 * reports a real denial itself.
 */
function micErrorCode(e: unknown): string | null {
  switch ((e as { name?: string } | null)?.name) {
    case 'NotAllowedError':
    case 'SecurityError':
      return 'not-allowed';
    case 'NotFoundError':
    case 'DevicesNotFoundError':
      return 'audio-capture';
    case 'NotReadableError':
    case 'TrackStartError':
      return 'not-readable';
    default:
      return null;
  }
}

/** The dictation language the person chose: `auto` follows the script of what was typed. */
export type VoiceLang = 'auto' | 'en' | 'ru';
const LANGS: VoiceLang[] = ['auto', 'en', 'ru'];

export function nextVoiceLang(l: VoiceLang): VoiceLang {
  return LANGS[(LANGS.indexOf(l) + 1) % LANGS.length];
}

/** Reads a stored choice; anything else is `auto`. */
export function parseVoiceLang(v: string | null): VoiceLang {
  return LANGS.includes(v as VoiceLang) ? (v as VoiceLang) : 'auto';
}

/**
 * The recognizer takes one language per workspace and cannot detect it, and the web cannot read the keyboard layout on
 * iOS. So: the person's choice, else the script of the text just typed (Latin -> English, Cyrillic -> Russian), else
 * the browser's language.
 */
export function pickLanguage(choice: VoiceLang, script: 'latin' | 'cyrillic' | null, browserLang: string): string {
  if (choice === 'en') return 'en-US';
  if (choice === 'ru') return 'ru-RU';
  if (script === 'latin') return 'en-US';
  if (script === 'cyrillic') return 'ru-RU';
  return browserLang || 'en-US';
}

export interface VoiceHandlers {
  /** Final text, to type as if typed (never followed by Enter). */
  onText: (text: string) => void;
  /** Words still being guessed; empty when nothing is. */
  onInterim: (text: string) => void;
  /** Listening started or stopped. */
  onState: (listening: boolean) => void;
  onError: (message: string) => void;
  /**
   * On iOS, the recognizer refused a page whose microphone check passed (Safari, and more so a home-screen app, often
   * does). The keyboard's own dictation still reaches the input, so the person is pointed there instead of at a
   * "blocked" settings screen that is not the problem. Without this handler the error is shown as usual.
   */
  onFallback?: (code: string) => void;
}

/** One dictation key: tap to start, tap to stop. */
export class Voice {
  private rec: Recognizer | null = null;
  private started = false;
  /** A start is waiting for the browser's permission answer. */
  private waiting = false;
  /** The microphone check succeeded; later starts need not open the device again. */
  private micReady = false;
  /** Distinguishes a live start from one cancelled by a stop while the prompt was open. */
  private generation = 0;

  constructor(
    private readonly make: RecognizerFactory,
    private readonly lang: () => string,
    private readonly on: VoiceHandlers,
    private readonly ask: () => Promise<string | null> = askMicrophone,
    private readonly ios: boolean = isIOS(),
  ) {}

  get listening(): boolean {
    return this.rec !== null;
  }

  toggle(): Promise<void> {
    if (this.rec) {
      this.stop();
      return Promise.resolve();
    }
    if (this.waiting) return Promise.resolve(); // the browser prompt is already open
    return this.start();
  }

  async start(): Promise<void> {
    if (this.rec || this.waiting) return;
    this.waiting = true;
    const generation = ++this.generation;
    if (!this.micReady) {
      let denied: string | null;
      try {
        denied = await this.ask();
      } catch {
        denied = 'Microphone permission was not granted.';
      }
      if (generation !== this.generation) return; // stopped while the prompt was open
      if (denied) {
        this.waiting = false;
        this.on.onError(denied);
        this.on.onState(false);
        return;
      }
      this.micReady = true;
    }
    this.waiting = false;
    this.begin();
  }

  private begin(): void {
    const rec = this.make();
    rec.lang = this.lang();
    rec.continuous = true;
    rec.interimResults = true;
    this.started = false;
    rec.onresult = (e) => {
      if (this.rec !== rec) return;
      const m = mapResults(e, this.started);
      if (m.typed) {
        this.started = true;
        this.on.onText(m.typed);
      }
      this.on.onInterim(m.interim);
    };
    rec.onerror = (e) => {
      if (this.rec !== rec) return;
      // Tear down here too: some browsers report the error without a following `onend`, and a blocked mic must
      // never leave the key pressed.
      this.rec = null;
      this.on.onInterim('');
      this.on.onState(false);
      try {
        rec.stop();
      } catch { /* already closing */ }
      if (e.error === 'not-allowed' || e.error === 'service-not-allowed' || e.error === 'audio-capture') {
        this.micReady = false; // permission may have been revoked: check again on the next tap
      }
      if (this.ios && this.on.onFallback && (e.error === 'not-allowed' || e.error === 'service-not-allowed')) {
        this.on.onFallback(e.error);
        return;
      }
      const msg = errorMessage(e.error);
      if (msg) this.on.onError(msg);
    };
    rec.onend = () => {
      if (this.rec !== rec) return;
      this.rec = null;
      this.on.onInterim('');
      this.on.onState(false);
    };
    this.rec = rec;
    try {
      rec.start();
      this.on.onState(true);
    } catch {
      this.rec = null;
      this.on.onState(false);
      this.on.onError('Dictation could not start.');
    }
  }

  /** Stops listening; a phrase the recognizer is about to finalize still arrives before `onend`. */
  stop(): void {
    this.generation++; // a permission prompt still open no longer starts a workspace
    this.waiting = false;
    const rec = this.rec;
    if (!rec) return;
    try {
      rec.stop();
    } catch { /* already closing */ }
  }
}
