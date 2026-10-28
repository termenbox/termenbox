import { describe, expect, it, vi } from 'vitest';
import { askMicrophone, errorMessage, isIOS, mapResults, nextVoiceLang, parseVoiceLang, pickLanguage, recognizerFactory, Voice, type Recognizer, type RecognizerEvent } from './voice';

const ev = (resultIndex: number, ...rs: [string, boolean][]): RecognizerEvent => ({
  resultIndex,
  results: Object.assign(
    rs.map(([transcript, isFinal]) => ({ isFinal, 0: { transcript } })),
    {},
  ),
});

describe('mapResults', () => {
  it('shows interim text and types nothing for it', () => {
    expect(mapResults(ev(0, ['hello wor', false]), false)).toEqual({ typed: '', interim: 'hello wor' });
  });

  it('types a final result trimmed, without a leading space for the first phrase', () => {
    expect(mapResults(ev(0, [' hello world', true]), false)).toEqual({ typed: 'hello world', interim: '' });
  });

  it('puts one space before later phrases, whether or not the browser supplied it', () => {
    expect(mapResults(ev(0, ['next one', true]), true).typed).toBe(' next one');
    expect(mapResults(ev(0, [' next one', true]), true).typed).toBe(' next one');
  });

  it('reads only from resultIndex on, and splits final from interim', () => {
    const e = ev(1, ['old', true], ['and then', true], ['some', false]);
    expect(mapResults(e, false)).toEqual({ typed: 'and then', interim: 'some' });
  });

  it('works for Russian', () => {
    expect(mapResults(ev(0, ['привет мир', true]), false).typed).toBe('привет мир');
  });

  it('ignores empty results', () => {
    expect(mapResults(ev(0, ['  ', true]), false)).toEqual({ typed: '', interim: '' });
  });
});

describe('errorMessage', () => {
  it('explains a blocked microphone and stays quiet on silence', () => {
    expect(errorMessage('not-allowed')).toMatch(/blocked/);
    expect(errorMessage('no-speech')).toBeNull();
    expect(errorMessage('weird')).toMatch(/weird/);
  });
});

describe('recognizerFactory', () => {
  it('is null when the browser has no recognizer, so the key is hidden', () => {
    expect(recognizerFactory({})).toBeNull();
  });
  it('uses the prefixed constructor', () => {
    class R {}
    expect(recognizerFactory({ webkitSpeechRecognition: R })?.()).toBeInstanceOf(R);
  });
});

function fake() {
  const rec: Recognizer = {
    lang: '',
    continuous: false,
    interimResults: false,
    onresult: null,
    onerror: null,
    onend: null,
    start: vi.fn(),
    stop: vi.fn(() => rec.onend?.()),
  };
  return rec;
}

