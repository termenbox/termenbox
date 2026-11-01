/** The class of this device for the attention meter: `phone` or `desktop`, never a name or an id. */
export function deviceClass(win: Pick<Window, 'matchMedia' | 'screen'> | undefined = typeof window === 'undefined' ? undefined : window): 'phone' | 'desktop' | 'other' {
  if (!win?.matchMedia) return 'other';
  const touch = win.matchMedia('(pointer: coarse)').matches;
  const small = Math.min(win.screen?.width ?? 0, win.screen?.height ?? 0) < 600;
  return touch && small ? 'phone' : 'desktop';
}
