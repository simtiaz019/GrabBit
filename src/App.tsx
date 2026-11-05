import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Activity, Archive, AudioLines, Check, ChevronDown, ChevronLeft, ChevronRight,
  CircleAlert, CirclePlay, Clock3, Copy, Download, File, FileArchive, FileAudio,
  FileCode2, FileText, FileVideo, Folder, Gauge, Info, LayoutList, Menu,
  MoreHorizontal, Pause, Plus, RefreshCw, RotateCcw, Search, Settings,
  SlidersHorizontal, Trash2, Video, X, type LucideIcon,
} from 'lucide-react';
import {
  addDownload, cancelDownload, checkBackend, chooseDownloadDirectory, createMediaDownload, getBrowserDiagnostics, getSettings,
  getBrowserSetup, getOnboardingComplete, inspectMedia, removeBrowserIntegration, repairBrowserIntegration,
  listDownloads, onBrowserMediaRequest, onProgress, pauseDownload, probeUrl, removeDownload, resumeDownload,
  retryDownload, saveSettings, setOnboardingComplete,
} from './bridge';
import type { AppSettings, BrowserDiagnostics, BrowserSetupReport, DownloadJob, DownloadState, MediaInspection, MediaOutput, RemoteMetadata } from './types';

type View = 'downloads' | 'active' | 'queued' | 'completed' | 'failed' | 'activity' | 'documents' | 'videos' | 'audio' | 'software' | 'archives' | 'others' | 'settings' | 'about';
type Action = 'pause' | 'resume' | 'retry' | 'cancel' | 'remove' | 'copy';
type SortKey = 'name' | 'size' | 'progress' | 'speed' | 'eta' | 'status';
const defaults: AppSettings = { maxConcurrentDownloads: 3, defaultSegments: 4, defaultDownloadDirectory: '', theme: 'system', browserCaptureMode: 'ask', browserExcludedSites: [], ytDlpPath: '', ffmpegPath: '', ffprobePath: '' };
const mainNav: Array<[View, string, LucideIcon]> = [['downloads', 'Downloads', Download], ['active', 'Active', Activity], ['queued', 'Queued', Clock3], ['completed', 'Completed', Check], ['failed', 'Failed', CircleAlert], ['activity', 'Activity', Gauge]];
const categories: Array<[View, string, LucideIcon]> = [['documents', 'Documents', FileText], ['videos', 'Videos', Video], ['audio', 'Audio', AudioLines], ['software', 'Software', FileCode2], ['archives', 'Archives', Archive], ['others', 'Others', File]];

export default function App() {
  const [jobs, setJobs] = useState<DownloadJob[]>([]);
  const [settings, setSettings] = useState(defaults);
  const [view, setView] = useState<View>('downloads');
  const [query, setQuery] = useState('');
  const [showAdd, setShowAdd] = useState(false);
  const [showMedia, setShowMedia] = useState(false);
  const [mediaUrl, setMediaUrl] = useState('');
  const [detailsId, setDetailsId] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [collapsed, setCollapsed] = useState(false);
  const [sort, setSort] = useState<{ key: SortKey; asc: boolean }>({ key: 'name', asc: true });
  const [backend, setBackend] = useState('Connecting…');
  const [error, setError] = useState<string | null>(null);
  const [context, setContext] = useState<{ id: string; x: number; y: number } | null>(null);
  const [loading, setLoading] = useState(true);
  const [settingsLoaded, setSettingsLoaded] = useState(false);
  const [showOnboarding, setShowOnboarding] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);
  const refresh = useCallback(async () => setJobs(await listDownloads()), []);

  useEffect(() => {
    void Promise.all([checkBackend(), listDownloads(), getSettings()]).then(([health, downloads, stored]) => {
      setBackend(`${health.appName} v${health.version}`); setJobs(downloads); setSettings(stored); setSettingsLoaded(true);
    }).catch((reason: unknown) => setError(message(reason))).finally(() => setLoading(false));
    let dispose: (() => void) | undefined;
    void onProgress((progress) => {
      setJobs((current) => current.map((job) => job.id === progress.id ? { ...job, state: progress.state, downloadedBytes: progress.downloadedBytes, totalBytes: progress.totalBytes, speedBytesPerSecond: progress.speedBytesPerSecond, etaSeconds: progress.etaSeconds } : job));
      if (progress.state === 'completed') void refresh();
    }).then((unlisten) => { dispose = unlisten; });
    return () => dispose?.();
  }, [refresh]);

  useEffect(() => {
    void getOnboardingComplete().then((complete) => setShowOnboarding(!complete)).catch(() => setShowOnboarding(true));
  }, []);

  useEffect(() => {
    let dispose: (() => void) | undefined;
    void onBrowserMediaRequest((url) => { setMediaUrl(url); setShowMedia(true); }).then((unlisten) => { dispose = unlisten; });
    return () => dispose?.();
  }, []);

  useEffect(() => {
    const media = matchMedia('(prefers-color-scheme: dark)');
    const apply = () => document.documentElement.classList.toggle('dark', settings.theme === 'dark' || (settings.theme === 'system' && media.matches));
    apply(); media.addEventListener('change', apply); return () => media.removeEventListener('change', apply);
  }, [settings.theme]);

  useEffect(() => {
    const keys = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'n') { event.preventDefault(); setShowAdd(true); }
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'f') { event.preventDefault(); searchRef.current?.focus(); }
      if (event.key === 'Escape') { setShowAdd(false); setShowMedia(false); setDetailsId(null); }
    };
    addEventListener('keydown', keys); return () => removeEventListener('keydown', keys);
  }, []);

  const visible = useMemo(() => jobs.filter((job) => matchesView(job, view) && matchesSearch(job, query)).sort((a, b) => compare(a, b, sort.key) * (sort.asc ? 1 : -1)), [jobs, query, sort, view]);
  const active = jobs.filter((job) => ['downloading', 'merging', 'converting', 'verifying'].includes(job.state));
  const speed = active.reduce((total, job) => total + job.speedBytesPerSecond, 0);
  const details = jobs.find((job) => job.id === detailsId) ?? null;

  async function act(operation: () => Promise<void>) { setError(null); try { await operation(); await refresh(); } catch (reason) { setError(message(reason)); } }
  function action(job: DownloadJob, name: Action) {
    if (name === 'copy') { void navigator.clipboard.writeText(job.url).catch((reason: unknown) => setError(message(reason))); return; }
    const operations = { pause: () => pauseDownload(job.id), resume: () => resumeDownload(job.id), retry: () => retryDownload(job.id), cancel: () => cancelDownload(job.id), remove: () => removeDownload(job.id) };
    void act(operations[name]);
  }
  function setSortKey(key: SortKey) { setSort((current) => ({ key, asc: current.key === key ? !current.asc : true })); }

  return <div className={`app-shell ${collapsed ? 'sidebar-collapsed' : ''}`} onClick={() => setContext(null)}>
    <Sidebar collapsed={collapsed} jobs={jobs} onCollapse={() => setCollapsed((value) => !value)} onNavigate={(next) => { setView(next); setDetailsId(null); }} view={view} />
    <div className="app-workspace">
      <header className="top-toolbar"><div className="page-heading"><button className="icon-button mobile-menu" onClick={() => setCollapsed((value) => !value)} title="Toggle sidebar" type="button"><Menu size={18} /></button><div><h1>{title(view)}</h1><p>{subtitle(view)}</p></div></div><div className="toolbar-actions"><label className="global-search"><Search size={16} /><input aria-label="Search downloads" onChange={(event) => setQuery(event.target.value)} placeholder="Search downloads" ref={searchRef} value={query} /><kbd>⌘F</kbd></label><button className="secondary compact" onClick={() => { setMediaUrl(''); setShowMedia(true); }} type="button"><Video size={17} /> Add Media</button><button className="primary compact" onClick={() => setShowAdd(true)} type="button"><Plus size={17} /> Add Download</button><button className="icon-button" onClick={() => setView('settings')} title="Settings" type="button"><Settings size={18} /></button></div></header>
      {error && <div className="error-banner" role="alert"><CircleAlert size={17} /><span>{error}</span><button onClick={() => setError(null)} type="button"><X size={16} /></button></div>}
      <div className="content-frame"><main className="main-content">
        {isListView(view) && <><Summary active={active.length} queued={jobs.filter((job) => job.state === 'queued').length} speed={speed} total={jobs.length} /><div className="download-toolbar"><div><SlidersHorizontal size={15} /><span>{visible.length} downloads</span>{selected.size > 0 && <b>{selected.size} selected</b>}</div><button className="secondary compact" onClick={() => void refresh()} type="button"><RefreshCw size={15} /> Refresh</button></div>{loading ? <LoadingState /> : <DownloadTable jobs={visible} onAction={action} onContext={(event, job) => { event.preventDefault(); event.stopPropagation(); setContext({ id: job.id, x: event.clientX, y: event.clientY }); }} onDetails={(job) => setDetailsId(job.id)} onSelect={(id, checked) => setSelected((current) => { const next = new Set(current); if (checked) next.add(id); else next.delete(id); return next; })} onSelectAll={(checked) => setSelected(checked ? new Set(visible.map((job) => job.id)) : new Set())} onSort={setSortKey} selected={selected} sort={sort} />}</>}
        {view === 'activity' && <ActivityView jobs={jobs} />}
        {view === 'settings' && <SettingsView onOpenSetup={() => setShowOnboarding(true)} settings={settings} onChange={setSettings} onSave={() => act(() => saveSettings(settings))} />}
        {view === 'about' && <About backend={backend} />}
      </main>{details && <Details job={details} onAction={action} onClose={() => setDetailsId(null)} />}</div>
      <footer className="status-bar"><div><i /> Engine ready <small>{backend}</small></div><div><span>{active.length} active</span><span className="numeric">{formatBytes(speed)}/s</span></div></footer>
    </div>
    {showAdd && <NewDownload settings={settings} onClose={() => setShowAdd(false)} onCreated={(job) => { setJobs((current) => [job, ...current]); setShowAdd(false); setView('downloads'); }} onError={setError} />}
    {showMedia && <MediaDownloadDialog initialUrl={mediaUrl} settings={settings} onClose={() => setShowMedia(false)} onCreated={(job) => { setJobs((current) => [job, ...current]); setShowMedia(false); setView('downloads'); }} onError={setError} />}
    {showOnboarding && settingsLoaded && <OnboardingWizard settings={settings} onChangeSettings={setSettings} onClose={() => setShowOnboarding(false)} onError={setError} />}
    {context && <ContextMenu job={jobs.find((job) => job.id === context.id) ?? null} onAction={(job, name) => { setContext(null); action(job, name); }} x={context.x} y={context.y} />}
  </div>;
}

