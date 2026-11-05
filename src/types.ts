export interface HealthResponse {
  appName: string;
  version: string;
  status: 'ok';
}

export type DownloadState =
  | 'queued'
  | 'inspecting'
  | 'downloading'
  | 'paused'
  | 'completed'
  | 'failed'
  | 'cancelled';

export interface RemoteMetadata {
  finalUrl: string;
  fileName: string;
  contentLength: number | null;
  supportsRanges: boolean;
  etag: string | null;
  lastModified: string | null;
  contentType: string | null;
}

export interface DownloadJob {
  id: string;
  url: string;
  destination: string;
  fileName: string;
  state: DownloadState;
  totalBytes: number | null;
  downloadedBytes: number;
  speedBytesPerSecond: number;
  etaSeconds: number | null;
  segments: number;
  etag: string | null;
  lastModified: string | null;
  sha256: string | null;
  error: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface ProgressSnapshot {
  id: string;
  state: DownloadState;
  downloadedBytes: number;
  totalBytes: number | null;
  speedBytesPerSecond: number;
  etaSeconds: number | null;
}

export type Theme = 'system' | 'light' | 'dark';

export interface AppSettings {
  maxConcurrentDownloads: number;
  defaultSegments: number;
  defaultDownloadDirectory: string;
  theme: Theme;
}