describe('Voice', () => {
  const setup = (ask: () => Promise<string | null> = async () => null) => {
    const rec = fake();
    const on = { onText: vi.fn(), onInterim: vi.fn(), onState: vi.fn(), onError: vi.fn() };
    const voice = new Voice(() => rec, () => 'ru-RU', on, ask);
    return { rec, on, voice };
  };

  it('listens in the given language, continuously, with interim results', async () => {
    const { rec, on, voice } = setup();
    await voice.toggle();
    expect(rec.lang).toBe('ru-RU');
    expect(rec.continuous && rec.interimResults).toBe(true);
    expect(rec.start).toHaveBeenCalled();
    expect(on.onState).toHaveBeenLastCalledWith(true);
  });

  it('asks for the microphone before starting', async () => {
    const ask = vi.fn(async () => null);
    const { rec, voice } = setup(ask);
    await voice.toggle();
    expect(ask).toHaveBeenCalledTimes(1);
    expect(rec.start).toHaveBeenCalled();
  });

  it('checks for the microphone only once after it is granted', async () => {
    const ask = vi.fn(async () => null);
    const { rec, voice } = setup(ask);
    await voice.toggle();
    await voice.toggle(); // stop
    await voice.toggle(); // start again
    expect(ask).toHaveBeenCalledTimes(1);
    expect(rec.start).toHaveBeenCalledTimes(2);
  });

  it('does not start when the microphone is refused, and does not stay pressed', async () => {
    const ask = vi.fn(async () => errorMessage('not-allowed')!);
    const { rec, on, voice } = setup(ask);
    await voice.toggle();
    expect(rec.start).not.toHaveBeenCalled();
    expect(on.onError).toHaveBeenCalledWith(expect.stringMatching(/blocked/));
    expect(on.onState).toHaveBeenLastCalledWith(false);
    expect(voice.listening).toBe(false);
  });

  it('ignores a second tap while waiting for the permission answer', async () => {
    let answer!: (message: string | null) => void;
    const ask = vi.fn(() => new Promise<string | null>((resolve) => (answer = resolve)));
    const { rec, on, voice } = setup(ask);
    const starting = voice.toggle();
    expect(voice.listening).toBe(false);
    await voice.toggle(); // a second tap leaves the browser prompt alone
    expect(on.onState).not.toHaveBeenCalledWith(true);
    answer(null);
    await starting;
    expect(ask).toHaveBeenCalledTimes(1);
    expect(rec.start).toHaveBeenCalledTimes(1);
    expect(on.onState).toHaveBeenLastCalledWith(true);
  });

  it('does not start when stopped while the permission answer is pending', async () => {
    let answer!: (message: string | null) => void;
    const ask = vi.fn(() => new Promise<string | null>((resolve) => (answer = resolve)));
    const { rec, on, voice } = setup(ask);
    const starting = voice.toggle();
    voice.stop(); // e.g. the view closes while the prompt is open
    answer(null);
    await starting;
    expect(rec.start).not.toHaveBeenCalled();
    expect(on.onState).not.toHaveBeenCalledWith(true);
  });

  it('types final phrases with a space between them, and clears the interim line at the end', async () => {
    const { rec, on, voice } = setup();
    await voice.toggle();
    rec.onresult?.(ev(0, ['one two', false]));
    expect(on.onInterim).toHaveBeenLastCalledWith('one two');
    rec.onresult?.(ev(0, ['one two', true]));
    rec.onresult?.(ev(1, ['one two', true], ['three', true]));
    expect(on.onText.mock.calls).toEqual([['one two'], [' three']]);
    await voice.toggle();
    expect(voice.listening).toBe(false);
    expect(on.onInterim).toHaveBeenLastCalledWith('');
    expect(on.onState).toHaveBeenLastCalledWith(false);
  });

  it('starts a new workspace without a leading space', async () => {
    const { rec, on, voice } = setup();
    await voice.toggle();
    rec.onresult?.(ev(0, ['first', true]));
    await voice.toggle();
    await voice.toggle();
    rec.onresult?.(ev(0, ['second', true]));
    expect(on.onText.mock.calls).toEqual([['first'], ['second']]);
  });

  it('reports a denied microphone and resets even when no end event follows', async () => {
    const ask = vi.fn(async () => null);
    const { rec, on, voice } = setup(ask);
    await voice.toggle();
    expect(voice.listening).toBe(true);
    (rec.stop as ReturnType<typeof vi.fn>).mockImplementation(() => {}); // no onend may follow onerror
    rec.onerror?.({ error: 'not-allowed' });
    expect(on.onError).toHaveBeenCalledWith(expect.stringMatching(/blocked/));
    expect(on.onState).toHaveBeenLastCalledWith(false);
    expect(voice.listening).toBe(false);
    await voice.toggle(); // the key can be tapped again
    expect(ask).toHaveBeenCalledTimes(2); // a blocked mic is checked again
    expect(rec.start).toHaveBeenCalledTimes(2);
  });

  it('reports a recognizer that cannot start', async () => {
    const { rec, on, voice } = setup();
    (rec.start as ReturnType<typeof vi.fn>).mockImplementation(() => {
      throw new Error('busy');
    });
    await voice.toggle();
    expect(voice.listening).toBe(false);
    expect(on.onState).toHaveBeenLastCalledWith(false);
    expect(on.onError).toHaveBeenCalled();
  });
});

