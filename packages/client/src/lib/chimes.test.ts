import { describe, expect, it } from 'vitest';
import { CHIMES, CHIME_IDS, DEFAULT_CHIME, MAX_GAIN, MAX_SECONDS, chimeById, mirror, notesFor, schedule, totalSeconds, type AudioLike } from './chimes';

describe('the chime table', () => {
  it('has unique ids and a default that exists', () => {
    expect(new Set(CHIME_IDS).size).toBe(CHIMES.length);
    expect(CHIME_IDS).toContain(DEFAULT_CHIME);
  });

  it.each(CHIMES)('$id stays short, quiet and finite', (chime) => {
    for (const kind of ['ask', 'done'] as const) {
      const notes = notesFor(chime, kind);
      expect(totalSeconds(notes)).toBeLessThanOrEqual(MAX_SECONDS);
      for (const n of notes) {
        expect(Number.isFinite(n.hz) && n.hz > 0).toBe(true);
        expect(n.to === undefined || (Number.isFinite(n.to) && n.to > 0)).toBe(true);
        expect(n.at).toBeGreaterThanOrEqual(0);
        expect(n.len).toBeGreaterThan(0);
        expect(n.gain * (1 + (n.overtone ?? 0))).toBeLessThanOrEqual(MAX_GAIN);
      }
    }
  });

  it('falls back to the default for an unknown id', () => {
    expect(chimeById('nope').id).toBe(DEFAULT_CHIME);
  });
});

describe('mirror', () => {
  it('plays the notes back in time and turns a rise into a fall', () => {
    const notes = [{ at: 0, hz: 100, to: 200, len: 0.1, gain: 0.1 }, { at: 0.2, hz: 400, len: 0.1, gain: 0.1 }];
    const back = mirror(notes);
    expect(totalSeconds(back)).toBeCloseTo(totalSeconds(notes));
    expect(back[0]).toMatchObject({ hz: 400 });
    expect(back[1]).toMatchObject({ at: expect.closeTo(0.2), hz: 200, to: 100 });
  });

  it('leaves a single steady note as it was', () => {
    const [classic] = CHIMES.filter(c => c.id === 'classic');
    expect(mirror(classic.notes)).toEqual(classic.notes.map(n => ({ ...n, to: undefined })));
  });
});

describe('schedule', () => {
  const fake = () => {
    const started: { at: number }[] = [];
    const param = () => ({ value: 0, setValueAtTime: () => {}, exponentialRampToValueAtTime: () => {} });
    const ctx: AudioLike = {
      currentTime: 10, destination: {},
      createOscillator: () => { const o = { type: 'sine' as OscillatorType, frequency: param(), connect: () => {}, start: (t?: number) => void started.push({ at: t ?? 0 }), stop: () => {} }; return o; },
      createGain: () => ({ gain: param(), connect: () => {} }),
    };
    return { ctx, started };
  };

  it('starts one oscillator per note, plus one for each overtone, at the right times', () => {
    const { ctx, started } = fake();
    const bloop = chimeById('bloop');
    const count = schedule(ctx, bloop.notes);
    expect(count).toBe(bloop.notes.length + bloop.notes.filter(n => n.overtone).length);
    expect(started[0].at).toBe(10);
    expect(Math.max(...started.map(s => s.at))).toBeCloseTo(10.35);
  });
});
