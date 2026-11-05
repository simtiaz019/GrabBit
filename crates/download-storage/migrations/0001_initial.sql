CREATE TABLE IF NOT EXISTS download_jobs (
    id TEXT PRIMARY KEY NOT NULL,
    url TEXT NOT NULL,
    destination TEXT NOT NULL,
    file_name TEXT NOT NULL,
    state TEXT NOT NULL,
    total_bytes INTEGER,
    downloaded_bytes INTEGER NOT NULL DEFAULT 0 CHECK (downloaded_bytes >= 0),
    speed_bytes_per_second INTEGER NOT NULL DEFAULT 0,
    eta_seconds INTEGER,
    segments INTEGER NOT NULL CHECK (segments BETWEEN 1 AND 32),
    etag TEXT,
    last_modified TEXT,
    sha256 TEXT,
    error TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_download_jobs_state_created
    ON download_jobs(state, created_at);

CREATE TABLE IF NOT EXISTS segment_checkpoints (
    job_id TEXT NOT NULL REFERENCES download_jobs(id) ON DELETE CASCADE,
    segment_index INTEGER NOT NULL,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    downloaded_bytes INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (job_id, segment_index)
);

CREATE TABLE IF NOT EXISTS app_settings (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
    max_concurrent_downloads INTEGER NOT NULL,
    default_segments INTEGER NOT NULL,
    default_download_directory TEXT NOT NULL,
    theme TEXT NOT NULL
);
