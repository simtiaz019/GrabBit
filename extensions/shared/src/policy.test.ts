import { describe, expect, it } from 'vitest';
import { hostExcluded, isEligibleUrl, shouldIntercept } from './policy';

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
  it('requires both global and automatic capture switches', () => {
    expect(shouldIntercept('https://example.com/a', { enabled: true, automatic: true, excludedSites: [] })).toBe(true);
    expect(shouldIntercept('https://example.com/a', { enabled: true, automatic: false, excludedSites: [] })).toBe(false);
  });
});