function Sidebar({ collapsed, jobs, onCollapse, onNavigate, view }: { collapsed: boolean; jobs: DownloadJob[]; onCollapse: () => void; onNavigate: (view: View) => void; view: View }) {
  const links = (items: typeof mainNav) => items.map(([target, label, Icon]) => <button aria-current={view === target ? 'page' : undefined} aria-label={label} className={`nav-button ${view === target ? 'active' : ''}`} key={target} onClick={() => onNavigate(target)} title={label} type="button"><Icon size={17} /><span>{label}</span>{target !== 'activity' && <b>{jobs.filter((job) => matchesView(job, target)).length}</b>}</button>);
  return <aside className="sidebar"><div className="sidebar-brand"><img alt="" src="/branding/logo-icon.png" /><span>GrabBit</span><button className="icon-button" onClick={onCollapse} title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'} type="button">{collapsed ? <ChevronRight size={16} /> : <ChevronLeft size={16} />}</button></div><nav>{links(mainNav)}<p className="nav-label">Categories</p>{links(categories)}</nav><div className="sidebar-bottom"><button className={`nav-button ${view === 'settings' ? 'active' : ''}`} onClick={() => onNavigate('settings')} type="button"><Settings size={17} /><span>Settings</span></button><button className={`nav-button ${view === 'about' ? 'active' : ''}`} onClick={() => onNavigate('about')} type="button"><Info size={17} /><span>About GrabBit</span></button></div></aside>;
}

function Summary({ active, queued, speed, total }: { active: number; queued: number; speed: number; total: number }) { const items: Array<[LucideIcon, string, string]> = [[Gauge, 'Current speed', `${formatBytes(speed)}/s`], [CirclePlay, 'Active', String(active)], [Clock3, 'Queued', String(queued)], [LayoutList, 'Total tasks', String(total)]]; return <section className="summary-strip">{items.map(([Icon, label, value]) => <div key={label}><Icon size={17} /><span>{label}<strong>{value}</strong></span></div>)}</section>; }

function LoadingState() { return <div aria-label="Loading downloads" className="loading-table" role="status">{[0, 1, 2].map((row) => <div key={row}><span /><span /><span /><span /></div>)}</div>; }

