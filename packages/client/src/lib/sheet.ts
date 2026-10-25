/** `closed` is for phones: the top bar's Workspaces button opens the sheet, so no strip of it stays on screen. */
export type SheetHeight = 'closed' | 'peek' | 'half' | 'full';
export const HEIGHTS: SheetHeight[] = ['peek', 'half', 'full'];
export function sheetHeight(height: SheetHeight, available: number): number {
  const space = Math.max(0, available);
  if (height === 'closed') return 0;
  return Math.min(space, height === 'peek' ? 96 : height === 'half' ? Math.max(96, space / 2) : space);
}
export function snapHeight(pixels: number, available: number): SheetHeight {
  return HEIGHTS.reduce((best, next) => Math.abs(sheetHeight(next, available) - pixels) < Math.abs(sheetHeight(best, available) - pixels) ? next : best);
}
