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

describe('settings', () => {
  it('links to the privacy page', () => {
    const out = render(SettingsView, { props: { user: 'octo', onservers() {}, onmenu() {}, onshortcuts() {}, onreport() {}, onlogout() {}, ondeletedata: async () => 0, historyServers: 0, ondeletehistory: async () => ({ servers: 0, removed: 0, failedNames: [] }) } }).body;
    expect(out).toContain('Privacy and your data');
  });
});