function DownloadTable({ jobs, onAction, onContext, onDetails, onSelect, onSelectAll, onSort, selected, sort }: { jobs: DownloadJob[]; onAction: (job: DownloadJob, action: Action) => void; onContext: (event: React.MouseEvent, job: DownloadJob) => void; onDetails: (job: DownloadJob) => void; onSelect: (id: string, checked: boolean) => void; onSelectAll: (checked: boolean) => void; onSort: (key: SortKey) => void; selected: Set<string>; sort: { key: SortKey; asc: boolean } }) {
  if (!jobs.length) return <div className="empty-state"><div><Download size={28} /></div><h2>No downloads found</h2><p>Add a download or adjust the current search and filter.</p><span>Press <kbd>⌘N</kbd> to create a new download</span></div>;
  const head = (key: SortKey, label: string) => <th><button className={sort.key === key ? 'sorted' : ''} onClick={() => onSort(key)} type="button">{label}<ChevronDown className={sort.key === key && !sort.asc ? 'descending' : ''} size={13} /></button></th>;
  return <div className="table-shell" tabIndex={0}><table className="download-table"><thead><tr><th className="check-cell"><input aria-label="Select all downloads" checked={jobs.every((job) => selected.has(job.id))} onChange={(event) => onSelectAll(event.target.checked)} type="checkbox" /></th>{head('name', 'Name')}{head('size', 'Size')}{head('progress', 'Progress')}{head('speed', 'Speed')}{head('eta', 'ETA')}{head('status', 'Status')}<th /></tr></thead><tbody>{jobs.map((job) => <DownloadRow job={job} key={job.id} onAction={onAction} onContext={onContext} onDetails={onDetails} onSelect={onSelect} selected={selected.has(job.id)} />)}</tbody></table></div>;
}

function DownloadRow({ job, onAction, onContext, onDetails, onSelect, selected }: { job: DownloadJob; onAction: (job: DownloadJob, action: Action) => void; onContext: (event: React.MouseEvent, job: DownloadJob) => void; onDetails: (job: DownloadJob) => void; onSelect: (id: string, checked: boolean) => void; selected: boolean }) {
  const percent = progress(job); const Icon = fileIcon(job.fileName);
  return <tr className={selected ? 'selected' : ''} onContextMenu={(event) => onContext(event, job)} onDoubleClick={() => onDetails(job)}><td className="check-cell"><input aria-label={`Select ${job.fileName}`} checked={selected} onChange={(event) => onSelect(job.id, event.target.checked)} type="checkbox" /></td><td><button className="file-cell" onClick={() => onDetails(job)} type="button"><span className="file-icon"><Icon size={19} /></span><span><strong>{job.fileName}</strong><small>{job.destination}</small></span></button></td><td className="numeric">{job.totalBytes ? formatBytes(job.totalBytes) : '—'}</td><td><div className="progress-cell"><div><span style={{ width: `${percent}%` }} /></div><small>{percent.toFixed(0)}%</small></div></td><td className="numeric">{job.state === 'downloading' ? `${formatBytes(job.speedBytesPerSecond)}/s` : '—'}</td><td className="numeric">{job.state === 'downloading' && job.etaSeconds != null ? formatDuration(job.etaSeconds) : '—'}</td><td><Status state={job.state} /></td><td><Actions job={job} onAction={onAction} /></td></tr>;
}

function Actions({ job, onAction }: { job: DownloadJob; onAction: (job: DownloadJob, action: Action) => void }) { const button = (Icon: LucideIcon, label: string, action: Action) => <button className="icon-button small" key={label} onClick={() => onAction(job, action)} title={label} type="button"><Icon size={15} /></button>; return <div className="row-actions">{job.state === 'downloading' && button(Pause, job.kind === 'media' ? 'Pause (restarts media transfer)' : 'Pause', 'pause')}{job.state === 'paused' && button(CirclePlay, job.kind === 'media' ? 'Restart media transfer' : 'Resume', 'resume')}{job.state === 'failed' && button(RotateCcw, 'Retry', 'retry')}{!['completed', 'cancelled'].includes(job.state) && button(X, 'Cancel', 'cancel')}{['completed', 'cancelled', 'failed'].includes(job.state) && button(Trash2, 'Remove from history', 'remove')}{button(MoreHorizontal, 'Copy URL', 'copy')}</div>; }
function Status({ state }: { state: DownloadState }) { return <span className={`status-badge status-${state}`}><i />{capitalize(state)}</span>; }

function ContextMenu({ job, onAction, x, y }: { job: DownloadJob | null; onAction: (job: DownloadJob, action: Action) => void; x: number; y: number }) {
  if (!job) return null;
  const item = (Icon: LucideIcon, label: string, action: Action, danger = false) => <button className={danger ? 'danger' : ''} key={label} onClick={() => onAction(job, action)} type="button"><Icon size={15} />{label}</button>;
  return <div aria-label="Download actions" className="context-menu" onClick={(event) => event.stopPropagation()} role="menu" style={{ left: Math.min(x, window.innerWidth - 190), top: Math.min(y, window.innerHeight - 220) }}>{job.state === 'downloading' && item(Pause, 'Pause', 'pause')}{job.state === 'paused' && item(CirclePlay, 'Resume', 'resume')}{job.state === 'failed' && item(RotateCcw, 'Retry', 'retry')}{!['completed', 'cancelled'].includes(job.state) && item(X, 'Cancel', 'cancel')}{item(Copy, 'Copy URL', 'copy')}{['completed', 'cancelled', 'failed'].includes(job.state) && item(Trash2, 'Remove from history', 'remove', true)}</div>;
}

function Details({ job, onAction, onClose }: { job: DownloadJob; onAction: (job: DownloadJob, action: Action) => void; onClose: () => void }) { const percent = progress(job); return <aside className="details-panel"><div className="panel-header"><div><span>Download details</span><strong>{job.fileName}</strong></div><button className="icon-button" onClick={onClose} type="button"><X size={17} /></button></div><section><h3>Overview</h3><div className="detail-progress"><div><span style={{ width: `${percent}%` }} /></div><strong>{percent.toFixed(1)}%</strong><small>{formatBytes(job.downloadedBytes)} of {job.totalBytes ? formatBytes(job.totalBytes) : 'unknown'}</small></div><Status state={job.state} /></section><section><h3>Transfer</h3><dl><dt>Speed</dt><dd>{job.state === 'downloading' ? `${formatBytes(job.speedBytesPerSecond)}/s` : '—'}</dd><dt>ETA</dt><dd>{job.etaSeconds == null ? '—' : formatDuration(job.etaSeconds)}</dd><dt>Connections</dt><dd>{job.segments}</dd></dl></section><section><h3>Destination</h3><p className="source-url">{job.destination}</p></section><section><h3>Source URL</h3><p className="source-url">{job.url}</p><button className="secondary compact" onClick={() => onAction(job, 'copy')} type="button"><Copy size={14} /> Copy URL</button></section>{job.error && <section className="error-detail"><h3>Error details</h3><p>{job.error}</p></section>}<div className="panel-actions"><Actions job={job} onAction={onAction} /></div></aside>; }

