// Full-colour marks for coding agents and common programs (plan H.5). They are our own simple badges in each
// product's usual colour, drawn locally: nothing is fetched, and no logo is copied. Unknown names get a neutral badge.
/**
 * `glyph` is the text mark. A glyph that some platform draws as a colour emoji (iOS draws ✳ as a green emoji and ⚡ as a
 * yellow one) would hide the badge's product colour, so such a mark has `shape` instead: an inline SVG (24x24 box,
 * drawn in the badge's white), which looks the same everywhere. `glyph` stays as the plain-text fallback and the key
 * the tests read. `emoji: true` marks a glyph that is meant as a colour emoji.
 */
export interface IconSpec { color: string; glyph: string; label: string; emoji?: boolean; shape?: string }

const SHAPES = {
  asterisk: '<path d="M12 3v18M4.2 7.5l15.6 9M4.2 16.5l15.6-9" stroke="#fff" stroke-width="2.6" stroke-linecap="round" fill="none"/>',
  ring: '<circle cx="12" cy="12" r="6.5" stroke="#fff" stroke-width="2.6" fill="none"/><circle cx="12" cy="12" r="2" fill="#fff"/>',
  spark: '<path d="M12 2.5C12.9 8.2 15.8 11.1 21.5 12 15.8 12.9 12.9 15.8 12 21.5 11.1 15.8 8.2 12.9 2.5 12 8.2 11.1 11.1 8.2 12 2.5Z" fill="#fff"/>',
  triangle: '<path d="M12 4 21 20H3Z" fill="#fff"/>',
  bolt: '<path d="M13.5 2 5 13.5h5.5L9.5 22 19 10h-5.5Z" fill="#fff"/>',
} as const;

const AGENT: Record<string, IconSpec> = {
  claude: { color: '#D97757', glyph: '✳', label: 'Claude Code', shape: SHAPES.asterisk },
  codex: { color: '#10A37F', glyph: '◎', label: 'Codex', shape: SHAPES.ring },
  copilot: { color: '#8957E5', glyph: 'Co', label: 'Copilot' },
  opencode: { color: '#2F81F7', glyph: '</>', label: 'OpenCode' },
  kilo: { color: '#E8B923', glyph: 'K', label: 'Kilo' },
  gemini: { color: '#4285F4', glyph: '✦', label: 'Gemini', shape: SHAPES.spark },
  aider: { color: '#2EA043', glyph: 'Ai', label: 'Aider' },
  'cursor-agent': { color: '#6E7681', glyph: '▲', label: 'Cursor', shape: SHAPES.triangle },
  amp: { color: '#F25A3C', glyph: '⚡', label: 'Amp', shape: SHAPES.bolt },
  goose: { color: '#3E9AD6', glyph: 'G', label: 'Goose' },
  qwen: { color: '#7C3AED', glyph: 'Q', label: 'Qwen' },
};
const PROGRAM: Record<string, IconSpec> = {
  bash: { color: '#4EAA25', glyph: '>_', label: 'bash' },
  zsh: { color: '#F15A24', glyph: '>_', label: 'zsh' },
  fish: { color: '#3B9ED0', glyph: '>_', label: 'fish' },
  sh: { color: '#6E7681', glyph: '>_', label: 'sh' },
  mc: { color: '#1F6FEB', glyph: '▤', label: 'Midnight Commander' },
  lazygit: { color: '#F05133', glyph: '⎇', label: 'lazygit' },
  git: { color: '#F05133', glyph: '⎇', label: 'git' },
  vim: { color: '#019733', glyph: 'V', label: 'vim' },
  nvim: { color: '#57A143', glyph: 'N', label: 'neovim' },
  nano: { color: '#4A90D9', glyph: 'n', label: 'nano' },
  htop: { color: '#D29922', glyph: '▥', label: 'htop' },
  top: { color: '#D29922', glyph: '▥', label: 'top' },
  ssh: { color: '#8B949E', glyph: '⌁', label: 'ssh' },
  node: { color: '#5FA04E', glyph: '⬢', label: 'node' },
  python: { color: '#3776AB', glyph: 'Py', label: 'python' },
  python3: { color: '#3776AB', glyph: 'Py', label: 'python' },
  docker: { color: '#2496ED', glyph: '🐳', label: 'docker', emoji: true },
  tmux: { color: '#1BB91F', glyph: '▦', label: 'tmux' },
  less: { color: '#8B949E', glyph: '≡', label: 'less' },
  cargo: { color: '#CE412B', glyph: '▣', label: 'cargo' },
  npm: { color: '#CB3837', glyph: 'n', label: 'npm' },
  pnpm: { color: '#F69220', glyph: 'p', label: 'pnpm' },
};
/** Every badge, for the test that none can render as an emoji. */
export const ALL_ICONS: Record<string, IconSpec> = { ...AGENT, ...PROGRAM };
const FALLBACK: IconSpec = { color: '#6E7681', glyph: '·', label: 'program' };

/** The icon for a foreground command or a known agent name. */
export function iconFor(name: string | undefined, agent?: string): IconSpec {
  if (agent && AGENT[agent]) return AGENT[agent];
  const n = (name ?? '').toLowerCase().replace(/^-/, '');
  return AGENT[n] ?? PROGRAM[n] ?? { ...FALLBACK, label: name || FALLBACK.label };
}
export const isAgentIcon = (name: string | undefined, agent?: string) => !!(agent && AGENT[agent]) || !!(name && AGENT[name.toLowerCase()]);
