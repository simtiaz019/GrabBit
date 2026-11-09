import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import type {
  AppSettings,
  DownloadJob,
  HealthResponse,
  ProgressSnapshot,
  RemoteMetadata,
  MediaInspection,
  MediaOutput,
  BrowserDiagnostics,
  BrowserSetupReport,
  MediaToolsDiagnostics,
} from './types';

export const checkBackend = (): Promise<HealthResponse> => invoke('health_check');
export const probeUrl = (url: string): Promise<RemoteMetadata> => invoke('probe_url', { url });
export const listDownloads = (): Promise<DownloadJob[]> => invoke('list_downloads');

export const addDownload = (request: {
  url: string;
  destinationDirectory: string;
  segments: number;
  sha256?: string;
}): Promise<DownloadJob> => invoke('add_download', request);

export const pauseDownload = (id: string): Promise<void> => invoke('pause_download', { id });
export const resumeDownload = (id: string): Promise<void> => invoke('resume_download', { id });
export const retryDownload = (id: string): Promise<void> => invoke('retry_download', { id });
export const cancelDownload = (id: string): Promise<void> => invoke('cancel_download', { id });
export const removeDownload = (id: string): Promise<void> => invoke('remove_download', { id });
export const getSettings = (): Promise<AppSettings> => invoke('get_settings');
export const saveSettings = (settings: AppSettings): Promise<void> =>
  invoke('save_settings', { settings });

export const inspectMedia = (url: string, authorizationConfirmed: boolean): Promise<MediaInspection> =>
  invoke('inspect_media', { url, authorizationConfirmed });

export const createMediaDownload = (request: {
  url: string;
  destinationDirectory: string;
  formatIds: string[];
  output: MediaOutput;
  authorizationConfirmed: boolean;
}): Promise<DownloadJob> => invoke('create_media_download', request);

export const testMediaTools = (): Promise<MediaToolsDiagnostics> => invoke('media_capabilities');

export const getBrowserDiagnostics = (): Promise<BrowserDiagnostics> =>
  invoke('get_browser_diagnostics');
export const getBrowserSetup = (): Promise<BrowserSetupReport> => invoke('get_browser_setup');
export const repairBrowserIntegration = (browserIds: string[]): Promise<BrowserSetupReport> =>
  invoke('repair_browser_integration', { browserIds });
export const removeBrowserIntegration = (): Promise<void> => invoke('remove_browser_integration');
export const getOnboardingComplete = (): Promise<boolean> => invoke('get_onboarding_complete');
export const setOnboardingComplete = (complete: boolean): Promise<void> => invoke('set_onboarding_complete', { complete });
export const getLaunchAtLogin = (): Promise<boolean> => invoke('get_launch_at_login');
export const setLaunchAtLogin = (enabled: boolean): Promise<void> => invoke('set_launch_at_login', { enabled });

export async function chooseDownloadDirectory(): Promise<string | null> {
  const selected = await open({ directory: true, multiple: false, title: 'Choose download folder' });
  return typeof selected === 'string' ? selected : null;
}

export const onProgress = (
  callback: (progress: ProgressSnapshot) => void,
): Promise<UnlistenFn> => listen<ProgressSnapshot>('download-progress', ({ payload }) => callback(payload));

export const onBrowserMediaRequest = (
  callback: (url: string) => void,
): Promise<UnlistenFn> => listen<string>('browser-media-request', ({ payload }) => callback(payload));

export const onTrayNavigate = (
  callback: (destination: 'active' | 'settings') => void,
): Promise<UnlistenFn> => listen<'active' | 'settings'>('tray-navigate', ({ payload }) => callback(payload));
