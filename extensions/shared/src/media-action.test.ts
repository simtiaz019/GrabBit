import { afterEach, describe, expect, it, vi } from 'vitest';

describe('on-page media action', () => {
  afterEach(() => {
    Reflect.deleteProperty(globalThis, 'chrome');
    document.body.replaceChildren();
  });

  it('shows on HTML media pages and forwards only the page URL on click', async () => {
    const sendMessage = vi.fn().mockResolvedValue({ ok: true });
    Object.defineProperty(globalThis, 'chrome', {
      configurable: true,
      value: { runtime: { sendMessage } },
    });
    document.body.innerHTML = '<video controls></video>';

    await import('./media-action');

    const button = document.querySelector<HTMLButtonElement>('#grabbit-media-action');
    expect(button).not.toBeNull();
    expect(button?.textContent).toBe('Download with GrabBit');
    button?.click();
    await vi.waitFor(() => expect(button?.textContent).toBe('Opened in GrabBit'));
    expect(sendMessage).toHaveBeenCalledWith({
      type: 'GRABBIT_INSPECT_PAGE_MEDIA',
      url: window.location.href,
    });
  });
});
