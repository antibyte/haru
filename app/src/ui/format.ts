/** Number formatting in the active language and small label helpers for the overlays. Unit symbols stay as they are. */
import { formatNumber } from './strings';

/** Decimal units (1 GB = 1e9 B), two decimals: de "6,62 GB", en "6.62 GB". */
export function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n < 0) return '–';
  if (n >= 1e9) return `${formatNumber(n / 1e9, 2)} GB`;
  if (n >= 1e6) return `${formatNumber(n / 1e6, 2)} MB`;
  if (n >= 1e3) return `${formatNumber(n / 1e3, 2)} KB`;
  return `${Math.round(n)} B`;
}

/** Transfer rate with one decimal: de "42,1 MB/s", en "42.1 MB/s". */
export function formatRate(bps: number): string {
  if (!Number.isFinite(bps) || bps < 0) return '–';
  if (bps >= 1e9) return `${formatNumber(bps / 1e9, 1)} GB/s`;
  if (bps >= 1e6) return `${formatNumber(bps / 1e6, 1)} MB/s`;
  if (bps >= 1e3) return `${formatNumber(bps / 1e3, 1)} KB/s`;
  return `${Math.round(bps)} B/s`;
}

/** Remaining time: "45 s", "1 min 45 s", "1 h 2 min", "–" when unknown. */
export function formatEta(secs: number): string {
  if (!Number.isFinite(secs) || secs < 0) return '–';
  const s = Math.round(secs);
  if (s < 60) return `${s} s`;
  if (s < 3600) {
    const m = Math.floor(s / 60);
    const r = s % 60;
    return r === 0 ? `${m} min` : `${m} min ${r} s`;
  }
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return m === 0 ? `${h} h` : `${h} h ${m} min`;
}

/** Product names of the backends (not translated). */
export const BACKEND_NAMES: Readonly<Record<string, string>> = { cuda: 'CUDA', vulkan: 'Vulkan', metal: 'Metal', cpu: 'CPU' };

/** HUD badge text, e.g. "Vulkan · Intel Arc B580". */
export function badgeLabel(backend: string, device: string): string {
  if (backend === 'cpu') return 'CPU';
  const name = BACKEND_NAMES[backend] ?? backend;
  const dev = device
    .replace(/\((R|TM)\)/gi, '')
    .replace(/\s+/g, ' ')
    .trim()
    .replace(/\s+Graphics$/i, '');
  return dev ? `${name} · ${dev}` : name;
}
