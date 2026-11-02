import type { UploadSource } from './uploadQueue';

/** The slice of a `DataTransfer` / `ClipboardEvent.clipboardData` these helpers read. */
export interface TransferLike {
  types?: readonly string[];
  files?: ArrayLike<UploadSource>;
  getData?(type: string): string;
}

/** Files carried by a drop or a paste (a File in `files`; `types` has "Files"). */
export function filesOf(t: TransferLike | null | undefined): UploadSource[] {
  return t?.files ? Array.from(t.files) : [];
}

/** A drag that carries files (text selections and links are left to the page). */
export function dragHasFiles(t: TransferLike | null | undefined): boolean {
  return !!t?.types && Array.from(t.types).includes('Files');
}

/**
 * Whether a paste is ours: the clipboard holds a file or an image and no text. Plain text, bracketed paste
 * included, must keep going to the terminal.
 */
export function takesPaste(t: TransferLike | null | undefined): boolean {
  if (filesOf(t).length === 0) return false;
  const text = t?.getData?.('text/plain') ?? '';
  return text === '';
}

/** Names a pasted image has no good name for ("image.png" from every screenshot): add the time so a batch stays apart. */
export function pastedName(name: string, type: string, at = new Date()): string {
  if (name && name !== 'image.png') return name;
  const ext = type.split('/')[1]?.replace(/[^a-z0-9]/gi, '') || 'png';
  const p = (n: number) => String(n).padStart(2, '0');
  return `pasted-${at.getFullYear()}${p(at.getMonth() + 1)}${p(at.getDate())}-${p(at.getHours())}${p(at.getMinutes())}${p(at.getSeconds())}.${ext}`;
}
