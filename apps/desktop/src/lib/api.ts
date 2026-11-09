// Typed wrappers over the Tauri commands (src-tauri/src/commands.rs) and events
// (src-tauri/src/bridge.rs). Failures reject with an AppError.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type { AppError, Bootstrap, NewTask, ProbeInfo, ProgressBatch, Settings, Task, TaskDetail, TaskOptions } from './types';

export function isAppError(e: unknown): e is AppError {
  return typeof e === 'object' && e !== null && 'code' in e && 'message' in e;
}

/** Anything thrown by invoke, as an AppError. */
export function asError(e: unknown): AppError {
  return isAppError(e) ? e : { code: 'internal', message: String(e) };
}

export const api = {
  bootstrap: () => invoke<Bootstrap>('app_bootstrap'),
  listTasks: () => invoke<Task[]>('task_list'),
  setSettings: (settings: Settings) => invoke<Settings>('settings_set', { settings }),
  setSpeedLimit: (limit: number | null) => invoke<Settings>('speed_limit_set', { limit }),

  probe: (url: string, options: TaskOptions) => invoke<ProbeInfo>('task_probe', { url, options }),
  add: (items: NewTask[]) => invoke<Task[]>('task_add', { items }),
  pause: (ids: string[]) => invoke<void>('task_pause', { ids }),
  resume: (ids: string[]) => invoke<void>('task_resume', { ids }),
  restart: (ids: string[]) => invoke<void>('task_restart', { ids }),
  remove: (ids: string[]) => invoke<void>('task_remove', { ids }),
  restore: (ids: string[]) => invoke<void>('task_restore', { ids }),
  purge: (ids: string[], deleteFiles: boolean) => invoke<void>('task_purge', { ids, deleteFiles }),
  detail: (id: string) => invoke<TaskDetail>('task_detail', { id }),
  setTaskSpeedLimit: (id: string, limit: number | null) => invoke<void>('task_set_speed_limit', { id, limit }),
  open: (id: string) => invoke<void>('task_open', { id }),
  reveal: (id: string) => invoke<void>('task_reveal', { id }),
  openFolder: (path: string) => invoke<void>('folder_open', { path }),
  diskFree: (path: string) => invoke<number | null>('disk_free', { path }),
};

export interface TaskEvents {
  added: (t: Task) => void;
  changed: (t: Task) => void;
  deleted: (ids: string[]) => void;
  progress: (b: ProgressBatch) => void;
  /** Events were missed: reload the list. */
  resync: () => void;
}

export async function listenTasks(h: TaskEvents): Promise<UnlistenFn> {
  const off = await Promise.all([
    listen<Task>('task:added', (e) => h.added(e.payload)),
    listen<Task>('task:changed', (e) => h.changed(e.payload)),
    listen<string[]>('task:deleted', (e) => h.deleted(e.payload)),
    listen<ProgressBatch>('task:progress', (e) => h.progress(e.payload)),
    listen('task:resync', () => h.resync()),
  ]);
  return () => off.forEach((f) => f());
}
