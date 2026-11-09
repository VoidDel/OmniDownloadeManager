// Shapes of what the Rust side sends and receives (serde, camelCase). Kept by hand next to their
// Rust definitions in omnidl-core (task.rs, options.rs, config.rs, engine.rs, events.rs) and the
// app's settings.rs; numbers are bytes, bytes per second, seconds or Unix milliseconds.

export type TaskKind = 'http' | 'ftp' | 'torrent';

export type TaskStatus = 'queued' | 'probing' | 'downloading' | 'paused' | 'seeding' | 'completed' | 'failed' | 'removed';

export type Category = 'video' | 'audio' | 'document' | 'archive' | 'program' | 'disk_image' | 'image' | 'other';

export type Priority = 'high' | 'normal' | 'low';

export type ErrorCode =
  | 'invalid_url'
  | 'unsupported'
  | 'network'
  | 'timeout'
  | 'tls'
  | 'http_status'
  | 'ftp_reply'
  | 'not_found'
  | 'auth_required'
  | 'forbidden'
  | 'remote_changed'
  | 'file_missing'
  | 'disk_full'
  | 'io'
  | 'invalid_request'
  | 'internal';

export interface AppError {
  code: ErrorCode;
  message: string;
  status?: number;
}

export type ProxyConfig = { mode: 'system' } | { mode: 'none' } | { mode: 'custom'; url: string };

export interface TaskOptions {
  fileName: string | null;
  connections: number | null;
  speedLimit: number | null;
  userAgent: string | null;
  referer: string | null;
  cookies: string | null;
  headers: Record<string, string>;
  proxy: ProxyConfig | null;
  /** BitTorrent: indices of the files to download (null: all). */
  onlyFiles: number[] | null;
  /** FTP account (anonymous when null); a password in the link moves here. */
  username: string | null;
  password: string | null;
  ftpActive: boolean | null;
  /** FTPS: accept a certificate that does not verify. */
  insecureTls: boolean | null;
}

export interface Task {
  id: string;
  kind: TaskKind;
  source: string;
  finalUrl: string | null;
  saveDir: string;
  fileName: string;
  status: TaskStatus;
  totalBytes: number | null;
  downloadedBytes: number;
  uploadedBytes: number;
  speedBps: number;
  uploadSpeedBps: number;
  etaSecs: number | null;
  /** Peers, for a torrent. */
  connections: number;
  category: Category;
  priority: Priority;
  resumable: boolean;
  options: TaskOptions;
  error: AppError | null;
  createdAt: number;
  startedAt: number | null;
  completedAt: number | null;
  /** Time spent downloading, pauses excluded (ms). */
  activeMs: number;
  /** The highest download speed seen (bytes per second). */
  peakBps: number;
}

export interface ProbeInfo {
  finalUrl: string;
  fileName: string;
  total: number | null;
  resumable: boolean;
  contentType: string | null;
  etag: string | null;
  lastModified: string | null;
  /** A torrent's files in its order (absent for other downloads). */
  files?: ProbeFile[];
}

export interface ProbeFile {
  /** Relative, `/`-separated. */
  path: string;
  size: number;
}

export interface NewTask {
  source: string;
  saveDir: string;
  options: TaskOptions;
  start: boolean;
  probe: ProbeInfo | null;
}

export interface SegmentView {
  start: number;
  end: number | null;
  downloaded: number;
  active: boolean;
}

export interface TaskDetail {
  task: Task;
  segments: SegmentView[];
  path: string;
  torrent?: TorrentDetail;
}

export interface TorrentDetail {
  infoHash: string;
  name: string;
  pieceSize: number;
  pieces: number;
  files: TorrentFile[];
  peers: PeerInfo[];
  peersSeen: number;
  trackers: string[];
}

export interface TorrentFile {
  index: number;
  path: string;
  size: number;
  /** Verified bytes while the task runs. */
  done: number | null;
  selected: boolean;
}

export interface PeerInfo {
  addr: string;
  client: string | null;
  transport: string;
  downloaded: number;
  uploaded: number;
}

export interface ProgressItem {
  id: string;
  downloaded: number;
  total: number | null;
  speed: number;
  eta: number | null;
  connections: number;
  uploaded: number;
  uploadSpeed: number;
}

export interface Stats {
  downloadSpeed: number;
  uploadSpeed: number;
  running: number;
  seeding: number;
}

export interface ProgressBatch {
  items: ProgressItem[];
  stats: Stats;
}

export type ThemeMode = 'system' | 'light' | 'dark';
export type Density = 'compact' | 'comfortable' | 'spacious';
export type MaterialKind = 'mica' | 'mica-alt' | 'acrylic';

