// Pure helpers behind DialogLayer and PickerSheet, kept out of the components so vitest (no DOM here) can cover them.

export const FOCUSABLE = 'a[href],button:not([disabled]),input:not([disabled]),select:not([disabled]),textarea:not([disabled]),[tabindex]:not([tabindex="-1"])';

/** Next listbox index for a key, or null when the key is not a listbox key. Arrows clamp (no wrap), Home/End jump. */
export function listboxIndex(key: string, current: number, count: number): number | null {
  if (count <= 0) return null;
  switch (key) {
    case 'ArrowDown': return Math.min(current + 1, count - 1);
    case 'ArrowUp': return Math.max(current - 1, 0);
    case 'Home': return 0;
    case 'End': return count - 1;
    default: return null;
  }
}

/** Where Tab goes inside a trapped layer: wraps at both ends. `index` is the focused item's position, -1 if none. */
export function trapTarget(shift: boolean, index: number, count: number): number | null {
  if (count <= 0) return null;
  if (shift) return index <= 0 ? count - 1 : null;
  return index === count - 1 || index < 0 ? 0 : null;
}

/** Case-insensitive match on label or value, as the old picker did. */
export function filterOptions<T extends { label: string; value: unknown }>(options: T[], query: string): T[] {
  const q = query.trim().toLowerCase();
  if (!q) return options;
  return options.filter(o => o.label.toLowerCase().includes(q) || String(o.value).toLowerCase().includes(q));
}
