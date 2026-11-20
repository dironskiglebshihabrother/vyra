// Interface state. Playback and the library belong to the core; this store mirrors
// what the core reports and keeps what only the window knows: the page and the panels.

import { create } from 'zustand';
import { call, on, type LibraryStatus, type ScanProgress, type Snapshot, type Track } from './api';

export type Route =
  | { name: 'albums' }
  | { name: 'album'; id: number }
  | { name: 'artists' }
  | { name: 'artist'; artist: string }
  | { name: 'tracks' }
  | { name: 'search'; text: string }
  | { name: 'settings' };

type Toast = { id: number; text: string };

type State = {
  route: Route;
  history: Route[];
  status: LibraryStatus | null;
  scan: ScanProgress | null;
  /** Grows whenever the library content changes; pages reload when it does. */
  libraryVersion: number;
  player: Snapshot;
  /** Last position reported by the core and the moment it arrived. */
  positionMs: number;
  positionAt: number;
  queue: Track[];
  nowPlayingOpen: boolean;
  queueOpen: boolean;
  toasts: Toast[];
  go: (route: Route) => void;
  back: () => void;
  setSearch: (text: string) => void;
  toast: (text: string) => void;
  openNowPlaying: (open: boolean) => void;
  toggleQueue: () => void;
  refreshStatus: () => Promise<void>;
};

const idle: Snapshot = {
  status: 'stopped',
  index: null,
  track: null,
  position_ms: 0,
  duration_ms: 0,
  volume: 1,
  exclusive: false,
  queue_len: 0,
  context: '',
  signal: null,
};

let toastId = 0;

export const useStore = create<State>((set, get) => ({
  route: { name: 'albums' },
  history: [],
  status: null,
  scan: null,
  libraryVersion: 0,
  player: idle,
  positionMs: 0,
  positionAt: performance.now(),
  queue: [],
  nowPlayingOpen: false,
  queueOpen: false,
  toasts: [],

  go: (route) => set((s) => ({ route, history: [...s.history, s.route].slice(-40) })),
  back: () =>
    set((s) => {
      const history = [...s.history];
      const route = history.pop();
      return route ? { route, history } : {};
    }),
  setSearch: (text) =>
    set((s) => {
      if (!text.trim()) return s.route.name === 'search' ? { route: s.history.at(-1) ?? { name: 'albums' }, history: s.history.slice(0, -1) } : {};
      if (s.route.name === 'search') return { route: { name: 'search', text } };
      return { route: { name: 'search', text }, history: [...s.history, s.route] };
    }),
  toast: (text) => {
    const id = ++toastId;
    set((s) => ({ toasts: [...s.toasts.slice(-2), { id, text }] }));
    setTimeout(() => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })), 6000);
  },
  openNowPlaying: (open) => set({ nowPlayingOpen: open }),
  toggleQueue: () => set((s) => ({ queueOpen: !s.queueOpen })),
  refreshStatus: async () => {
    const status = await call<LibraryStatus>('library_status');
    set({ status });
    if (!status.scanning && get().scan?.phase !== 'done') set({ scan: null });
  },
}));

/** Connects the store to the core. Called once at start. */
export function connect() {
  const { refreshStatus, toast } = useStore.getState();
  const setPlayer = (player: Snapshot) =>
    useStore.setState({ player, positionMs: player.position_ms, positionAt: performance.now() });

  refreshStatus();
  call<Snapshot>('player_snapshot').then(setPlayer);
  call<Track[]>('queue').then((queue) => useStore.setState({ queue }));

  on<Snapshot>('player', setPlayer);
  on<{ index: number; position_ms: number }>('position', (p) =>
    useStore.setState({ positionMs: p.position_ms, positionAt: performance.now() }),
  );
  on<unknown>('queue-changed', () => call<Track[]>('queue').then((queue) => useStore.setState({ queue })));
  on<{ message: string }>('player-error', (e) => toast(e.message));
  on<string>('scan-error', (message) => toast(message));
  on<ScanProgress>('scan', (scan) => {
    useStore.setState((s) => ({ scan, status: s.status ? { ...s.status, scanning: scan.phase !== 'done' } : s.status }));
  });
  on<unknown>('library-changed', () => {
    useStore.setState((s) => ({ libraryVersion: s.libraryVersion + 1 }));
    refreshStatus();
  });
}
