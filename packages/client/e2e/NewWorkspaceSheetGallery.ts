// Standalone test page for NewWorkspaceSheet screenshots
import { mount } from 'svelte';
import '../src/app.css';
import NewWorkspaceSheet from '../src/NewWorkspaceSheet.svelte';
import { HubServer, HubWorkspace } from '../src/lib/hub';
import type { WindowMap } from '../src/lib/windows';

// Mock data
const mockServers: HubServer[] = [
  {
    id: 'server-1',
    server: { server_name: 'dev-server', registered_at: Date.now() },
    backends: ['tmux', 'acp'],
    acpAgents: ['claude', 'codex', 'gemini', 'opencode', 'copilot', 'kilo', 'aider', 'goose'],
    status: 'connected',
    lastSeen: Date.now(),
  },
  {
    id: 'server-2',
    server: { server_name: 'prod-server', registered_at: Date.now() },
    backends: ['tmux'],
    acpAgents: [],
    status: 'connected',
    lastSeen: Date.now(),
  },
];

const mockWorkspaces: HubWorkspace[] = [
  { id: 'ws-1', serverId: 'server-1', name: 'termenbox-claude', dir: '/home/user/project1', agent: 'claude', state: 'working' },
  { id: 'ws-2', serverId: 'server-1', name: 'termenbox-codex', dir: '/home/user/project2', agent: 'codex', state: 'idle' },
];

const mockWindows: WindowMap = {
  'server-1/ws-1': { windows: [{ path: '/home/user/project1' }] },
  'server-1/ws-2': { windows: [{ path: '/home/user/project2' }] },
  'server-1/ws-3': { windows: [{ path: '/home/user/other-project' }] },
  'server-1/ws-4': { windows: [{ path: '/home/user/another-project' }] },
  'server-1/ws-5': { windows: [{ path: '/home/user/yet-another-project' }] },
  'server-1/ws-6': { windows: [{ path: '/home/user/more-projects' }] },
};

const app = mount(NewWorkspaceSheet, {
  target: document.body,
  props: {
    servers: mockServers,
    initialServerId: 'server-1',
    locked: false,
    workspaces: mockWorkspaces,
    windows: mockWindows,
    oncreate: (req) => console.log('create', req),
    onclose: () => console.log('close'),
  },
});

export default app;