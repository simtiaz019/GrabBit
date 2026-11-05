import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import type {
  AppSettings,
  DownloadJob,
  HealthResponse,
  ProgressSnapshot,
  RemoteMetadata,
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

export async function chooseDownloadDirectory(): Promise<string | null> {
  const selected = await open({ directory: true, multiple: false, title: 'Choose download folder' });
  return typeof selected === 'string' ? selected : null;
}

export const onProgress = (
  callback: (progress: ProgressSnapshot) => void,
): Promise<UnlistenFn> => listen<ProgressSnapshot>('download-progress', ({ payload }) => callback(payload));