export interface EngineConfig {
  maxConcurrent: number;
  connections: number;
  speedLimit: number | null;
  userAgent: string;
  proxy: ProxyConfig;
  connectTimeoutSecs: number;
  readTimeoutSecs: number;
  maxRetries: number;
  conflict: 'rename' | 'overwrite';
  resumeOnStartup: boolean;
  syncWrites: boolean;
  seeding: boolean;
  /** 0: no limit. */
  seedRatio: number;
  /** 0: no limit. */
  seedMinutes: number;
  uploadLimit: number | null;
  btPort: number;
  btDht: boolean;
  btUpnp: boolean;
  ftpConnections: number;
  ftpActive: boolean;
}

export interface BrowserSettings {
  quickDialog: boolean;
  silentSites: string[];
  httpBridge: boolean;
  httpPort: number;
  httpToken: string;
}

export interface Settings {
  /** `auto` (the system's) or a supported language tag. */
  language: string;
  general: {
    /** Closing the main window hides it in the notification area instead of quitting. */
    closeToTray: boolean;
    notifyDone: boolean;
    notifyFailed: boolean;
    /** Global shortcuts (accelerators such as `CommandOrControl+Alt+D`); empty: none. */
    shortcutShow: string;
    shortcutNew: string;
    /** Look for a new version a little after starting. */
    checkUpdates: boolean;
    /** The first-run guide has been through. */
    onboarded: boolean;
  };
  appearance: {
    mode: ThemeMode;
    lightTheme: string;
    darkTheme: string;
    density: Density;
    /** Window material where the system has one. */
    material: boolean;
    /** Windows 11: which material (macOS always uses vibrancy). */
    materialKind: MaterialKind;
    /** `theme`, `system` or `#RRGGBB`. */
    accent: string;
  };
  downloadDir: string;
  engine: EngineConfig;
  browser: BrowserSettings;
}

export interface Bootstrap {
  settings: Settings;
  tasks: Task[];
  version: string;
  defaultDir: string;
  logDir: string;
  /** Started by the browser bridge: keep the main window hidden. */
  background: boolean;
  /** Browsers whose extension has been in touch: [name, Unix ms]. */
  seenBrowsers: [string, number][];
  /** Magnet links and .torrent files the app was opened with. */
  openLinks: string[];
  /** Window materials exist on this system (Windows 11, macOS). */
  materialSupported: boolean;
  /** The system's accent colour, when known. */
  systemAccent: string | null;
  /** The app starts with the system. */
  autostart: boolean;
}

/** A newer version (src-tauri/src/update.rs). */
export interface UpdateInfo {
  version: string;
  /** Release notes, possibly empty. */
  notes: string;
  date: string | null;
}

export interface UpdateProgress {
  downloaded: number;
  total: number | null;
}

export interface MagnetHandler {
  canRegister: boolean;
  registered: boolean;
}

export type BrowserId = 'chrome' | 'edge' | 'firefox' | 'brave' | 'chromium';

export interface BrowserState {
  id: BrowserId;
  name: string;
  detected: boolean;
  registered: boolean;
  ours: boolean;
}

/** The browser integration settings page (src-tauri/src/browser/mod.rs). */
export interface Integration {
  hostPath: string | null;
  bridgeOk: boolean;
  browsers: BrowserState[];
  seen: [string, number][];
  httpRunning: boolean;
  pairingCode: string | null;
  settings: Settings;
}

/** A download the browser handed over, waiting in the quick-download window. */
export interface QuickRequest {
  url: string;
  fileName: string | null;
  size: number | null;
  mime: string | null;
  site: string;
  browser: string | null;
  hasCookies: boolean;
  hasReferer: boolean;
  saveDir: string;
  connections: number;
  options: TaskOptions;
}

export interface QuickDecision {
  action: 'start' | 'later' | 'browser' | 'cancel';
  fileName?: string | null;
  saveDir?: string | null;
  connections?: number | null;
  speedLimit?: number | null;
  remember?: boolean;
  probe?: ProbeInfo | null;
}

/** What the new-download dialog opens with (a batch from the browser). */
export interface Prefill {
  text: string;
  referer?: string | null;
  cookies?: string | null;
  userAgent?: string | null;
}

export const emptyOptions = (): TaskOptions => ({
  fileName: null,
  connections: null,
  speedLimit: null,
  userAgent: null,
  referer: null,
  cookies: null,
  headers: {},
  proxy: null,
  onlyFiles: null,
  username: null,
  password: null,
  ftpActive: null,
  insecureTls: null,
});
