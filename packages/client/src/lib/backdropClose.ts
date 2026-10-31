/** Closes a modal <dialog> on a click that both starts and ends on its backdrop: a drag that begins in a field (selecting text) and is released outside must not dismiss it. */
export function backdropClose(node: HTMLDialogElement, close: () => void) {
  let downOnBackdrop = false;
  const down = (e: PointerEvent) => { downOnBackdrop = e.target === node; };
  const click = (e: MouseEvent) => {
    const start = downOnBackdrop;
    downOnBackdrop = false;
    if (e.target === node && start) close();
  };
  node.addEventListener('pointerdown', down);
  node.addEventListener('click', click);
  return {
    update(next: () => void) { close = next; },
    destroy() {
      node.removeEventListener('pointerdown', down);
      node.removeEventListener('click', click);
    },
  };
}