function NewDownload({ settings, onClose, onCreated, onError }: { settings: AppSettings; onClose: () => void; onCreated: (job: DownloadJob) => void; onError: (error: string) => void }) {
  const [url, setUrl] = useState(''); const [directory, setDirectory] = useState(settings.defaultDownloadDirectory); const [segments, setSegments] = useState(settings.defaultSegments); const [sha256, setSha256] = useState(''); const [metadata, setMetadata] = useState<RemoteMetadata | null>(null); const [busy, setBusy] = useState(false); const [inspecting, setInspecting] = useState(false); const [probeError, setProbeError] = useState<string | null>(null); const sequence = useRef(0);
  useEffect(() => { const candidate = url.trim(); const current = ++sequence.current; setMetadata(null); setProbeError(null); if (!validUrl(candidate)) { setInspecting(false); return; } const timer = setTimeout(() => { setInspecting(true); void probeUrl(candidate).then((value) => { if (sequence.current === current) setMetadata(value); }).catch((reason: unknown) => { if (sequence.current === current) setProbeError(message(reason)); }).finally(() => { if (sequence.current === current) setInspecting(false); }); }, 450); return () => clearTimeout(timer); }, [url]);
  async function inspect() { const current = ++sequence.current; setInspecting(true); setProbeError(null); try { const value = await probeUrl(url.trim()); if (sequence.current === current) setMetadata(value); } catch (reason) { if (sequence.current === current) setProbeError(message(reason)); } finally { if (sequence.current === current) setInspecting(false); } }
  async function submit() { setBusy(true); try { onCreated(await addDownload({ url: url.trim(), destinationDirectory: directory, segments, sha256: sha256 || undefined })); } catch (reason) { onError(message(reason)); setBusy(false); } }
  return <div className="modal-backdrop"><section aria-label="Add download" aria-modal="true" className="new-download-dialog" role="dialog"><div className="dialog-header"><div><h2>New download</h2><p>Paste an HTTP or HTTPS link to inspect it automatically.</p></div><button className="icon-button" onClick={onClose} type="button"><X size={18} /></button></div><div className="dialog-body"><label>Download URL<div className={`url-field ${probeError ? 'invalid' : ''}`}><Download size={17} /><input autoFocus onChange={(event) => setUrl(event.target.value)} placeholder="https://example.com/file.zip" value={url} />{inspecting && <RefreshCw className="spin" size={16} />}</div></label>{probeError && <div className="inline-error"><CircleAlert size={15} /><span>{probeError}</span><button onClick={() => void inspect()} type="button">Retry</button></div>}{metadata && <div className="detected-file"><span className="file-icon"><File size={19} /></span><div><strong>{metadata.fileName}</strong><p>{metadata.contentLength ? formatBytes(metadata.contentLength) : 'Unknown size'} · {metadata.supportsRanges ? 'Multi-connection supported' : 'Single connection'}</p></div><Check size={18} /></div>}<label>Save location<div className="field-with-action"><Folder size={17} /><input onChange={(event) => setDirectory(event.target.value)} value={directory} /><button className="secondary compact" onClick={() => void chooseDownloadDirectory().then((value) => value && setDirectory(value))} type="button">Browse</button></div></label><div className="dialog-grid"><label>Connections<select onChange={(event) => setSegments(Number(event.target.value))} value={segments}>{[1, 2, 4, 8, 16, 32].map((value) => <option key={value}>{value}</option>)}</select></label><label>Expected SHA-256 <span>(optional)</span><input onChange={(event) => setSha256(event.target.value)} placeholder="64 hexadecimal characters" value={sha256} /></label></div></div><div className="dialog-footer"><span>{metadata ? 'Ready to download' : inspecting ? 'Inspecting link…' : 'Waiting for a valid link'}</span><div><button className="secondary" onClick={onClose} type="button">Cancel</button><button className="primary" disabled={!validUrl(url) || !directory || busy || inspecting || Boolean(probeError)} onClick={() => void submit()} type="button">{busy ? 'Starting…' : 'Start Download'}</button></div></div></section></div>;
}

