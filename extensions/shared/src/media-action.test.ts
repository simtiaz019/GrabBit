import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const cleanupKey = '__grabbitMediaActionCleanup';

function setVideoSize(video: HTMLVideoElement, width = 640, height = 360): void {
  vi.spyOn(video, 'getBoundingClientRect').mockReturnValue({
    x: 0, y: 0, top: 0, left: 0, right: width, bottom: height, width, height,
    toJSON: () => ({}),
  });
}

async function loadAction(sendMessage = vi.fn().mockResolvedValue({ ok: true })): Promise<typeof sendMessage> {
  Object.defineProperty(globalThis, 'chrome', {
    configurable: true,
    value: { runtime: { sendMessage, getURL: (path: string) => `chrome-extension://test/${path}` } },
  });
  await import('./media-action');
  return sendMessage;
}

describe('on-page media action', () => {
  beforeEach(() => vi.resetModules());

  afterEach(() => {
    (window as Window & { [cleanupKey]?: () => void })[cleanupKey]?.();
    Reflect.deleteProperty(window, cleanupKey);
    Reflect.deleteProperty(globalThis, 'chrome');
    document.body.replaceChildren();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('detects an HTML5 video and places the branded action on its player', async () => {
    document.body.innerHTML = '<div id="player"><video controls></video></div>';
    setVideoSize(document.querySelector('video')!);
    await loadAction();

    const button = document.querySelector<HTMLButtonElement>('#grabbit-media-action');
    expect(button?.parentElement?.id).toBe('player');
    expect(button?.title).toBe('Download with GrabBit');
    expect(button?.querySelector('img')?.src).toBe('chrome-extension://test/icon.png');
    expect(button?.style.right).toBe('10px');
    expect(button?.style.bottom).toBe('48px');
  });

  it('does not create duplicate actions after DOM mutations', async () => {
    vi.useFakeTimers();
    document.body.innerHTML = '<div><video></video></div>';
    setVideoSize(document.querySelector('video')!);
    await loadAction();

    document.body.append(document.createElement('span'));
    await Promise.resolve();
    vi.advanceTimersByTime(200);
    expect(document.querySelectorAll('#grabbit-media-action')).toHaveLength(1);
  });

  it('moves the action when a dynamic page replaces its video player', async () => {
    vi.useFakeTimers();
    document.body.innerHTML = '<div id="old"><video></video></div>';
    setVideoSize(document.querySelector('video')!);
    await loadAction();

    const replacement = document.createElement('div');
    replacement.id = 'new';
    replacement.innerHTML = '<video></video>';
    setVideoSize(replacement.querySelector('video')!);
    document.body.replaceChildren(replacement);
    await Promise.resolve();
    vi.advanceTimersByTime(200);

    expect(document.querySelector('#new > #grabbit-media-action')).not.toBeNull();
    expect(document.querySelectorAll('#grabbit-media-action')).toHaveLength(1);
  });

  it('opens existing native media inspection when clicked', async () => {
    document.body.innerHTML = '<div><video></video></div>';
    setVideoSize(document.querySelector('video')!);
    const sendMessage = await loadAction();

    document.querySelector<HTMLButtonElement>('#grabbit-media-action')?.click();
    expect(sendMessage).toHaveBeenCalledWith({
      type: 'GRABBIT_INSPECT_PAGE_MEDIA',
      url: window.location.href,
    });
  });

  it('hides the action when supported media is removed', async () => {
    vi.useFakeTimers();
    document.body.innerHTML = '<div><video></video></div>';
    setVideoSize(document.querySelector('video')!);
    await loadAction();

    document.querySelector('video')?.remove();
    await Promise.resolve();
    vi.advanceTimersByTime(200);
    expect(document.querySelector('#grabbit-media-action')).toBeNull();
  });

  it('ignores unsupported tiny and DRM-backed media', async () => {
    document.body.innerHTML = '<div><video id="tiny"></video><video id="drm"></video></div>';
    setVideoSize(document.querySelector<HTMLVideoElement>('#tiny')!, 100, 60);
    const drm = document.querySelector<HTMLVideoElement>('#drm')!;
    setVideoSize(drm);
    Object.defineProperty(drm, 'mediaKeys', { configurable: true, value: {} });
    await loadAction();

    expect(document.querySelector('#grabbit-media-action')).toBeNull();
  });
});
