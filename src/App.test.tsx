import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App';
import { addDownload, cancelDownload, checkBackend, createMediaDownload, getBrowserDiagnostics, getBrowserSetup, getLaunchAtLogin, getOnboardingComplete, getSettings, inspectMedia, listDownloads, onBrowserMediaRequest, onProgress, onTrayNavigate, pauseDownload, probeUrl, removeDownload, repairBrowserIntegration, resumeDownload, retryDownload, saveSettings, setLaunchAtLogin, setOnboardingComplete } from './bridge';
import type { ProgressSnapshot } from './types';

vi.mock('./bridge', () => ({
  addDownload: vi.fn(), cancelDownload: vi.fn(), checkBackend: vi.fn(),
  chooseDownloadDirectory: vi.fn(), createMediaDownload: vi.fn(), getBrowserDiagnostics: vi.fn(), getBrowserSetup: vi.fn(), getLaunchAtLogin: vi.fn(), getOnboardingComplete: vi.fn(), getSettings: vi.fn(), inspectMedia: vi.fn(), listDownloads: vi.fn(),
  onBrowserMediaRequest: vi.fn(), onProgress: vi.fn(), onTrayNavigate: vi.fn(), pauseDownload: vi.fn(), probeUrl: vi.fn(), removeDownload: vi.fn(),
  removeBrowserIntegration: vi.fn(), repairBrowserIntegration: vi.fn(), resumeDownload: vi.fn(), retryDownload: vi.fn(), saveSettings: vi.fn(), setLaunchAtLogin: vi.fn(), setOnboardingComplete: vi.fn(),
}));

const job = {
  id: '26ee8ee4-bff2-4740-a014-9815157f42f9', url: 'https://example.com/file.bin',
  destination: '/Downloads/file.bin', fileName: 'file.bin', state: 'queued' as const,
  kind: 'file' as const,
  totalBytes: 1024, downloadedBytes: 0, speedBytesPerSecond: 0, etaSeconds: null,
  segments: 4, etag: '"v1"', lastModified: null, sha256: null, error: null,
  createdAt: 1, updatedAt: 1,
};

afterEach(() => { cleanup(); document.documentElement.className = ''; });