function MediaDownloadDialog({ initialUrl, settings, onClose, onCreated, onError }: { initialUrl: string; settings: AppSettings; onClose: () => void; onCreated: (job: DownloadJob) => void; onError: (error: string) => void }) {
  const [url, setUrl] = useState(initialUrl); const [authorized, setAuthorized] = useState(false); const [inspection, setInspection] = useState<MediaInspection | null>(null); const [busy, setBusy] = useState(false); const [audioOnly, setAudioOnly] = useState(false); const [videoId, setVideoId] = useState(''); const [audioId, setAudioId] = useState(''); const [output, setOutput] = useState<MediaOutput>('mp4'); const [directory, setDirectory] = useState(settings.defaultDownloadDirectory);
  const videos = inspection?.formats.filter((format) => format.hasVideo).sort((a, b) => (b.height ?? 0) - (a.height ?? 0) || (b.frameRate ?? 0) - (a.frameRate ?? 0)) ?? [];
  const audios = inspection?.formats.filter((format) => format.hasAudio && !format.hasVideo).sort((a, b) => (b.audioBitrateKbps ?? 0) - (a.audioBitrateKbps ?? 0)) ?? [];
  async function inspect() { setBusy(true); try { const value = await inspectMedia(url.trim(), authorized); setInspection(value); const video = value.formats.filter((format) => format.hasVideo).sort((a, b) => (b.height ?? 0) - (a.height ?? 0))[0]; const audio = value.formats.filter((format) => format.hasAudio && !format.hasVideo).sort((a, b) => (b.audioBitrateKbps ?? 0) - (a.audioBitrateKbps ?? 0))[0]; setVideoId(video?.id ?? ''); setAudioId(audio?.id ?? ''); } catch (reason) { onError(message(reason)); } finally { setBusy(false); } }
  async function submit() { const selectedVideo = videos.find((format) => format.id === videoId); const formatIds = audioOnly ? [audioId] : [videoId, ...(selectedVideo?.hasAudio || !audioId ? [] : [audioId])]; setBusy(true); try { onCreated(await createMediaDownload({ url: url.trim(), destinationDirectory: directory, formatIds, output, authorizationConfirmed: authorized })); } catch (reason) { onError(message(reason)); setBusy(false); } }
  const selected = audioOnly ? audios.find((format) => format.id === audioId) : videos.find((format) => format.id === videoId);
  const outputs: MediaOutput[] = audioOnly ? ['original', 'm4a', 'mp3', 'aac', 'opus', 'flac', 'wav'] : ['original', 'mp4', 'webm', 'mkv'];
  return <div className="modal-backdrop"><section aria-label="Add media download" aria-modal="true" className="new-download-dialog media-dialog" role="dialog"><div className="dialog-header"><div><h2>Video or audio download</h2><p>Inspect real source formats using your configured yt-dlp and FFmpeg tools.</p></div><button className="icon-button" onClick={onClose} type="button"><X size={18} /></button></div><div className="dialog-body"><label>Media page or direct media URL<div className="url-field"><Video size={17} /><input autoFocus onChange={(event) => { setUrl(event.target.value); setInspection(null); }} placeholder="https://media.example/watch/…" value={url} /></div></label><label className="authorization-check"><input checked={authorized} onChange={(event) => setAuthorized(event.target.checked)} type="checkbox" />I own this media or have permission to download it, and it is not DRM-protected.</label><button className="secondary" disabled={!validUrl(url) || !authorized || busy} onClick={() => void inspect()} type="button">{busy && !inspection ? 'Inspecting…' : 'Inspect available formats'}</button>{inspection && <><div className="media-summary">{inspection.thumbnailUrl ? <img alt="" className="media-thumbnail" src={inspection.thumbnailUrl} /> : <span className="file-icon"><FileVideo size={20} /></span>}<div><strong>{inspection.title}</strong><p>{inspection.website ?? 'Direct media'}{inspection.durationSeconds ? ` · ${formatDuration(Math.round(inspection.durationSeconds))}` : ''} · {capitalize(inspection.availability.replace('_', ' '))}</p></div></div><label className="authorization-check"><input checked={audioOnly} onChange={(event) => { const next = event.target.checked; setAudioOnly(next); setOutput(next ? 'original' : 'mp4'); }} type="checkbox" />Audio only</label>{!audioOnly && <label>Video format<select onChange={(event) => setVideoId(event.target.value)} value={videoId}>{videos.map((format) => <option key={format.id} value={format.id}>{mediaFormatLabel(format)}</option>)}</select></label>}<label>{audioOnly ? 'Audio format' : 'Audio track (used when video has no audio)'}<select disabled={!audioOnly && Boolean(videos.find((format) => format.id === videoId)?.hasAudio)} onChange={(event) => setAudioId(event.target.value)} value={audioId}>{audios.map((format) => <option key={format.id} value={format.id}>{mediaFormatLabel(format)}</option>)}</select></label><div className="dialog-grid"><label>Output<select onChange={(event) => setOutput(event.target.value as MediaOutput)} value={output}>{outputs.map((value) => <option key={value} value={value}>{value === 'original' ? 'Original (no conversion)' : `${value.toUpperCase()}${audioOnly && ['mp3', 'aac', 'flac', 'wav'].includes(value) ? ' (convert)' : ' (remux/convert if needed)'}`}</option>)}</select></label><label>Save location<div className="field-with-action"><Folder size={17} /><input onChange={(event) => setDirectory(event.target.value)} value={directory} /><button className="secondary compact" onClick={() => void chooseDownloadDirectory().then((value) => value && setDirectory(value))} type="button">Browse</button></div></label></div>{output === 'flac' && <p className="muted-message">FLAC conversion does not restore quality lost in the source audio.</p>}<p className="muted-message">Selected source: {selected ? mediaFormatLabel(selected) : 'Choose a format'}. Transcoding is explicit and can use significant CPU and disk space.</p></>}</div><div className="dialog-footer"><span>Restricted media is not supported; use the website's official export when offered.</span><div><button className="secondary" onClick={onClose} type="button">Cancel</button><button className="primary" disabled={!inspection || !directory || busy || (audioOnly ? !audioId : !videoId)} onClick={() => void submit()} type="button">{busy ? 'Starting…' : 'Start media download'}</button></div></div></section></div>;
}

function mediaFormatLabel(format: MediaInspection['formats'][number]) { const resolution = format.height ? `${format.height}p${format.frameRate ? ` ${format.frameRate}fps` : ''}` : 'Audio'; const codecs = [format.videoCodec, format.audioCodec].filter(Boolean).join(' + '); return `${resolution} · ${format.extension ?? format.container ?? 'source'}${codecs ? ` · ${codecs}` : ''}${format.estimatedBytes ? ` · ${formatBytes(format.estimatedBytes)}` : ''}`; }

function ActivityView({ jobs }: { jobs: DownloadJob[] }) { const active = jobs.filter((job) => job.state === 'downloading'); const completed = jobs.filter((job) => job.state === 'completed').sort((a, b) => b.updatedAt - a.updatedAt).slice(0, 8); const speed = active.reduce((sum, job) => sum + job.speedBytesPerSecond, 0); return <div className="page-stack"><Summary active={active.length} queued={jobs.filter((job) => job.state === 'queued').length} speed={speed} total={jobs.length} /><section className="surface-section"><div className="section-heading"><div><h2>Transfer activity</h2><p>Live information reported by the download engine.</p></div><Status state={active.length ? 'downloading' : 'paused'} /></div><div className="activity-meter"><Gauge size={22} /><div><span>Aggregate download speed</span><strong>{formatBytes(speed)}/s</strong></div></div></section><section className="surface-section"><div className="section-heading"><div><h2>Recent completions</h2><p>Latest successfully finished downloads.</p></div></div>{completed.length ? <div className="recent-list">{completed.map((job) => <div key={job.id}><span className="file-icon"><Check size={16} /></span><span><strong>{job.fileName}</strong><small>{job.totalBytes ? formatBytes(job.totalBytes) : 'Unknown size'}</small></span><time>{relativeTime(job.updatedAt)}</time></div>)}</div> : <p className="muted-message">No completed downloads yet.</p>}</section></div>; }

