import { describe, expect, it } from 'vitest';
import { backendFor, COLD_START_AGENTS, getRecentDirs, inlineAgents, mergeRecentDirs, saveRecentDir, loadView, managerOptions, recentDirs, resolveView, saveView, suggestName, TEMPLATES, validDir, validName, viewControl } from './newworkspace';

const w = (path: string) => ({ index: 0, name: 'n', command: 'bash', path, active: false });

describe('new workspace', () => {
  it('offers every agent the server recognises, plus a plain shell', () => {
    const ids = TEMPLATES.map(t => t.id);
    for (const a of ['claude', 'codex', 'copilot', 'opencode', 'kilo', 'gemini', 'aider', 'cursor-agent', 'amp', 'goose', 'qwen', 'shell']) expect(ids).toContain(a);
  });
  it('names a workspace after its directory and agent, and keeps the name unique and valid', () => {
    expect(suggestName('/home/u/my app', 'claude', [])).toBe('my-app-claude');
    expect(suggestName('/srv/api', 'shell', [])).toBe('api');
    expect(suggestName('/srv/api', 'shell', ['api', 'api-2'])).toBe('api-3');
    expect(suggestName('', '', [])).toBe('workspace');
    expect(validName(suggestName('/x/' + 'a'.repeat(200), 'claude', []))).toBe(true);
    expect(validName(suggestName('/x/ünï', 'codex', []))).toBe(true);
  });
  it('lists directories in use on one server, most used first', () => {
    const windows = {
      's1/a': { windows: [w('/srv/app'), w('/srv/app')], representative: 0 },
      's1/b': { windows: [w('/srv/api')], representative: 0 },
      's2/c': { windows: [w('/other')], representative: 0 },
    };
    expect(recentDirs(windows, 's1/')).toEqual(['/srv/app', '/srv/api']);
    expect(recentDirs(windows, 's1/', 1)).toEqual(['/srv/app']);
  });
  it('accepts only absolute, clean directories (or none)', () => {
    expect(validDir('')).toBe(true);
    expect(validDir('/srv/app')).toBe(true);
    expect(validDir('app')).toBe(false);
    expect(validDir('/a\nb')).toBe(false);
  });
});

const mem = (initial: Record<string, string> = {}) => {
  const m = { ...initial };
  return { getItem: (k: string) => m[k] ?? null, setItem: (k: string, v: string) => { m[k] = v; }, raw: m };
};
const throwing = { getItem: () => { throw new Error('denied'); }, setItem: () => { throw new Error('denied'); } };
const both = { backends: ['tmux', 'acp'], acpAgents: ['claude', 'opencode', 'mine'] };

describe('the view', () => {
  it('is a choice only where the server has both views and the agent has an ACP form', () => {
    expect(viewControl(both, 'claude')).toBe('choose');
    expect(viewControl(both, 'shell')).toBe('terminal');
    expect(viewControl(both, 'codex')).toBe('terminal');
    expect(viewControl({ backends: ['tmux'], acpAgents: ['claude'] }, 'claude')).toBe('terminal');
    expect(viewControl({ backends: ['acp'], acpAgents: ['claude'] }, 'claude')).toBe('terminal');
    expect(viewControl(undefined, 'claude')).toBe('terminal');
  });
  it('is always the agent view for an ACP agent that tmux cannot start', () => {
    expect(viewControl(both, 'mine')).toBe('acp');
    expect(resolveView(both, 'mine', 'terminal')).toBe('acp');
  });
  it('applies the wanted view only where there is a choice, so a remembered acp for a gone agent is the terminal', () => {
    expect(resolveView(both, 'claude', 'acp')).toBe('acp');
    expect(resolveView(both, 'claude', 'terminal')).toBe('terminal');
    expect(resolveView(both, 'codex', 'acp')).toBe('terminal');
    expect(resolveView({ backends: ['tmux', 'acp'], acpAgents: [] }, 'claude', 'acp')).toBe('terminal');
  });
  it('derives the backend from the view and leaves other managers alone', () => {
    expect(backendFor(both, 'tmux', 'claude', 'acp')).toBe('acp');
    expect(backendFor(both, 'tmux', 'claude', 'terminal')).toBe('tmux');
    expect(backendFor(both, 'tmux', 'shell', 'acp')).toBe('tmux');
    const withCommand = { backends: ['tmux', 'acp', 'command'], acpAgents: ['claude'] };
    expect(backendFor(withCommand, 'command', 'claude', 'acp')).toBe('command');
    expect(managerOptions(withCommand)).toEqual(['tmux', 'command']);
    expect(managerOptions(both)).toEqual(['tmux']);
    // acp and command, no tmux: the Manager dropdown stays and lists both.
    const noTmux = { backends: ['command', 'acp'], acpAgents: ['claude'] };
    expect(managerOptions(noTmux)).toEqual(['command', 'acp']);
    expect(backendFor(noTmux, 'acp', 'claude', 'acp')).toBe('acp');
    expect(backendFor(noTmux, 'command', 'claude', 'acp')).toBe('command');
  });
  it('defaults to the terminal and remembers the choice per server and agent', () => {
    const st = mem();
    expect(loadView(st, 's1', 'claude')).toBe('terminal');
    saveView(st, 's1', 'claude', 'acp');
    expect(loadView(st, 's1', 'claude')).toBe('acp');
    expect(loadView(st, 's1', 'opencode')).toBe('terminal');
    expect(loadView(st, 's2', 'claude')).toBe('terminal');
    saveView(st, 's1', 'opencode', 'acp');
    saveView(st, 's1', 'claude', 'terminal');
    expect(loadView(st, 's1', 'claude')).toBe('terminal');
    expect(loadView(st, 's1', 'opencode')).toBe('acp');
  });
  it('ignores a corrupt or hostile stored value', () => {
    expect(loadView(mem({ termenbox_new_session_view: 'not json' }), 's1', 'claude')).toBe('terminal');
    expect(loadView(mem({ termenbox_new_session_view: '{"s1/claude":"root"}' }), 's1', 'claude')).toBe('terminal');
    expect(loadView(mem({ termenbox_new_session_view: '[1]' }), 's1', 'claude')).toBe('terminal');
    const st = mem({ termenbox_new_session_view: 'not json' });
    saveView(st, 's1', 'claude', 'acp');
    expect(loadView(st, 's1', 'claude')).toBe('acp');
  });
  it('keeps the choice in memory where storage is missing or throws', () => {
    expect(loadView(throwing, 'a', 'claude')).toBe('terminal');
    saveView(throwing, 'a', 'claude', 'acp');
    expect(loadView(throwing, 'a', 'claude')).toBe('acp');
    expect(loadView(undefined, 'a', 'claude')).toBe('terminal');
    saveView(undefined, 'a', 'claude', 'acp');
    expect(loadView(undefined, 'a', 'claude')).toBe('acp');
  });
});