describe('iOS fallback', () => {
  const setup = (ios: boolean, withFallback = true) => {
    const rec = fake();
    const on = { onText: vi.fn(), onInterim: vi.fn(), onState: vi.fn(), onError: vi.fn(), ...(withFallback ? { onFallback: vi.fn() } : {}) };
    const voice = new Voice(() => rec, () => 'en-US', on, async () => null, ios);
    return { rec, on, voice };
  };

  it.each(['not-allowed', 'service-not-allowed'])('points an iPhone at the keyboard mic on %s instead of "blocked"', async (code) => {
    const { rec, on, voice } = setup(true);
    await voice.toggle();
    rec.onerror?.({ error: code });
    expect((on as { onFallback: unknown }).onFallback).toHaveBeenCalledWith(code);
    expect(on.onError).not.toHaveBeenCalled();
    expect(on.onState).toHaveBeenLastCalledWith(false);
  });

  it('keeps the error elsewhere, with the code in it, and for other iOS errors', async () => {
    const other = setup(false);
    await other.voice.toggle();
    other.rec.onerror?.({ error: 'service-not-allowed' });
    expect(other.on.onError).toHaveBeenCalledWith(expect.stringContaining('(service-not-allowed)'));
    const ios = setup(true);
    await ios.voice.toggle();
    ios.rec.onerror?.({ error: 'audio-capture' });
    expect(ios.on.onError).toHaveBeenCalledWith('No microphone found.');
  });

  it('still shows the error when nothing handles the fallback', async () => {
    const { rec, on, voice } = setup(true, false);
    await voice.toggle();
    rec.onerror?.({ error: 'not-allowed' });
    expect(on.onError).toHaveBeenCalled();
  });

  it('reads an iPhone, an iPad and a desktop-mode iPad, and nothing else', () => {
    expect(isIOS({ userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)' })).toBe(true);
    expect(isIOS({ userAgent: 'Mozilla/5.0 (Macintosh)', platform: 'MacIntel', maxTouchPoints: 5 })).toBe(true);
    expect(isIOS({ userAgent: 'Mozilla/5.0 (Macintosh)', platform: 'MacIntel', maxTouchPoints: 0 })).toBe(false);
    expect(isIOS({ userAgent: 'Mozilla/5.0 (Linux; Android 14)' })).toBe(false);
    expect(isIOS(undefined)).toBe(false);
  });
});

describe('askMicrophone', () => {
  const devices = (getUserMedia: () => Promise<MediaStream>) => ({ getUserMedia } as unknown as MediaDevices);

  it('asks for the microphone and releases the device for the recognizer', async () => {
    const stop = vi.fn();
    const getUserMedia = vi.fn(async () => ({ getTracks: () => [{ stop }] }) as unknown as MediaStream);
    expect(await askMicrophone({ mediaDevices: devices(getUserMedia) })).toBeNull();
    expect(getUserMedia).toHaveBeenCalledWith({ audio: true });
    expect(stop).toHaveBeenCalled();
  });

  it('explains a missing microphone', async () => {
    const getUserMedia = async () => {
      throw Object.assign(new Error('missing'), { name: 'NotFoundError' });
    };
    expect(await askMicrophone({ mediaDevices: devices(getUserMedia) })).toMatch(/No microphone/);
  });

  it('explains a blocked microphone', async () => {
    const getUserMedia = async () => {
      throw Object.assign(new Error('blocked'), { name: 'NotAllowedError' });
    };
    expect(await askMicrophone({ mediaDevices: devices(getUserMedia) })).toMatch(/blocked/);
  });

  it('names the exception and the permission state when the microphone is blocked, and where to look on an iPhone', async () => {
    const getUserMedia = async () => {
      throw Object.assign(new Error('blocked'), { name: 'NotAllowedError' });
    };
    const permissions = { query: async () => ({ state: 'denied' }) as PermissionStatus };
    const msg = await askMicrophone({ mediaDevices: devices(getUserMedia), permissions, ios: true });
    expect(msg).toContain('[NotAllowedError, permission denied]');
    expect(msg).toMatch(/Settings > Safari > Microphone/);
    expect(await askMicrophone({ mediaDevices: devices(getUserMedia), permissions })).not.toMatch(/Safari/);
  });

  it('still explains a block when the Permissions API cannot be read', async () => {
    const getUserMedia = async () => {
      throw Object.assign(new Error('blocked'), { name: 'SecurityError' });
    };
    const permissions = { query: async () => { throw new TypeError('no microphone'); } };
    expect(await askMicrophone({ mediaDevices: devices(getUserMedia), permissions })).toContain('[SecurityError]');
  });

  it('does not block dictation on a failure that is not a denial (iOS InvalidStateError)', async () => {
    const getUserMedia = async () => {
      throw Object.assign(new Error('invalid'), { name: 'InvalidStateError' });
    };
    expect(await askMicrophone({ mediaDevices: devices(getUserMedia) })).toBeNull();
  });

  it('lets the recognizer ask when the browser has no MediaDevices', async () => {
    expect(await askMicrophone({})).toBeNull();
  });
});

describe('dictation language', () => {
  it('follows the script of the typed text when the choice is auto', () => {
    expect(pickLanguage('auto', 'latin', 'ru-RU')).toBe('en-US');
    expect(pickLanguage('auto', 'cyrillic', 'en-US')).toBe('ru-RU');
  });

  it('falls back to the browser language, then English', () => {
    expect(pickLanguage('auto', null, 'ru-RU')).toBe('ru-RU');
    expect(pickLanguage('auto', null, '')).toBe('en-US');
  });

  it('a forced language wins over what was typed', () => {
    expect(pickLanguage('en', 'cyrillic', 'ru-RU')).toBe('en-US');
    expect(pickLanguage('ru', 'latin', 'en-US')).toBe('ru-RU');
  });

  it('cycles auto -> en -> ru -> auto and reads stored values safely', () => {
    expect(nextVoiceLang('auto')).toBe('en');
    expect(nextVoiceLang('en')).toBe('ru');
    expect(nextVoiceLang('ru')).toBe('auto');
    expect(parseVoiceLang('ru')).toBe('ru');
    expect(parseVoiceLang('xx')).toBe('auto');
    expect(parseVoiceLang(null)).toBe('auto');
  });
});