function SettingsView({ settings, onChange, onSave, onOpenSetup }: { settings: AppSettings; onChange: (settings: AppSettings) => void; onSave: () => void; onOpenSetup: () => void }) { return <div className="settings-layout"><aside><a href="#general">General</a><a href="#downloads">Downloads</a><a href="#browser">Browser integration</a><a href="#media-tools">Media tools</a><a href="#appearance">Appearance</a></aside><div className="settings-content"><SettingsSection description="Default behavior for new downloads." id="general" title="General"><label>Default download directory<div className="field-with-action"><Folder size={17} /><input onChange={(event) => onChange({ ...settings, defaultDownloadDirectory: event.target.value })} value={settings.defaultDownloadDirectory} /><button className="secondary compact" onClick={() => void chooseDownloadDirectory().then((value) => value && onChange({ ...settings, defaultDownloadDirectory: value }))} type="button">Browse</button></div></label></SettingsSection><SettingsSection description="Connection defaults. Concurrency changes take effect after restart." id="downloads" title="Downloads"><div className="settings-grid"><label>Maximum concurrent downloads<input max="32" min="1" onChange={(event) => onChange({ ...settings, maxConcurrentDownloads: Number(event.target.value) })} type="number" value={settings.maxConcurrentDownloads} /></label><label>Connections per download<input max="32" min="1" onChange={(event) => onChange({ ...settings, defaultSegments: Number(event.target.value) })} type="number" value={settings.defaultSegments} /></label></div></SettingsSection><SettingsSection description="Control browser capture centrally. The extension always reads and saves this policy through GrabBit." id="browser" title="Browser integration"><label>Capture mode<select onChange={(event) => { const value = event.target.value; if (value === 'off' || value === 'ask' || value === 'automatic') onChange({ ...settings, browserCaptureMode: value }); }} value={settings.browserCaptureMode}><option value="off">Off</option><option value="ask">Ask every time</option><option value="automatic">Automatic (IDM-style)</option></select></label><label>Excluded sites <span>(one hostname per line)</span><textarea onChange={(event) => onChange({ ...settings, browserExcludedSites: event.target.value.split('\n').map((value) => value.trim()).filter(Boolean) })} rows={4} value={settings.browserExcludedSites.join('\n')} /></label><button className="secondary compact" onClick={onOpenSetup} type="button">Open browser setup wizard</button><BrowserDiagnosticsPanel /></SettingsSection><SettingsSection description="Use absolute paths to trusted, separately installed executables. GrabBit never downloads or updates these tools silently." id="media-tools" title="Media tools"><div className="tool-paths"><label>yt-dlp<input onChange={(event) => onChange({ ...settings, ytDlpPath: event.target.value })} placeholder="/opt/homebrew/bin/yt-dlp" value={settings.ytDlpPath} /></label><label>FFmpeg<input onChange={(event) => onChange({ ...settings, ffmpegPath: event.target.value })} placeholder="/opt/homebrew/bin/ffmpeg" value={settings.ffmpegPath} /></label><label>FFprobe<input onChange={(event) => onChange({ ...settings, ffprobePath: event.target.value })} placeholder="/opt/homebrew/bin/ffprobe" value={settings.ffprobePath} /></label></div></SettingsSection><SettingsSection description="Choose how GrabBit follows your desktop appearance." id="appearance" title="Appearance"><div className="theme-options">{(['system', 'light', 'dark'] as const).map((theme) => <button className={settings.theme === theme ? 'selected' : ''} key={theme} onClick={() => onChange({ ...settings, theme })} type="button"><span className={`theme-preview ${theme}`} /><strong>{theme === 'system' ? 'Follow system' : capitalize(theme)}</strong>{settings.theme === theme && <Check size={16} />}</button>)}</div></SettingsSection><div className="settings-save"><button className="primary" onClick={onSave} type="button">Save settings</button></div></div></div>; }

function BrowserDiagnosticsPanel() {
  const [diagnostics, setDiagnostics] = useState<BrowserDiagnostics | null>(null);
  const [setup, setSetup] = useState<BrowserSetupReport | null>(null);
  const [diagnosticError, setDiagnosticError] = useState<string | null>(null);
  const refresh = useCallback(() => {
    setDiagnosticError(null);
    void Promise.all([getBrowserDiagnostics(), getBrowserSetup()])
      .then(([nextDiagnostics, nextSetup]) => { setDiagnostics(nextDiagnostics); setSetup(nextSetup); })
      .catch((reason: unknown) => setDiagnosticError(message(reason)));
  }, []);
  useEffect(refresh, [refresh]);

  async function repair() {
    const browserIds = setup?.browsers.filter((browser) => browser.detected && browser.id !== 'safari').map((browser) => browser.id) ?? [];
    if (!browserIds.length) { setDiagnosticError('No detected browser is available to configure.'); return; }
    setDiagnosticError(null);
    try { setSetup(await repairBrowserIntegration(browserIds)); refresh(); }
    catch (reason) { setDiagnosticError(message(reason)); }
  }

  async function remove() {
    if (!globalThis.confirm('Remove GrabBit browser registrations? Downloads and history will remain untouched.')) return;
    setDiagnosticError(null);
    try { await removeBrowserIntegration(); refresh(); }
    catch (reason) { setDiagnosticError(message(reason)); }
  }

  async function copyReport() {
    const report = {
      applicationVersion: setup?.appVersion, packagedExtensionVersion: setup?.packagedExtensionVersion,
      nativeHostVersion: setup?.nativeHostVersion, protocolVersion: setup?.protocolVersion,
      platform: diagnostics?.platform, ipcAvailable: diagnostics?.ipcAvailable,
      extensionConnected: diagnostics?.extensionConnected, lastSuccessfulPingAt: diagnostics?.lastSuccessfulPingAt,
      captureDecision: diagnostics?.captureDecision, jobAcceptanceStatus: diagnostics?.jobAcceptanceStatus,
      cancellationResult: diagnostics?.chromeCancellationResult, lastDownloadJobId: diagnostics?.lastDownloadJobId,
      lastFailureCategory: diagnostics?.lastFailureCategory,
      browsers: setup?.browsers.map(({ id, detected, nativeHostRegistered, hostExecutableValid }) => ({ id, detected, nativeHostRegistered, hostExecutableValid })),
    };
    await navigator.clipboard.writeText(JSON.stringify(report, null, 2));
  }

  return <div className="browser-diagnostics">
    <div><strong>Application</strong><span>v{setup?.appVersion ?? 'Unknown'}</span></div>
    <div><strong>Packaged extension</strong><span>v{setup?.packagedExtensionVersion ?? 'Unknown'}</span></div>
    <div><strong>Protocol</strong><span>v{setup?.protocolVersion ?? 'Unknown'}</span></div>
    <div><strong>Native host</strong><span>{diagnostics?.nativeHostRegistered ? 'Registered' : 'Not registered'}</span></div>
    <div><strong>Private IPC</strong><span>{diagnostics?.ipcAvailable ? 'Available' : 'Unavailable'}</span></div>
    <div><strong>Extension connected</strong><span>{diagnostics?.extensionConnected ? 'Yes' : 'No recent PING'}</span></div>
    <div><strong>Last successful PING</strong><span>{diagnostics?.lastSuccessfulPingAt ? new Date(diagnostics.lastSuccessfulPingAt * 1000).toLocaleString() : 'None'}</span></div>
    <div><strong>Automatic interception</strong><span>{diagnostics?.automaticInterceptionEnabled === null ? 'Unknown' : diagnostics?.automaticInterceptionEnabled ? 'Enabled' : 'Disabled'}</span></div>
    <div><strong>Downloads permission</strong><span>{diagnostics?.downloadsPermissionGranted === null ? 'Unknown' : diagnostics?.downloadsPermissionGranted ? 'Granted' : 'Denied'}</span></div>
    <div><strong>Last download event</strong><span>{diagnostics?.lastDownloadEventAt ? new Date(diagnostics.lastDownloadEventAt).toLocaleString() : 'None'}</span></div>
    <div><strong>Last URL hostname</strong><span>{diagnostics?.lastUrlHostname ?? 'None'}</span></div>
    <div><strong>Capture decision</strong><span>{diagnostics?.captureDecision ?? 'None'}</span></div>
    <div><strong>GrabBit acceptance</strong><span>{diagnostics?.jobAcceptanceStatus ?? 'None'}</span></div>
    <div><strong>Browser cancellation</strong><span>{diagnostics?.chromeCancellationResult ?? 'None'}</span></div>
    <div><strong>Last job ID</strong><span>{diagnostics?.lastDownloadJobId ?? 'None'}</span></div>
    <div><strong>Failure category</strong><span>{diagnostics?.lastFailureCategory ?? 'None'}</span></div>
    <div className="diagnostic-actions">
      <button className="secondary compact" onClick={refresh} type="button">Test connection status</button>
      <button className="secondary compact" onClick={() => void repair()} type="button">Repair integration</button>
      <button className="secondary compact" onClick={() => void remove()} type="button">Remove browser integration</button>
      <button className="secondary compact" onClick={() => void copyReport().catch((reason: unknown) => setDiagnosticError(message(reason)))} type="button">Copy sanitized report</button>
    </div>
    {setup?.browsers.map((browser) => <div className="browser-status" key={browser.id}>
      <strong>{browser.name}</strong>
      <span>{browser.detected ? 'Detected' : 'Not detected'} · host {browser.nativeHostRegistered ? 'registered' : 'not registered'} · extension unknown until it connects</span>
      {browser.setupLocation && <code>{browser.setupLocation}</code>}
    </div>)}
    {diagnostics?.manifestLocations.map((location) => <code key={location}>{location}</code>)}
    {(diagnosticError ?? diagnostics?.lastError) && <p className="inline-error">{diagnosticError ?? diagnostics?.lastError}</p>}
  </div>;
}

