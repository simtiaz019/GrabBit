export interface HealthResponse {
  appName: string;
  version: string;
  status: 'ok';
}

export type DownloadState =
  | 'queued'
  | 'inspecting'
  | 'downloading'
  | 'merging'
  | 'converting'
  | 'verifying'
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
  kind: 'file' | 'media';
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
  browserCaptureMode: 'off' | 'ask' | 'automatic';
  browserExcludedSites: string[];
  ytDlpPath: string;
  ffmpegPath: string;
  ffprobePath: string;
}

export interface MediaFormat {
  id: string;
  extension: string | null;
  container: string | null;
  width: number | null;
  height: number | null;
  frameRate: number | null;
  videoCodec: string | null;
  audioCodec: string | null;
  audioBitrateKbps: number | null;
  estimatedBytes: number | null;
  hasVideo: boolean;
  hasAudio: boolean;
}

export interface MediaInspection {
  sourceUrl: string;
  website: string | null;
  title: string;
  thumbnailUrl: string | null;
  durationSeconds: number | null;
  availability: 'public' | 'login_required' | 'drm_protected' | 'unavailable' | 'unknown';
  restrictionReason: string | null;
  formats: MediaFormat[];
  subtitles: Array<{ language: string; name: string | null; extensions: string[] }>;
}

export type MediaOutput = 'original' | 'mp4' | 'webm' | 'mkv' | 'mp3' | 'm4a' | 'aac' | 'opus' | 'flac' | 'wav';

export interface BrowserDiagnostics {
  platform: string;
  ipcAvailable: boolean;
  nativeHostRegistered: boolean;
  manifestLocations: string[];
  extensionConnected: boolean;
  lastConnectionAt: number | null;
  lastError: string | null;
  automaticInterceptionEnabled: boolean | null;
  downloadsPermissionGranted: boolean | null;
  lastDownloadEventAt: number | null;
  lastUrlHostname: string | null;
  captureDecision: string | null;
  jobAcceptanceStatus: string | null;
  chromeCancellationResult: string | null;
}