const store = () => { const m = new Map<string, string>(); return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) }; };
const broken = { getItem: () => { throw new Error('off'); }, setItem: () => { throw new Error('off'); } };

describe('the agent tiles', () => {
  it('offers claude, codex, shell on a cold start of a tmux server', () => {
    expect(inlineAgents(TEMPLATES, []).map(t => t.id)).toEqual(COLD_START_AGENTS);
  });
  it('puts recent agents first and pads from the cold-start list', () => {
    expect(inlineAgents(TEMPLATES, ['gemini']).map(t => t.id)).toEqual(['gemini', 'claude', 'codex']);
    expect(inlineAgents(TEMPLATES, ['codex', 'gemini', 'aider', 'amp']).map(t => t.id)).toEqual(['codex', 'gemini', 'aider']);
  });
  it('never offers an unsupported agent', () => {
    const acp = TEMPLATES.filter(t => ['claude', 'gemini', 'shell'].includes(t.id));
    expect(inlineAgents(acp, ['codex', 'aider']).map(t => t.id)).toEqual(['claude', 'shell', 'gemini']);
    expect(inlineAgents([], ['claude'])).toEqual([]);
  });
});

describe('recent folders', () => {
  it('keeps the most recent first, once each, at most ten, per server', () => {
    const s = store();
    for (let i = 0; i < 12; i++) saveRecentDir('a', `/d${i}`, s);
    saveRecentDir('a', '/d5', s);
    saveRecentDir('b', '/other', s);
    const a = getRecentDirs('a', s);
    expect(a).toHaveLength(10);
    expect(a.slice(0, 2)).toEqual(['/d5', '/d11']);
    expect(getRecentDirs('b', s)).toEqual(['/other']);
  });
  it('ignores a relative path', () => {
    const s = store();
    saveRecentDir('a', 'code', s);
    expect(getRecentDirs('a', s)).toEqual([]);
  });
  it('falls back to memory when storage throws', () => {
    saveRecentDir('mem', '/x', broken);
    expect(getRecentDirs('mem', broken)).toEqual(['/x']);
  });
  it('survives with no open windows and seeds from them only when empty', () => {
    const s = store();
    saveRecentDir('a', '/kept', s);
    expect(mergeRecentDirs(getRecentDirs('a', s), {}, 'a/')).toEqual(['/kept']);
    const wins = { 'a/ws': { windows: [{ path: '/open', index: 0 }] } } as never;
    expect(mergeRecentDirs([], wins, 'a/')).toEqual(['/open']);
    expect(mergeRecentDirs(['/kept'], wins, 'a/')).toEqual(['/kept']);
  });
});

describe('server home memory', () => {
  it('keeps a home per server and survives storage that throws', async () => {
    const { getHome, saveHome } = await import('./newworkspace');
    const store = new Map<string, string>();
    const ok = { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => void store.set(k, v) };
    saveHome('a', '/home/a', ok);
    saveHome('b', '/home/b', ok);
    expect(getHome('a', ok)).toBe('/home/a');
    expect(getHome('b', ok)).toBe('/home/b');
    const boom = { getItem: () => { throw new Error('off'); }, setItem: () => { throw new Error('off'); } };
    saveHome('c', '/home/c', boom);
    expect(getHome('c', boom)).toBe('/home/c');
    expect(getHome('zzz', boom)).toBe('');
  });
});
