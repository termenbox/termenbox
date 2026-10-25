// Swipe-back from an open workspace (decision 0101): a drag that starts in the left 20 px and goes right.
export const EDGE_PX = 20;
export const SWIPE_MIN_PX = 72;

export function edgeSwipeBack(start: { x: number; y: number }, end: { x: number; y: number }): boolean {
  const dx = end.x - start.x, dy = Math.abs(end.y - start.y);
  return start.x <= EDGE_PX && dx >= SWIPE_MIN_PX && dx > dy * 2;
}