function OnboardingWizard({ settings, onChangeSettings, onClose, onError }: { settings: AppSettings; onChangeSettings: (settings: AppSettings) => void; onClose: () => void; onError: (error: string) => void }) {
  const [step, setStep] = useState(0);
  const [report, setReport] = useState<BrowserSetupReport | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const hasInitializedSelection = useRef(false);
  const steps = ['Welcome', 'Detect browsers', 'Choose integrations', 'Install native host', 'Install extensions', 'Verify connection', 'Download preferences', 'Finish'];
  const refresh = useCallback(() => {
    void getBrowserSetup().then((next) => {
      setReport(next);
      if (!hasInitializedSelection.current) {
        setSelected(new Set(next.browsers.filter((browser) => browser.detected && browser.id !== 'safari').map((browser) => browser.id)));
        hasInitializedSelection.current = true;
      } else {
        const detected = new Set(next.browsers.filter((browser) => browser.detected && browser.id !== 'safari').map((browser) => browser.id));
        setSelected((current) => new Set([...current].filter((id) => detected.has(id))));
      }
    }).catch((reason: unknown) => onError(message(reason)));
  }, [onError]);
  useEffect(refresh, [refresh]);
  async function finish() {
    try { await saveSettings(settings); await setOnboardingComplete(true); onClose(); }
    catch (reason) { onError(message(reason)); }
  }
  async function skip() {
    try { await setOnboardingComplete(true); onClose(); }
    catch (reason) { onError(message(reason)); }
  }
  async function repair() {
    if (!selected.size) { onError('Select at least one detected browser to configure.'); return; }
    setBusy(true);
    try { setReport(await repairBrowserIntegration([...selected])); }
    catch (reason) { onError(message(reason)); }
    finally { setBusy(false); }
  }
  return <div className="modal-backdrop"><section aria-label="GrabBit setup" aria-modal="true" className="setup-wizard" role="dialog">
    <header><img alt="GrabBit" src="/branding/logo-icon.png" /><div><h2>Welcome to GrabBit</h2><p>Browser integration setup · Step {step + 1} of {steps.length}</p></div><button className="icon-button" onClick={() => void skip()} title="Skip setup" type="button"><X size={18} /></button></header>
    <div className="setup-progress">{steps.map((name, index) => <span className={index <= step ? 'active' : ''} key={name} title={name} />)}</div>
    <div className="setup-body">
      {step === 0 && <><h3>Fast downloads, with your browser under your control</h3><p>GrabBit works without an extension. This wizard optionally connects supported browsers using a private, user-scoped native host.</p></>}
      {step === 1 && <><h3>Detected browsers</h3><div className="browser-list">{report?.browsers.length ? report.browsers.map((browser) => <div key={browser.id}><strong>{browser.name}</strong><span>{browser.detected ? 'Detected' : 'Not detected'}</span></div>) : <p>Browser detection is still loading. You can continue with manual downloads.</p>}</div><button className="secondary compact" onClick={refresh} type="button">Recheck browsers</button></>}
      {step === 2 && <><h3>Choose browser integrations</h3><p>Set up only browsers you use. GrabBit never edits browser profiles or force-installs extensions.</p><div className="browser-list">{report?.browsers.filter((browser) => browser.detected).map((browser) => <label className="browser-choice" key={browser.id}><input checked={selected.has(browser.id)} disabled={browser.id === 'safari'} onChange={(event) => setSelected((current) => { const next = new Set(current); if (event.target.checked) next.add(browser.id); else next.delete(browser.id); return next; })} type="checkbox" /><strong>{browser.name}</strong><span>{browser.id === 'safari' ? 'Requires signed Safari extension approval' : 'Include in guided setup'}</span></label>)}</div>{report && !report.browsers.some((browser) => browser.detected && browser.id !== 'safari') && <p>No supported browser was detected. You can skip integration and continue using manual downloads.</p>}</>}
      {step === 3 && <><h3>Install or repair the native host</h3><p>This writes only GrabBit-owned user-level manifests and registration keys. It does not require browser policy changes.</p><button className="primary" disabled={busy || !selected.size} onClick={() => void repair()} type="button">{busy ? 'Repairing…' : 'Install / repair native host'}</button><code>{report?.hostExecutable}</code></>}
      {step === 4 && <><h3>Approve the extension in your browser</h3><p>Browsers require your approval. Open the browser extension manager, enable Developer mode for this beta, choose Load unpacked, and select the folder below. Reload it after GrabBit upgrades.</p>{report?.browsers.filter((browser) => selected.has(browser.id) && browser.setupLocation).map((browser) => <div className="setup-path" key={browser.id}><strong>{browser.name}</strong><code>{browser.setupLocation}</code><button className="secondary compact" onClick={() => void navigator.clipboard.writeText(browser.setupLocation ?? '')} type="button">Copy path</button></div>)}</>}
      {step === 5 && <><h3>Verify browser connectivity</h3><p>Open the GrabBit extension popup in each browser and confirm it says Connected. Per-browser installation cannot be inferred from browser detection alone.</p><BrowserDiagnosticsPanel /></>}
      {step === 6 && <><h3>Download preferences</h3><label>Download directory<div className="field-with-action"><Folder size={17} /><input onChange={(event) => onChangeSettings({ ...settings, defaultDownloadDirectory: event.target.value })} value={settings.defaultDownloadDirectory} /><button className="secondary compact" onClick={() => void chooseDownloadDirectory().then((value) => value && onChangeSettings({ ...settings, defaultDownloadDirectory: value }))} type="button">Browse</button></div></label><label>Browser interception<select onChange={(event) => { const value = event.target.value as AppSettings['browserCaptureMode']; onChangeSettings({ ...settings, browserCaptureMode: value }); }} value={settings.browserCaptureMode}><option value="ask">Ask every time</option><option value="automatic">Automatic</option><option value="off">Disabled</option></select></label></>}
      {step === 7 && <><h3>Setup is ready</h3><p>GrabBit remains usable for manual URLs even when no extension is installed. You can reopen this wizard from Settings at any time.</p></>}
    </div>
    <footer><button className="secondary" onClick={() => void skip()} type="button">Skip setup</button><div><button className="secondary" disabled={step === 0} onClick={() => setStep((value) => value - 1)} type="button">Back</button>{step < steps.length - 1 ? <button className="primary" onClick={() => setStep((value) => value + 1)} type="button">Continue</button> : <button className="primary" onClick={() => void finish()} type="button">Finish setup</button>}</div></footer>
  </section></div>;
}
function SettingsSection({ children, description, id, title }: { children: React.ReactNode; description: string; id: string; title: string }) { return <section className="settings-section" id={id}><div className="section-heading"><div><h2>{title}</h2><p>{description}</p></div></div>{children}</section>; }
function About({ backend }: { backend: string }) { return <section className="about-view"><div className="about-logo"><img alt="GrabBit" src="/branding/logo-icon.png" /></div><h2>GrabBit</h2><p>A resilient cross-platform download manager.</p><span className="version-pill">{backend}</span><div className="about-facts"><div><strong>Engine</strong><span>Rust + Tokio</span></div><div><strong>Desktop</strong><span>Tauri 2</span></div><div><strong>Protocols</strong><span>HTTP / HTTPS</span></div></div><p className="about-note">GrabBit validates ranged responses and persists recovery checkpoints before advancing progress.</p></section>; }

