export type BackAction = 'workspace-list' | 'close-sheet' | 'none';

/** What the Android back gesture does: leave the open workspace, else close the sheet, else let the wrapper minimise the app. */
export function backAction(s: { inWorkspace: boolean; sheetOpen: boolean }): BackAction {
  if (s.inWorkspace) return 'workspace-list';
  if (s.sheetOpen) return 'close-sheet';
  return 'none';
}
