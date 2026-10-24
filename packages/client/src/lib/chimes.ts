// The sounds a session that needs you can make. Each is data: a list of notes that one player schedules, so adding
// a sound or cutting a bad one is an edit to this table. The terminal bell keeps its own beep (bell.ts).

/** Asking: a question or a failure, rising. Done: a finished reply, the same notes played back falling. */
export type ChimeKind = 'ask' | 'done';

export interface Note {
  /** Seconds after the start. */
  at: number;
  hz: number;
  /** Glide to this pitch over `glide` seconds; without it the pitch is steady. */
  to?: number;
  glide?: number;
  /** Seconds the note sounds (attack 8 ms, then an exponential decay). */
  len: number;
  gain: number;
  wave?: OscillatorType;
  /** An octave above at this share of `gain`, for warmth. */
  overtone?: number;
}

export interface Chime { id: string; label: string; notes: Note[] }

export const MAX_GAIN = 0.25;
export const MAX_SECONDS = 1;

export const CHIMES: Chime[] = [
  { id: 'bloop', label: 'Bloop-bloop-ping', notes: [
    { at: 0, hz: 659, to: 880, glide: 0.09, len: 0.13, gain: 0.16 },
    { at: 0.11, hz: 880, to: 1318, glide: 0.09, len: 0.15, gain: 0.18 },
    { at: 0.23, hz: 2093, len: 0.32, gain: 0.12, overtone: 0.15 },
    { at: 0.35, hz: 2093, len: 0.3, gain: 0.03 }, // the echo
  ] },
  { id: 'pebble', label: 'Pebble', notes: [
    { at: 0, hz: 392, len: 0.18, gain: 0.2 },
    { at: 0.14, hz: 523, len: 0.22, gain: 0.18 },
  ] },
  { id: 'sparkle', label: 'Sparkle', notes: [
    { at: 0, hz: 1568, len: 0.14, gain: 0.1, wave: 'triangle' },
    { at: 0.08, hz: 2093, len: 0.14, gain: 0.1, wave: 'triangle' },
    { at: 0.16, hz: 2637, len: 0.3, gain: 0.1, wave: 'triangle' },
    { at: 0.28, hz: 2637, len: 0.2, gain: 0.025, wave: 'triangle' }, // the echo
  ] },
  { id: 'classic', label: 'Classic beep', notes: [{ at: 0, hz: 880, len: 0.27, gain: 0.2 }] },
];

export const CHIME_IDS = CHIMES.map(c => c.id);
export const DEFAULT_CHIME = CHIMES[0].id;
export const chimeById = (id: string): Chime => CHIMES.find(c => c.id === id) ?? CHIMES[0];

export const totalSeconds = (notes: Note[]) => Math.max(0, ...notes.map(n => n.at + n.len));

/** The same notes played back in time, so a rise becomes a fall: what a finished reply sounds like. */
export function mirror(notes: Note[]): Note[] {
  const total = totalSeconds(notes);
  return notes.map(n => ({ ...n, at: Math.max(0, Math.round((total - n.at - n.len) * 1000) / 1000), hz: n.to ?? n.hz, to: n.to === undefined ? undefined : n.hz }))
    .sort((a, b) => a.at - b.at);
}

export const notesFor = (chime: Chime, kind: ChimeKind): Note[] => kind === 'done' ? mirror(chime.notes) : chime.notes;

/** The part of the Web Audio API the player uses, so a test can stand in for it. */
export interface AudioLike {
  currentTime: number;
  destination: unknown;
  createOscillator(): { type: OscillatorType; frequency: { value: number; setValueAtTime(v: number, t: number): unknown; exponentialRampToValueAtTime(v: number, t: number): unknown }; connect(n: unknown): unknown; start(t?: number): void; stop(t?: number): void };
  createGain(): { gain: { setValueAtTime(v: number, t: number): unknown; exponentialRampToValueAtTime(v: number, t: number): unknown }; connect(n: unknown): unknown };
}

/** Schedules the notes on a running context; returns how many oscillators it started. */
export function schedule(ctx: AudioLike, notes: Note[]): number {
  const t0 = ctx.currentTime;
  let started = 0;
  const voice = (hz: number, to: number | undefined, glide: number | undefined, wave: OscillatorType, at: number, len: number, gain: number) => {
    const osc = ctx.createOscillator();
    const amp = ctx.createGain();
    osc.type = wave;
    osc.frequency.setValueAtTime(hz, t0 + at);
    if (to !== undefined) osc.frequency.exponentialRampToValueAtTime(to, t0 + at + (glide ?? len));
    amp.gain.setValueAtTime(0.0001, t0 + at);
    amp.gain.exponentialRampToValueAtTime(gain, t0 + at + 0.008);
    amp.gain.exponentialRampToValueAtTime(0.0001, t0 + at + len);
    osc.connect(amp);
    amp.connect(ctx.destination);
    osc.start(t0 + at);
    osc.stop(t0 + at + len + 0.02);
    started++;
  };
  for (const n of notes) {
    voice(n.hz, n.to, n.glide, n.wave ?? 'sine', n.at, n.len, n.gain);
    if (n.overtone) voice(n.hz * 2, n.to === undefined ? undefined : n.to * 2, n.glide, n.wave ?? 'sine', n.at, n.len, n.gain * n.overtone);
  }
  return started;
}