describe('App', () => {
  beforeEach(() => {
    vi.mocked(checkBackend).mockResolvedValue({ appName: 'GrabBit', version: '0.1.0', status: 'ok' });
    vi.mocked(listDownloads).mockResolvedValue([job]);
    vi.mocked(getSettings).mockResolvedValue({ maxConcurrentDownloads: 3, defaultSegments: 4, defaultDownloadDirectory: '/Downloads', theme: 'system', browserCaptureMode: 'ask', browserExcludedSites: [], ytDlpPath: '', ffmpegPath: '', ffprobePath: '' });
    vi.mocked(getBrowserDiagnostics).mockResolvedValue({ platform: 'macos', ipcAvailable: true, nativeHostRegistered: true, manifestLocations: ['/manifest.json'], extensionConnected: false, lastConnectionAt: null, lastSuccessfulPingAt: null, lastError: null, automaticInterceptionEnabled: false, downloadsPermissionGranted: true, lastDownloadEventAt: null, lastUrlHostname: null, captureDecision: null, jobAcceptanceStatus: null, chromeCancellationResult: null, lastDownloadJobId: null, lastFailureCategory: null });
    const setup = { appVersion: '0.1.0', packagedExtensionVersion: '0.3.0', nativeHostVersion: '0.1.0', protocolVersion: 1, hostExecutable: '/host', browsers: [{ id: 'chrome', name: 'Google Chrome', detected: true, extensionInstalled: null, extensionEnabled: null, permissionsGranted: null, nativeHostRegistered: true, hostExecutableValid: true, connected: null, lastTestResult: 'Not tested from this browser', setupLocation: '/extensions/chromium' }] };
    vi.mocked(getBrowserSetup).mockResolvedValue(setup);
    vi.mocked(repairBrowserIntegration).mockResolvedValue(setup);
    vi.mocked(getOnboardingComplete).mockResolvedValue(true);
    vi.mocked(getLaunchAtLogin).mockResolvedValue(false);
    vi.mocked(setLaunchAtLogin).mockResolvedValue();
    vi.mocked(setOnboardingComplete).mockResolvedValue();
    vi.mocked(onProgress).mockResolvedValue(vi.fn());
    vi.mocked(onBrowserMediaRequest).mockResolvedValue(vi.fn());
    vi.mocked(onTrayNavigate).mockResolvedValue(vi.fn());
    vi.mocked(probeUrl).mockResolvedValue({ finalUrl: job.url, fileName: job.fileName, contentLength: 1024, supportsRanges: true, etag: '"v1"', lastModified: null, contentType: 'application/octet-stream' });
    vi.mocked(addDownload).mockResolvedValue(job);
    vi.mocked(inspectMedia).mockResolvedValue({ sourceUrl: 'https://media.example/watch/1', website: 'Generic', title: 'Licensed sample', thumbnailUrl: null, durationSeconds: 30, availability: 'public', restrictionReason: null, subtitles: [], formats: [
      { id: 'v720', extension: 'mp4', container: 'mp4', width: 1280, height: 720, frameRate: 30, videoCodec: 'avc1', audioCodec: null, audioBitrateKbps: null, estimatedBytes: 2048, hasVideo: true, hasAudio: false },
      { id: 'a128', extension: 'm4a', container: 'm4a', width: null, height: null, frameRate: null, videoCodec: null, audioCodec: 'mp4a', audioBitrateKbps: 128, estimatedBytes: 512, hasVideo: false, hasAudio: true },
    ] });
    vi.mocked(createMediaDownload).mockResolvedValue({ ...job, id: 'media-job', fileName: 'Licensed sample.mp4', kind: 'media' });
  });

  it('loads the real backend-facing queue contract', async () => {
    render(<App />);
    expect(await screen.findByText('GrabBit v0.1.0')).toBeVisible();
    expect(screen.getByText('file.bin')).toBeVisible();
    expect(checkBackend).toHaveBeenCalledOnce();
    expect(listDownloads).toHaveBeenCalledOnce();
  });

  it('uses one browser capture mode in desktop settings', async () => {
    render(<App />);
    fireEvent.click(screen.getByTitle('Settings'));
    const captureMode = await screen.findByLabelText('Capture mode');
    expect(screen.getByRole('option', { name: 'Off' })).toBeVisible();
    expect(screen.getByRole('option', { name: 'Ask every time' })).toBeVisible();
    fireEvent.change(captureMode, { target: { value: 'automatic' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save settings' }));
    await waitFor(() => expect(saveSettings).toHaveBeenCalledWith(expect.objectContaining({ browserCaptureMode: 'automatic' })));
  });

  it('shows a skippable first-run browser setup without claiming extension installation', async () => {
    vi.mocked(getOnboardingComplete).mockResolvedValue(false);
    render(<App />);
    expect(await screen.findByRole('dialog', { name: 'GrabBit setup' })).toBeVisible();
    expect(screen.getByText(/works without an extension/i)).toBeVisible();
    fireEvent.click(screen.getAllByRole('button', { name: 'Skip setup' }).at(-1)!);
    await waitFor(() => expect(setOnboardingComplete).toHaveBeenCalledWith(true));
  });

  it('preserves stored preferences when finishing first-run setup', async () => {
    const storedSettings = {
      maxConcurrentDownloads: 5, defaultSegments: 8, defaultDownloadDirectory: '/existing-downloads',
      theme: 'dark' as const, browserCaptureMode: 'automatic' as const, browserExcludedSites: ['example.com'],
      ytDlpPath: '/tools/yt-dlp', ffmpegPath: '/tools/ffmpeg', ffprobePath: '/tools/ffprobe',
    };
    vi.mocked(getSettings).mockResolvedValue(storedSettings);
    vi.mocked(getOnboardingComplete).mockResolvedValue(false);
    render(<App />);
    const wizard = await screen.findByRole('dialog', { name: 'GrabBit setup' });
    for (let step = 0; step < 7; step += 1) fireEvent.click(within(wizard).getByRole('button', { name: 'Continue' }));
    fireEvent.click(within(wizard).getByRole('button', { name: 'Finish setup' }));
    await waitFor(() => expect(saveSettings).toHaveBeenCalledWith(storedSettings));
  });

  it('repairs only the browser selected in first-run setup', async () => {
    vi.mocked(getOnboardingComplete).mockResolvedValue(false);
    render(<App />);
    const wizard = await screen.findByRole('dialog', { name: 'GrabBit setup' });
    fireEvent.click(within(wizard).getByRole('button', { name: 'Continue' }));
    fireEvent.click(within(wizard).getByRole('button', { name: 'Continue' }));
    fireEvent.click(within(wizard).getByRole('button', { name: 'Continue' }));
    fireEvent.click(within(wizard).getByRole('button', { name: 'Install / repair native host' }));
    await waitFor(() => expect(repairBrowserIntegration).toHaveBeenCalledWith(['chrome']));
  });

  it('opens the media inspection dialog for a browser media request', async () => {
    let requestMedia: ((url: string) => void) | undefined;
    vi.mocked(onBrowserMediaRequest).mockImplementation(async (callback) => {
      requestMedia = callback;
      return () => undefined;
    });
    render(<App />);
    await waitFor(() => expect(requestMedia).toBeDefined());
    act(() => requestMedia?.('https://media.example/watch/1'));
    const dialog = await screen.findByRole('dialog', { name: 'Add media download' });
    expect(within(dialog).getByPlaceholderText('https://media.example/watch/…')).toHaveValue('https://media.example/watch/1');
  });

  it('inspects metadata and submits a download', async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole('button', { name: 'Add Download' }));
    fireEvent.change(screen.getByPlaceholderText('https://example.com/file.zip'), { target: { value: job.url } });
    expect(await screen.findByText(/Multi-connection supported/)).toBeVisible();
    expect(probeUrl).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole('button', { name: 'Start Download' }));
    await waitFor(() => expect(addDownload).toHaveBeenCalledWith({ url: job.url, destinationDirectory: '/Downloads', segments: 4, sha256: undefined }));
  });

  it('lists real media formats and creates the selected media job', async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole('button', { name: 'Add Media' }));
    fireEvent.change(screen.getByPlaceholderText('https://media.example/watch/…'), { target: { value: 'https://media.example/watch/1' } });
    fireEvent.click(screen.getByText(/I own this media/));
    fireEvent.click(screen.getByRole('button', { name: 'Inspect available formats' }));
    expect(await screen.findByText('Licensed sample')).toBeVisible();
    expect(screen.getByRole('option', { name: /720p 30fps/ })).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Start media download' }));
    await waitFor(() => expect(createMediaDownload).toHaveBeenCalledWith(expect.objectContaining({ formatIds: ['v720', 'a128'], output: 'mp4', authorizationConfirmed: true })));
  });

  it('shows the branded About screen', async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole('button', { name: 'About GrabBit' }));
    expect(screen.getByRole('img', { name: 'GrabBit' })).toHaveAttribute('src', '/branding/logo-icon.png');
    expect(screen.getByText('A resilient cross-platform download manager.')).toBeVisible();
  });

  it('renders real progress, speed, ETA, and state values', async () => {
    vi.mocked(listDownloads).mockResolvedValue([{ ...job, state: 'downloading', downloadedBytes: 512, speedBytesPerSecond: 128, etaSeconds: 60 }]);
    render(<App />);
    expect(await screen.findByText('50%')).toBeVisible();
    expect(screen.getAllByText('128 B/s').length).toBeGreaterThan(0);
    expect(screen.getByText('Downloading')).toBeVisible();
    expect(screen.getByText('1m')).toBeVisible();
  });

  it('updates a row from the Tauri progress event stream', async () => {
    let emit: ((snapshot: ProgressSnapshot) => void) | undefined;
    vi.mocked(onProgress).mockImplementation(async (callback) => { emit = callback; return () => undefined; });
    render(<App />);
    await screen.findByText('file.bin');
    act(() => emit?.({ id: job.id, state: 'downloading', downloadedBytes: 256, totalBytes: 1024, speedBytesPerSecond: 64, etaSeconds: 12 }));
    expect(screen.getByText('25%')).toBeVisible();
    expect(screen.getByText('Downloading')).toBeVisible();
    expect(screen.getByText('12s')).toBeVisible();
  });

  it('searches downloads and filters by file category', async () => {
    const documentJob = { ...job, id: 'doc', fileName: 'report.pdf', destination: '/Downloads/report.pdf' };
    const audioJob = { ...job, id: 'audio', fileName: 'song.mp3', destination: '/Downloads/song.mp3' };
    vi.mocked(listDownloads).mockResolvedValue([documentJob, audioJob]);
    render(<App />);
    await screen.findByText('report.pdf');
    fireEvent.change(screen.getByRole('textbox', { name: 'Search downloads' }), { target: { value: 'song' } });
    expect(screen.getByText('song.mp3')).toBeVisible();
    expect(screen.queryByText('report.pdf')).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole('textbox', { name: 'Search downloads' }), { target: { value: '' } });
    fireEvent.click(screen.getByTitle('Documents'));
    expect(screen.getByText('report.pdf')).toBeVisible();
    expect(screen.queryByText('song.mp3')).not.toBeInTheDocument();
  });

  it('sorts download names in both directions', async () => {
    vi.mocked(listDownloads).mockResolvedValue([{ ...job, id: 'b', fileName: 'beta.bin' }, { ...job, id: 'a', fileName: 'alpha.bin' }]);
    render(<App />);
    await screen.findByText('alpha.bin');
    const names = () => within(screen.getByRole('table')).getAllByRole('row').slice(1).map((row) => within(row).getByRole('button', { name: /\.bin/ }).textContent);
    expect(names()[0]).toContain('alpha.bin');
    fireEvent.click(screen.getByRole('button', { name: 'Name' }));
    expect(names()[0]).toContain('beta.bin');
  });

  it('opens a state-valid right-click context menu', async () => {
    render(<App />);
    const row = (await screen.findByText('file.bin')).closest('tr');
    expect(row).not.toBeNull();
    fireEvent.contextMenu(row!, { clientX: 120, clientY: 80 });
    const menu = screen.getByRole('menu', { name: 'Download actions' });
    expect(within(menu).getByRole('button', { name: 'Cancel' })).toBeVisible();
    expect(within(menu).getByRole('button', { name: 'Copy URL' })).toBeVisible();
    expect(within(menu).queryByRole('button', { name: 'Resume' })).not.toBeInTheDocument();
  });

  it('shows only actions valid for a paused download and invokes resume', async () => {
    vi.mocked(listDownloads).mockResolvedValue([{ ...job, state: 'paused' }]);
    render(<App />);
    fireEvent.click(await screen.findByTitle('Resume'));
    await waitFor(() => expect(resumeDownload).toHaveBeenCalledWith(job.id));
    expect(screen.queryByTitle('Retry')).not.toBeInTheDocument();
  });

  it.each([
    ['downloading', 'Pause', pauseDownload],
    ['queued', 'Cancel', cancelDownload],
    ['failed', 'Retry', retryDownload],
    ['completed', 'Remove from history', removeDownload],
  ] as const)('invokes the valid %s-state action', async (state, label, command) => {
    vi.mocked(listDownloads).mockResolvedValue([{ ...job, state }]);
    render(<App />);
    fireEvent.click(await screen.findByTitle(label));
    await waitFor(() => expect(command).toHaveBeenCalledWith(job.id));
  });

  it('keeps Start Download disabled for an invalid URL', async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole('button', { name: 'Add Download' }));
    fireEvent.change(screen.getByPlaceholderText('https://example.com/file.zip'), { target: { value: 'not-a-url' } });
    expect(screen.getByRole('button', { name: 'Start Download' })).toBeDisabled();
    expect(probeUrl).not.toHaveBeenCalled();
  });

  it('surfaces backend startup errors', async () => {
    vi.mocked(listDownloads).mockRejectedValue(new Error('database unavailable'));
    render(<App />);
    expect(await screen.findByRole('alert')).toHaveTextContent('database unavailable');
  });

  it('applies and persists the selected theme', async () => {
    render(<App />);
    await screen.findByText('file.bin');
    fireEvent.click(screen.getAllByRole('button', { name: 'Settings' })[0]);
    fireEvent.click(screen.getByRole('button', { name: 'Dark' }));
    expect(document.documentElement).toHaveClass('dark');
    fireEvent.click(screen.getByRole('button', { name: 'Save settings' }));
    await waitFor(() => expect(saveSettings).toHaveBeenCalledWith(expect.objectContaining({ theme: 'dark' })));
  });

  it('supports new-download, search, and Escape keyboard shortcuts', async () => {
    render(<App />);
    await screen.findByText('file.bin');
    fireEvent.keyDown(window, { ctrlKey: true, key: 'f' });
    expect(screen.getByRole('textbox', { name: 'Search downloads' })).toHaveFocus();
    fireEvent.keyDown(window, { ctrlKey: true, key: 'n' });
    expect(screen.getByRole('dialog', { name: 'Add download' })).toBeVisible();
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(screen.queryByRole('dialog', { name: 'Add download' })).not.toBeInTheDocument();
  });
});
