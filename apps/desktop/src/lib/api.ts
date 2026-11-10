// Typed wrappers over the Tauri commands (src-tauri/src/commands.rs) and events
// (src-tauri/src/events.rs, src-tauri/src/browser). Failures reject with an AppError.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type { AppError, Bootstrap, FloatState, Integration, PowerPending, MagnetHandler, NewTask, Prefill, ProbeInfo, ProgressBatch, QuickDecision, QuickRequest, Settings, Task, TaskDetail, TaskOptions, UpdateInfo, UpdateProgress } from './types';

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
  /** Starts with the system (or not); resolves to what the system now has. */
  setAutostart: (enabled: boolean) => invoke<boolean>('autostart_set', { enabled }),
  updateCheck: () => invoke<UpdateInfo | null>('update_check'),
  /** Downloads and installs the update found by the last check, then restarts the app. */
  updateInstall: () => invoke<void>('update_install'),
  /** The extension's store page for a browser (or how to install it). */
  extensionOpen: (browser: string) => invoke<void>('extension_open', { browser }),
  /** The user guide or the page for reporting a problem, in the browser. */
  helpOpen: (page: 'guide' | 'feedback') => invoke<void>('help_open', { page }),
  /** Plays the completion sound (`file`: a WAV file; empty: the built-in one). */
  soundTest: (file: string) => invoke<void>('sound_test', { file }),

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
  /** The checksum the file should have, as typed (empty clears it); a finished file is checked now. */
  setChecksum: (id: string, value: string) => invoke<Task>('task_set_checksum', { id, value }),
  /** MD5, SHA-1 and SHA-256 of a finished file: [algorithm, hex] pairs. */
  digests: (id: string) => invoke<[string, string][]>('task_digests', { id }),
  setTorrentFiles: (id: string, files: number[]) => invoke<void>('task_files_set', { id, files }),
  magnetStatus: () => invoke<MagnetHandler>('magnet_status'),
  magnetRegister: () => invoke<MagnetHandler>('magnet_register'),
  open: (id: string) => invoke<void>('task_open', { id }),
  /** Finished downloads whose file is no longer there. */
  missingFiles: () => invoke<string[]>('task_missing'),
  reveal: (id: string) => invoke<void>('task_reveal', { id }),
  openFolder: (path: string) => invoke<void>('folder_open', { path }),

  browserStatus: () => invoke<Integration>('browser_status'),
  browserRepair: () => invoke<Integration>('browser_repair'),
  httpBridgeSet: (enabled: boolean) => invoke<Integration>('http_bridge_set', { enabled }),
  httpBridgeNewToken: () => invoke<Integration>('http_bridge_new_token'),
  quickRequest: () => invoke<QuickRequest | null>('quick_request'),
  quickResolve: (decision: QuickDecision) => invoke<void>('quick_resolve', { decision }),

  powerPending: () => invoke<PowerPending | null>('power_pending'),
  powerCancel: () => invoke<void>('power_cancel'),
  powerNow: () => invoke<void>('power_now'),

  floatState: () => invoke<FloatState>('float_state'),
  floatDrop: (text: string) => invoke<void>('float_drop', { text }),
  floatDropTorrent: (name: string, data: number[]) => invoke<void>('float_drop_torrent', { name, data }),
  floatOpen: () => invoke<void>('float_open'),
  floatMenu: () => invoke<void>('float_menu'),
};

/** Window-level events from the app (not tasks). */
export interface AppEvents {
  /** Links to add (a batch from the browser, a magnet link or .torrent from the system). */
  newTask: (p: Prefill) => void;
  /** An extension got in touch for the first time since the app started. */
  browserSeen: (seen: [string, number][]) => void;
  /** The backend changed the settings (a site added to "don't ask again", the tray's limit). */
  settingsChanged: (s: Settings) => void;
  /** Everything downloaded: the computer sleeps or shuts down after a countdown. */
  powerCountdown: (p: PowerPending) => void;
  /** An update package downloading. */
  updateProgress: (p: UpdateProgress) => void;
}

export async function listenApp(h: AppEvents): Promise<UnlistenFn> {
  const off = await Promise.all([
    listen<Prefill>('app:new-task', (e) => h.newTask(e.payload)),
    listen<[string, number][]>('browser:seen', (e) => h.browserSeen(e.payload)),
    listen<Settings>('settings:changed', (e) => h.settingsChanged(e.payload)),
    listen<PowerPending | null>('power:countdown', (e) => e.payload && h.powerCountdown(e.payload)),
    listen<UpdateProgress>('update:progress', (e) => h.updateProgress(e.payload)),
  ]);
  return () => off.forEach((f) => f());
}

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
