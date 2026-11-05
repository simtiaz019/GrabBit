ALTER TABLE download_jobs ADD COLUMN job_kind TEXT NOT NULL DEFAULT 'file';

CREATE TABLE IF NOT EXISTS browser_preferences (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
    capture_enabled INTEGER NOT NULL DEFAULT 1 CHECK (capture_enabled IN (0, 1)),
    automatic_interception INTEGER NOT NULL DEFAULT 0 CHECK (automatic_interception IN (0, 1)),
    excluded_sites_json TEXT NOT NULL DEFAULT '[]',
    yt_dlp_path TEXT NOT NULL DEFAULT '',
    ffmpeg_path TEXT NOT NULL DEFAULT '',
    ffprobe_path TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS browser_captures (
    idempotency_key TEXT PRIMARY KEY NOT NULL,
    request_id TEXT NOT NULL,
    job_id TEXT,
    browser TEXT NOT NULL,
    source_origin TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(job_id) REFERENCES download_jobs(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_browser_captures_job ON browser_captures(job_id);

CREATE TABLE IF NOT EXISTS media_jobs (
    id TEXT PRIMARY KEY NOT NULL,
    source_url TEXT NOT NULL,
    title TEXT NOT NULL,
    website TEXT,
    destination TEXT NOT NULL,
    selected_format_ids_json TEXT NOT NULL,
    output_format TEXT NOT NULL,
    quality TEXT,
    state TEXT NOT NULL,
    stage TEXT NOT NULL,
    downloaded_bytes INTEGER NOT NULL DEFAULT 0,
    total_bytes INTEGER,
    speed_bytes_per_second INTEGER NOT NULL DEFAULT 0,
    eta_seconds INTEGER,
    error TEXT,
    temporary_path TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY(id) REFERENCES download_jobs(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_media_jobs_state_created ON media_jobs(state, created_at);
