// Numbers for people: sizes, speeds, durations, times. Chinese wording until i18n (Phase 5).

const UNITS = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];

/** 1024-based: whole numbers from 100, one decimal from 10, two below: `643 MB`, `5.9 GB`, `1.24 GB`. */
export function bytes(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return '—';
  if (n < 1024) return `${Math.max(0, Math.round(n))} B`;
  let v = n;
  let u = 0;
  while (v >= 1024 && u < UNITS.length - 1) {
    v /= 1024;
    u++;
  }
  const round = (x: number) => (x >= 100 ? Math.round(x) : x >= 10 ? Math.round(x * 10) / 10 : Math.round(x * 100) / 100);
  let r = round(v);
  // 1023.9 KB rounds to 1024: that is 1 MB.
  if (r >= 1024 && u < UNITS.length - 1) {
    u++;
    r = round(v / 1024);
  }
  return `${r} ${UNITS[u]}`;
}

export function speed(bps: number | null | undefined): string {
  return bps == null ? '—' : `${bytes(bps)}/s`;
}

/** `47 秒`, `3 分 5 秒`, `2 小时 3 分`, `4 天 2 小时`. */
export function duration(secs: number | null | undefined): string {
  if (secs == null || !Number.isFinite(secs) || secs < 0) return '—';
  const s = Math.round(secs);
  if (s < 60) return `${s} 秒`;
  const m = Math.floor(s / 60);
  if (m < 60) return s % 60 ? `${m} 分 ${s % 60} 秒` : `${m} 分`;
  const h = Math.floor(m / 60);
  if (h < 24) return m % 60 ? `${h} 小时 ${m % 60} 分` : `${h} 小时`;
  const d = Math.floor(h / 24);
  return h % 24 ? `${d} 天 ${h % 24} 小时` : `${d} 天`;
}

/** Whole percent, never 100 before the end. */
export function percent(done: number, total: number | null): string {
  if (!total) return '—';
  return `${Math.min(Math.floor((done / total) * 100), done >= total ? 100 : 99)}%`;
}

export function ratio(done: number, total: number | null): number {
  return total ? Math.min(1, done / total) : 0;
}

/** `10:42` today, `昨天 10:42`, `11月3日`, `2025年11月3日`. */
export function when(ms: number | null | undefined, now = Date.now()): string {
  if (ms == null) return '—';
  const d = new Date(ms);
  const today = new Date(now);
  const hm = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
  const sameDay = (a: Date, b: Date) => a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
  if (sameDay(d, today)) return hm;
  const y = new Date(today);
  y.setDate(y.getDate() - 1);
  if (sameDay(d, y)) return `昨天 ${hm}`;
  const md = `${d.getMonth() + 1}月${d.getDate()}日`;
  return d.getFullYear() === today.getFullYear() ? md : `${d.getFullYear()}年${md}`;
}

/**
 * A speed typed by the user: `500` (KB/s), `500K`, `2M`, `1.5 MB/s`, `1g`. Empty is unlimited
 * (`null`); anything else unreadable is `undefined`.
 */
export function parseSpeed(input: string): number | null | undefined {
  const s = input.trim().toLowerCase().replace(/\s+/g, '').replace(/\/s$/, '').replace(/ps$/, '');
  if (!s || s === '0' || s === '不限' || s === '不限速') return null;
  const m = /^(\d+(?:\.\d+)?)([kmg]?)(?:i?b)?$/.exec(s);
  if (!m) return undefined;
  const n = Number(m[1]);
  const mult = { '': 1024, k: 1024, m: 1024 ** 2, g: 1024 ** 3 }[m[2] as '' | 'k' | 'm' | 'g'];
  const v = Math.round(n * mult);
  return v > 0 ? v : null;
}
