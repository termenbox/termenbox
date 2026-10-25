// The terminal bell (BEL, \x07): a short beep, which agents and shells use to say "look at me". The needs-you
// sounds (chimes.ts) share its audio context.
//
// Three iOS rules shape this file:
//  1. Audio starts only inside a user gesture, and `pointerdown` does not count: unlock on the events that do
//     (`touchend`, `click`, `keydown`, `pointerup`), and keep listening: the browser can suspend the context again.
//  2. Web Audio obeys the ringer switch unless the page declares itself as playback
//     (`navigator.audioSession.type = "playback"`, iOS 16.4+).
//  3. A context created before the gesture stays suspended, so it is created inside the gesture.
import { writable } from 'svelte/store';
import { chimeById, notesFor, schedule, type ChimeKind } from './chimes';

const KEY = 'termenbox_bell';
const UNLOCK_EVENTS = ['pointerup', 'touchend', 'click', 'keydown'] as const;

let ctx: AudioContext | null = null;

export const bellEnabled = (storage: Pick<Storage, 'getItem'> = localStorage) => storage.getItem(KEY) !== '0';
export const setBellEnabled = (on: boolean, storage: Pick<Storage, 'setItem'> = localStorage) => storage.setItem(KEY, on ? '1' : '0');

type AudioSessionNav = Navigator & { audioSession?: { type: string } };

/** Whether sound can play now: `locked` until a gesture (or again after the browser suspends the page's audio). */
export type AudioState = 'running' | 'locked' | 'unsupported';
export const audioState = writable<AudioState>('locked');
const running = new Set<() => void>();

/** Runs `fn` each time the audio context starts running (after a tap, or when the browser lets it resume). */
export function onAudioRunning(fn: () => void): () => void {
  running.add(fn);
  return () => void running.delete(fn);
}

const audioCtor = () => typeof window === 'undefined' ? undefined : window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;

function publish() {
  if (!ctx) { audioState.set(audioCtor() ? 'locked' : 'unsupported'); return; }
  const next: AudioState = ctx.state === 'running' ? 'running' : 'locked'; // `suspended`, and iOS's `interrupted`
  audioState.set(next);
  if (next === 'running') for (const fn of [...running]) fn();
}

/** Creates/resumes the audio context. Call from a gesture handler; returns whether sound can play now. */
export function unlockAudio(): boolean {
  try {
    const Ctx = audioCtor();
    if (!Ctx) { audioState.set('unsupported'); return false; }
    const workspace = (navigator as AudioSessionNav).audioSession;
    if (workspace) workspace.type = 'playback'; // audible even with the ringer switch off
    if (!ctx) {
      ctx = new Ctx();
      ctx.addEventListener('statechange', publish);
    }
    if (ctx.state !== 'running') void ctx.resume().catch(() => {});
    publish();
    return ctx.state === 'running';
  } catch {
    return false;
  }
}

/**
 * Unlocks audio on a real gesture and keeps the listeners for the life of the page: the browser may suspend the
 * context again (iOS does when the page is backgrounded or the phone locks), and the next tap must wake it. It also
 * tries to wake it when the page comes back, which works wherever the browser has already seen a gesture.
 * Returns the function that removes the listeners.
 */
export function installAudioUnlock(target: Document = document): () => void {
  const gesture = () => {
    if (!ctx || ctx.state !== 'running') unlockAudio();
  };
  const back = () => {
    if (ctx && ctx.state !== 'running' && target.visibilityState !== 'hidden') void ctx.resume().catch(() => {});
  };
  UNLOCK_EVENTS.forEach((name) => target.addEventListener(name, gesture, true));
  target.addEventListener('visibilitychange', back);
  target.defaultView?.addEventListener('pageshow', back);
  return () => {
    UNLOCK_EVENTS.forEach((name) => target.removeEventListener(name, gesture, true));
    target.removeEventListener('visibilitychange', back);
    target.defaultView?.removeEventListener('pageshow', back);
  };
}

/** What a request to play a needs-you sound came to. */
export type PlayResult = 'played' | 'locked' | 'unsupported';

/** Plays a needs-you sound; `locked` when audio is not running (it is asked to resume, and `onAudioRunning` fires when it does). */
export function playChime(id: string, kind: ChimeKind): PlayResult {
  if (!audioCtor()) return 'unsupported';
  if (!ctx || ctx.state !== 'running') {
    if (ctx) void ctx.resume().catch(() => {});
    return 'locked';
  }
  schedule(ctx, notesFor(chimeById(id), kind));
  return 'played';
}

/** The menu's preview: unlocks inside the tap, waits for the context to run, then plays. */
export async function previewChime(id: string, kind: ChimeKind): Promise<PlayResult> {
  unlockAudio();
  try { await ctx?.resume(); } catch { /* playChime reports the lock */ }
  return playChime(id, kind);
}

/** Plays the beep; false when audio is not unlocked yet (the caller can tell the user to tap). */
export function beep(): boolean {
  if (!ctx || ctx.state !== 'running') return false;
  const osc = ctx.createOscillator();
  const gain = ctx.createGain();
  osc.type = 'sine';
  osc.frequency.value = 880;
  gain.gain.setValueAtTime(0.0001, ctx.currentTime);
  gain.gain.exponentialRampToValueAtTime(0.2, ctx.currentTime + 0.01);
  gain.gain.exponentialRampToValueAtTime(0.0001, ctx.currentTime + 0.25);
  osc.connect(gain).connect(ctx.destination);
  osc.start();
  osc.stop(ctx.currentTime + 0.27);
  return true;
}
