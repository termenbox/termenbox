import { describe, expect, it } from 'vitest';
import { backendFor, loadView, managerOptions, recentDirs, resolveView, saveView, suggestName, TEMPLATES, validDir, validName, viewControl } from './newsession';

const w = (path: string) => ({ index: 0, name: 'n', command: 'bash', path, active: false });

describe('new session', () => {
  it('offers every agent the server recognises, plus a plain shell', () => {
    const ids = TEMPLATES.map(t => t.id);
    for (const a of ['claude', 'codex', 'copilot', 'opencode', 'kilo', 'gemini', 'aider', 'cursor-agent', 'amp', 'goose', 'qwen', 'shell']) expect(ids).toContain(a);
  });
  it('names a session after its directory and agent, and keeps the name unique and valid', () => {
    expect(suggestName('/home/u/my app', 'claude', [])).toBe('my-app-claude');
    expect(suggestName('/srv/api', 'shell', [])).toBe('api');
    expect(suggestName('/srv/api', 'shell', ['api', 'api-2'])).toBe('api-3');
    expect(suggestName('', '', [])).toBe('session');
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
