/**
 * Remembers what has focus now and returns a function that gives it back. A modal <dialog> takes focus when it
 * opens, and Svelte removes it from the page without the browser's usual "return focus" step, so closing a
 * menu used to leave the terminal blurred (#107). Call it before the dialog opens, restore when it goes away.
 */
type FocusDoc = { activeElement: unknown; body: unknown; querySelector?: (selector: string) => unknown };

export function rememberFocus(doc: FocusDoc = typeof document === 'undefined' ? { activeElement: null, body: null } : document): () => void {
  const previous = (backToTerminal(doc) ?? doc.activeElement) as { isConnected: boolean; focus: (o?: { preventScroll: boolean }) => void } | null;
  return () => {
    if (previous && previous !== doc.body && previous.isConnected) previous.focus({ preventScroll: true });
  };
}

/** The terminal's input when the focus is on the control that opened the dialog (a button focused by the tap), so
 * closing the dialog returns to typing instead of to that button (R02). Null when the focus is anywhere else. */
function backToTerminal(doc: FocusDoc): unknown {
  const active = doc.activeElement as { tagName?: string; closest?: (s: string) => unknown } | null;
  if (active?.tagName !== 'BUTTON') return null;
  return doc.querySelector?.('.xterm-helper-textarea') ?? null;
}
