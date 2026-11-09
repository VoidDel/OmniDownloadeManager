// Messages to the Omni Download app — the TypeScript twin of crates/omnidl-ipc/src/protocol.rs
// (snake_case on the wire, as there).

export const HOST_NAME = 'app.omnidownload.host';

export interface AddDownload {
  url: string;
  filename?: string | null;
  size?: number | null;
  referer?: string | null;
  user_agent?: string | null;
  cookies?: string | null;
  headers?: Record<string, string>;
  mime?: string | null;
  tab_url?: string | null;
  /** Start without the quick-download window. */
  silent?: boolean;
}

export type RequestBody =
  | { type: 'ping' }
  | { type: 'get_status' }
  | { type: 'open_app' }
  | { type: 'add_download'; payload: AddDownload }
  | { type: 'add_batch'; payload: { items: AddDownload[] } }
  | { type: 'add_magnet'; payload: { uri: string } };

export type Request = RequestBody & { id: string; browser?: string };

export type ErrorCode = 'app_not_running' | 'bad_request' | 'unsupported' | 'unauthorized' | 'internal';

export interface Response<T = unknown> {
  id: string;
  ok: boolean;
  result?: T;
  error?: { code: ErrorCode; message: string };
}

export interface RecentTask {
  id: string;
  name: string;
  status: string;
  progress: number | null;
}

export interface Status {
  app_version: string;
  running: number;
  download_speed: number;
  recent: RecentTask[];
}

export type AddAction = 'started' | 'queued' | 'browser' | 'cancelled' | 'dialog';

export interface AddResult {
  action: AddAction;
  /** The user saw the quick-download window. */
  asked: boolean;
  task_id?: string;
}
