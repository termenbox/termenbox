import { writable } from 'svelte/store';

export const SKINS = ['navy', 'graphite', 'teal'] as const;
export type Skin = (typeof SKINS)[number];
export interface Settings { theme: 'dark' | 'light' | 'system'; /** The palette (decision 0110); the theme picks its dark or light variant. */ skin: Skin; cacheTtlMs: 300000 | 3600000; /** What Keep warm sends; empty hides the button. */ keepWarm: string; /** WebGL is fast; the DOM renderer is the plain fallback to try when the screen flickers or goes blank. */ renderer: 'webgl' | 'dom'; /** Play a short sound when a session starts needing you. */ alertSound: boolean; /** Vibrate (where the browser can) when a session starts needing you. */ alertVibrate: boolean }
const KEY = 'termenbox_settings';
const DEFAULT: Settings = { theme: 'dark', skin: 'navy', cacheTtlMs: 300000, keepWarm: '', renderer: 'webgl', alertSound: true, alertVibrate: true };
const storage = () => typeof localStorage === 'undefined' ? undefined : localStorage;
export function loadSettings(store: Pick<Storage, 'getItem'> | undefined = storage()): Settings {
  try {
    const saved = JSON.parse(store?.getItem(KEY) ?? 'null');
    return { theme: ['dark', 'light', 'system'].includes(saved?.theme) ? saved.theme : DEFAULT.theme,
      skin: (SKINS as readonly string[]).includes(saved?.skin) ? saved.skin : DEFAULT.skin,
      cacheTtlMs: saved?.cacheTtlMs === 3600000 ? 3600000 : 300000,
      keepWarm: typeof saved?.keepWarm === 'string' ? saved.keepWarm.replace(/[\r\n]+/g, ' ').slice(0, 200) : '',
      renderer: saved?.renderer === 'dom' ? 'dom' : 'webgl',
      alertSound: saved?.alertSound !== false,
      alertVibrate: saved?.alertVibrate !== false };
  } catch { return { ...DEFAULT }; }
}
export function saveSettings(value: Settings, store: Pick<Storage, 'setItem'> | undefined = storage()) {
  try { store?.setItem(KEY, JSON.stringify(value)); } catch { /* Private mode: settings still work in memory. */ }
}
export const settings = writable(loadSettings());
export function changeSettings(patch: Partial<Settings>) { settings.update(value => { const next = { ...value, ...patch }; saveSettings(next); return next; }); }
export function applyTheme(theme: Settings['theme'], prefersLight: boolean, skin: Skin = DEFAULT.skin) {
  document.documentElement.dataset.theme = theme === 'system' ? (prefersLight ? 'light' : 'dark') : theme;
  document.documentElement.dataset.skin = skin;
}
