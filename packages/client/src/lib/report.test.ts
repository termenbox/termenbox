import { describe, expect, it } from 'vitest';
import { buildReport, describeProblem, reportPlan, scrub, MAX_URL_TEXT, type Env } from './report';

// Realistic samples: a device certificate, a claim secret, a GitHub token, addresses.
const CERT = 'eyJ2IjoxLCJraWQiOiJrMSIsInN1YiI6NDIsImxvZ2luIjoib2N0byJ9.mGt9Nv_YUuWy4UuPrwU4roLVMdX5b0l8f92FM0oHyk8aa';
const CLAIM = 'Qk3j9wZpV2xL0aRtY7uHn5dEoC1sMbGf8iKvXyT4zNw';
const TOKEN = 'gho_16C7e42F292c6912E7710c838347Ae178B4a';

describe('scrub', () => {
  it('removes certificates, claim secrets, tokens, addresses and host:port pairs', () => {
    const dirty = `cert ${CERT} claim ${CLAIM} token ${TOKEN} ip 203.0.113.7:4443 v6 2001:db8:85a3::8a2e:370:7334 host vps.example.dev:4443`;
    const clean = scrub(dirty);
    for (const secret of [CERT, CLAIM, TOKEN, '203.0.113.7', '4443', '2001:db8', 'vps.example.dev']) {
      expect(clean).not.toContain(secret);
    }
    expect(clean).toContain('[cert]');
    expect(clean).toContain('[ip]');
  });

  it('keeps ordinary diagnostics readable', () => {
    expect(scrub('TypeError: Cannot read properties of undefined (reading "onRequestRedraw")')).toContain('onRequestRedraw');
    expect(scrub('window 390x800')).toBe('window 390x800');
  });
});

describe('buildReport and reportPlan', () => {
  const env: Env = {
    version: '/assets/index-abc.js',
    userAgent: 'Mozilla/5.0 (iPhone)',
    inner: '390x800',
    visual: '390x460 top 120',
    connection: 'connected',
    errors: [{ at: 0, text: `failed for ${CERT} at 10.0.0.5:4443` }],
  };

  it('never lets a secret into the final text', () => {
    const text = buildReport(env);
    expect(text).toContain('connection: connected');
    expect(text).not.toContain(CERT);
    expect(text).not.toContain('10.0.0.5');
  });

  it('puts short text in the URL and long text on the clipboard', () => {
    const short = reportPlan('abc');
    expect(short.url).toContain('template=problem.yml');
    expect(short.url).toContain('diagnostics=abc');
    expect(short.copy).toBeUndefined();
    const long = reportPlan('x'.repeat(MAX_URL_TEXT + 1));
    expect(long.copy).toHaveLength(MAX_URL_TEXT + 1);
    expect(long.url.length).toBeLessThan(400);
  });
});

describe('describeProblem', () => {
  it('structures the description and leaves it empty when nothing was written', () => {
    expect(describeProblem({ steps: '', actual: ' ', expected: '' })).toBe('');
    expect(describeProblem({ steps: '1. open', actual: 'shifts left', expected: '' })).toBe('Steps to reproduce:\n1. open\n\nActual result:\nshifts left\n\nExpected result:\n-');
  });
  it('puts the description into the issue address', () => {
    expect(reportPlan('diag', 'Actual result:\nx').url).toContain('&what=Actual%20result%3A%0Ax&diagnostics=diag');
  });
});

describe('steps in the report', () => {
  it('lists the steps and says so when there are none', () => {
    const base = { version: null, userAgent: 'x', inner: '1x1', visual: '1x1', connection: 'connected', errors: [] };
    expect(buildReport({ ...base, steps: ['1. open workspace tab (3 s ago)'] })).toContain('steps before this report:\n1. open workspace tab (3 s ago)');
    expect(buildReport(base)).toContain('steps before this report:\n(none)');
  });
});
