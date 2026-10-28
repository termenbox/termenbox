// What happens when a name field is committed (Enter, the keyboard's done key, Save). One rule for the sheets that rename.

export type RenameOutcome = { kind: 'empty' } | { kind: 'unchanged' } | { kind: 'rename'; name: string };

/** An empty name keeps the field open (there is nothing to save, and closing would lose the user's place); the same name closes with nothing sent. */
export function renameOutcome(draft: string, current: string): RenameOutcome {
  const name = draft.trim();
  if (!name) return { kind: 'empty' };
  if (name === current) return { kind: 'unchanged' };
  return { kind: 'rename', name };
}
