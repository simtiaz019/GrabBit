import { describe, expect, it } from 'vitest';
import { hostExcluded, isEligibleUrl, safeSuggestedFileName, shouldIntercept } from './policy';

describe('browser capture policy', () => {
  it('accepts only credential-free HTTP URLs', () => {
    expect(isEligibleUrl('https://example.com/file')).toBe(true);
    expect(isEligibleUrl('file:///tmp/secret')).toBe(false);
    expect(isEligibleUrl('https://user:secret@example.com/file')).toBe(false);
  });
  it('matches exact sites and subdomains without matching suffix attacks', () => {
    expect(hostExcluded('https://cdn.example.com/a', ['example.com'])).toBe(true);
    expect(hostExcluded('https://notexample.com/a', ['example.com'])).toBe(false);
  });
  it('intercepts only when the canonical capture mode is automatic', () => {
    expect(shouldIntercept('https://example.com/a', { mode: 'automatic', excludedSites: [] })).toBe(true);
    expect(shouldIntercept('https://example.com/a', { mode: 'ask', excludedSites: [] })).toBe(false);
    expect(shouldIntercept('https://example.com/a', { mode: 'off', excludedSites: [] })).toBe(false);
  });
  it('reduces Chrome absolute filenames to a protocol-safe basename', () => {
    expect(safeSuggestedFileName('/Users/me/Downloads/CentOS-Stream-9.iso')).toBe('CentOS-Stream-9.iso');
    expect(safeSuggestedFileName('C:\\Users\\me\\Downloads\\fixture.bin')).toBe('fixture.bin');
  });
});
