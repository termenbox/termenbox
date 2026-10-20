import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import PrivacyView from './PrivacyView.svelte';
import SettingsView from './SettingsView.svelte';

const html = () => render(PrivacyView, { props: { ondelete: async () => 0, onback: () => {}, onlogout: () => {}, historyServers: 2, ondeletehistory: async () => ({ servers: 2, removed: 5, failedNames: [] }) } }).body;

describe('the privacy page', () => {
  it('states what is stored and the 30-day idle retention', () => {
    const out = html();
    expect(out).toContain('What we store');
    expect(out).toContain('GitHub account id');
    expect(out).toContain('IP address, port, security fingerprint');
    expect(out).toContain('30 days');
  });

  it('starts with nothing destructive armed: only the first, non-final step is offered', () => {
    const out = html();
    expect(out).toContain('data-delete-step="idle"');
    expect(out).toContain('Delete my data…');
    expect(out).not.toContain('This cannot be undone');
  });
});

describe('the history notice says what is kept', () => {
  const out = html();
  it('names the title source and exactly what is scrubbed, and what is never kept', () => {
    expect(out).toContain('the first line of your prompt');
    expect(out).toContain('email addresses, links and paths in your home folder are replaced');
    expect(out).toContain('other words stay as you wrote them');
    expect(out).toContain('a plan');
    expect(out).toContain('Claude Code hooks, Codex, ACP agents');
    expect(out).not.toContain('addresses replaced;');
  });
});

describe('the notice covers the counts (U13)', () => {
  const out = html();
  it('says counts are numbers the agent reported, that the transcript is read for numbers only, and what an empty field means', () => {
    expect(out).toContain('the name, not the path');
    expect(out).toContain('counts the agent itself reported, as numbers');
    expect(out).toContain('as numbers only; nothing else from that file is kept');
    expect(out).toContain('leaves these empty, and Activity says so');
  });
});

describe('settings', () => {
  it('links to the privacy page', () => {
    const out = render(SettingsView, { props: { user: 'octo', onservers() {}, onmenu() {}, onshortcuts() {}, onreport() {}, onlogout() {}, ondeletedata: async () => 0, historyServers: 0, ondeletehistory: async () => ({ servers: 0, removed: 0, failedNames: [] }) } }).body;
    expect(out).toContain('Privacy and your data');
  });
});
