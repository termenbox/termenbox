import { describe, expect, it } from 'vitest';
import { breadcrumb, displayPath, isUnderHome, parentOf } from './dirs';

describe('dirs', () => {
  it('knows what is under home', () => {
    expect(isUnderHome('/home/me', '/home/me')).toBe(true);
    expect(isUnderHome('/home/me/code', '/home/me')).toBe(true);
    expect(isUnderHome('/home/mean', '/home/me')).toBe(false);
    expect(isUnderHome('/srv', '/home/me')).toBe(false);
    expect(isUnderHome('/srv', '')).toBe(false);
  });
  it('finds the parent', () => {
    expect(parentOf('/home/me/code')).toBe('/home/me');
    expect(parentOf('/home')).toBe('/');
    expect(parentOf('/')).toBeNull();
    expect(parentOf('/a/b/')).toBe('/a');
  });
  it('builds a breadcrumb from ~ or from /', () => {
    expect(breadcrumb('/home/me/code/app', '/home/me')).toEqual([
      { name: '~', path: '/home/me' }, { name: 'code', path: '/home/me/code' }, { name: 'app', path: '/home/me/code/app' },
    ]);
    expect(breadcrumb('/home/me', '/home/me')).toEqual([{ name: '~', path: '/home/me' }]);
    expect(breadcrumb('/srv/work', '/home/me')).toEqual([
      { name: '/', path: '/' }, { name: 'srv', path: '/srv' }, { name: 'work', path: '/srv/work' },
    ]);
  });
  it('shortens a path with ~ and a middle ellipsis that keeps the tail', () => {
    expect(displayPath('/home/me', '/home/me', 40)).toBe('~');
    expect(displayPath('/home/me/code/termenbox', '/home/me', 40)).toBe('~/code/termenbox');
    expect(displayPath('/srv/work', '/home/me', 40)).toBe('/srv/work');
    const long = displayPath('/home/me/code/some/deeply/nested/termenbox-app', '/home/me', 24);
    expect(long.length).toBe(24);
    expect(long).toContain('…');
    expect(long.endsWith('termenbox-app')).toBe(true);
    expect(long.startsWith('~/')).toBe(true);
  });
});
