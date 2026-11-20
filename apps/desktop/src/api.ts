// Types and calls shared by the whole interface. In the application every call goes to
// the Rust core. In a plain browser, where the core is absent, a small demo library
// answers instead, so the interface can be developed and looked at on its own.

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type Album = {
  id: number;
  title: string;
  artist: string;
  year: number | null;
  track_count: number;
  duration_ms: number;
  artwork: string | null;
  color: string | null;
};

export type Track = {
  id: number;
  path: string;
  title: string;
  artist: string;
  album: string;
  album_id: number | null;
  disc_no: number | null;
  track_no: number | null;
  duration_ms: number;
  sample_rate: number | null;
  bit_depth: number | null;
  codec: string;
  artwork: string | null;
};

export type AlbumDetail = { album: Album; tracks: Track[] };

export type Artist = { name: string; album_count: number; track_count: number; artwork: string | null };

export type SearchResults = { albums: Album[]; artists: Artist[]; tracks: Track[] };

export type LibraryStatus = {
  roots: string[];
  counts: { tracks: number; albums: number; artists: number };
  scanning: boolean;
  suggested_root: string | null;
};

export type ScanProgress =
  | { phase: 'listing'; files: number }
  | { phase: 'reading'; done: number; total: number }
  | { phase: 'artwork'; done: number; total: number }
  | { phase: 'done'; summary: { tracks: number; albums: number; added: number; removed: number; unreadable: number } };

export type Reason =
  | { kind: 'lossy_source' }
  | { kind: 'shared_mode' }
  | { kind: 'sample_rate'; from: number; to: number }
  | { kind: 'channels'; from: number; to: number }
  | { kind: 'float' }
  | { kind: 'bit_depth'; from: number; to: number };

export type Signal = {
  device: string;
  exclusive: boolean;
  bit_perfect: boolean;
  source_rate: number;
  source_bits: number | null;
  output_rate: number;
  output_bits: number | null;
  reasons: Reason[];
};

export type PlayerStatus = 'stopped' | 'playing' | 'paused';

export type Snapshot = {
  status: PlayerStatus;
  index: number | null;
  track: Track | null;
  position_ms: number;
  duration_ms: number;
  volume: number;
  exclusive: boolean;
  queue_len: number;
  context: string;
  signal: Signal | null;
};

export type OutputState = {
  devices: { id: string; name: string; is_default: boolean }[];
  device_id: string | null;
  exclusive: boolean;
};

export const inApp = '__TAURI_INTERNALS__' in window;

type Mock = typeof import('./mock');
let mock: Promise<Mock> | null = null;
const demo = () => (mock ??= import('./mock'));

/** Calls a command of the core. */
export async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (inApp) return invoke<T>(command, args);
  return (await demo()).call(command, args ?? {}) as T;
}

/** Subscribes to an event of the core. Returns the function that unsubscribes. */
export function on<T>(event: string, handler: (payload: T) => void): () => void {
  let cancelled = false;
  let stop: (() => void) | null = null;
  const attach = inApp
    ? listen<T>(event, (e) => handler(e.payload))
    : demo().then((m) => m.on(event, handler as (payload: unknown) => void));
  attach.then((unlisten) => {
    if (cancelled) unlisten();
    else stop = unlisten;
  });
  return () => {
    cancelled = true;
    stop?.();
  };
}

/** Address of a cached cover. The small size is for lists, the large one for the player. */
export function coverUrl(hash: string | null, size: 400 | 1000 = 400): string | null {
  if (!hash) return null;
  if (hash.startsWith('data:')) return hash;
  return `http://vyra-art.localhost/${hash}/${size}`;
}
