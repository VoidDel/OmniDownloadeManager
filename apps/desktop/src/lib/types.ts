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
  etaSecs: number | null;
  connections: number;
  category: Category;
  priority: Priority;
  resumable: boolean;
  options: TaskOptions;
  error: AppError | null;
  createdAt: number;
  startedAt: number | null;
  completedAt: number | null;
}

export interface ProbeInfo {
  finalUrl: string;
  fileName: string;
  total: number | null;
  resumable: boolean;
  contentType: string | null;
  etag: string | null;
  lastModified: string | null;
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
}

export interface ProgressItem {
  id: string;
  downloaded: number;
  total: number | null;
  speed: number;
  eta: number | null;
  connections: number;
}

export interface Stats {
  downloadSpeed: number;
  running: number;
}

export interface ProgressBatch {
  items: ProgressItem[];
  stats: Stats;
}

export type ThemeMode = 'system' | 'light' | 'dark';
export type Density = 'compact' | 'comfortable' | 'spacious';

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
}

export interface Settings {
  appearance: {
    mode: ThemeMode;
    lightTheme: string;
    darkTheme: string;
    density: Density;
  };
  downloadDir: string;
  engine: EngineConfig;
}

export interface Bootstrap {
  settings: Settings;
  tasks: Task[];
  version: string;
  defaultDir: string;
  logDir: string;
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
});