function isListView(view: View) { return !['activity', 'settings', 'about'].includes(view); }
function matchesView(job: DownloadJob, view: View) { if (view === 'downloads') return true; if (view === 'active') return ['downloading', 'inspecting', 'merging', 'converting', 'verifying'].includes(job.state); if (view === 'queued') return ['queued', 'paused'].includes(job.state); if (view === 'completed' || view === 'failed') return job.state === view; if (categories.some(([category]) => category === view)) return fileCategory(job.fileName) === view; return false; }
function matchesSearch(job: DownloadJob, query: string) { const value = query.trim().toLowerCase(); return !value || [job.fileName, job.url, job.destination].some((text) => text.toLowerCase().includes(value)); }
function compare(a: DownloadJob, b: DownloadJob, key: SortKey) { const values = { name: () => a.fileName.localeCompare(b.fileName), size: () => (a.totalBytes ?? -1) - (b.totalBytes ?? -1), progress: () => progress(a) - progress(b), speed: () => a.speedBytesPerSecond - b.speedBytesPerSecond, eta: () => (a.etaSeconds ?? Number.MAX_SAFE_INTEGER) - (b.etaSeconds ?? Number.MAX_SAFE_INTEGER), status: () => a.state.localeCompare(b.state) }; return values[key](); }
function progress(job: DownloadJob) { return job.totalBytes ? Math.min(100, job.downloadedBytes / job.totalBytes * 100) : 0; }
function fileCategory(name: string): View { const ext = name.split('.').pop()?.toLowerCase() ?? ''; if (['pdf', 'doc', 'docx', 'txt', 'rtf', 'xls', 'xlsx', 'ppt', 'pptx', 'csv'].includes(ext)) return 'documents'; if (['mp4', 'mkv', 'mov', 'avi', 'webm', 'm4v'].includes(ext)) return 'videos'; if (['mp3', 'wav', 'flac', 'aac', 'm4a', 'ogg'].includes(ext)) return 'audio'; if (['exe', 'msi', 'dmg', 'pkg', 'deb', 'rpm', 'appimage', 'apk'].includes(ext)) return 'software'; if (['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz'].includes(ext)) return 'archives'; return 'others'; }
function fileIcon(name: string): LucideIcon { const icons: Partial<Record<View, LucideIcon>> = { documents: FileText, videos: FileVideo, audio: FileAudio, software: FileCode2, archives: FileArchive, others: File }; return icons[fileCategory(name)] ?? File; }
function title(view: View) { return ({ downloads: 'Downloads', active: 'Active downloads', queued: 'Queue', completed: 'Completed', failed: 'Failed downloads', activity: 'Activity', documents: 'Documents', videos: 'Videos', audio: 'Audio', software: 'Software', archives: 'Archives', others: 'Other files', settings: 'Settings', about: 'About GrabBit' } as Record<View, string>)[view]; }
function subtitle(view: View) { if (view === 'activity') return 'Live engine and transfer activity'; if (view === 'settings') return 'Configure GrabBit for your workflow'; if (view === 'about') return 'Version and application information'; return categories.some(([category]) => category === view) ? 'Downloads grouped by file type' : 'Manage and monitor your transfers'; }
function formatBytes(value: number) { if (value < 1024) return `${value} B`; const units = ['KB', 'MB', 'GB', 'TB']; let size = value / 1024; let unit = 0; while (size >= 1024 && unit < units.length - 1) { size /= 1024; unit++; } return `${size.toFixed(size >= 10 ? 1 : 2)} ${units[unit]}`; }
function formatDuration(seconds: number) { return seconds < 60 ? `${seconds}s` : seconds < 3600 ? `${Math.ceil(seconds / 60)}m` : `${Math.ceil(seconds / 3600)}h`; }
function relativeTime(timestamp: number) { const seconds = Math.max(0, Math.floor(Date.now() / 1000) - timestamp); return seconds < 60 ? 'Just now' : seconds < 3600 ? `${Math.floor(seconds / 60)}m ago` : seconds < 86400 ? `${Math.floor(seconds / 3600)}h ago` : `${Math.floor(seconds / 86400)}d ago`; }
function capitalize(value: string) { return value.charAt(0).toUpperCase() + value.slice(1); }
function message(reason: unknown) { return reason instanceof Error ? reason.message : String(reason); }
function validUrl(value: string) { try { return ['http:', 'https:'].includes(new URL(value.trim()).protocol); } catch { return false; } }
