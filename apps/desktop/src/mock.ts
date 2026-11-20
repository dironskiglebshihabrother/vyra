// A demo library for running the interface in a browser without the core.
// It is never loaded inside the application.

import type { Album, AlbumDetail, Artist, Snapshot, Track } from './api';

type Seed = { title: string; artist: string; year: number; hue: number; tracks: string[]; bits: number; rate: number };

const seeds: Seed[] = [
  { title: 'Тихая вода', artist: 'Анна Левина', year: 2019, hue: 208, bits: 24, rate: 96000, tracks: ['Перед рассветом', 'Тихая вода', 'Мост', 'Письмо без адреса', 'Северный берег', 'Домой'] },
  { title: 'Harbor Lights', artist: 'Nina Sol', year: 2021, hue: 24, bits: 16, rate: 44100, tracks: ['Dusk', 'Harbor Lights', 'Lanterns', 'Salt', 'Low Tide', 'Morning Ferry', 'Last Train'] },
  { title: 'Этажи', artist: 'Квартал 12', year: 2016, hue: 142, bits: 16, rate: 44100, tracks: ['Подъезд', 'Этажи', 'Соседи', 'Лифт', 'Крыша'] },
  { title: 'Paper Planes', artist: 'The Lowlands', year: 2014, hue: 48, bits: 16, rate: 44100, tracks: ['Paper Planes', 'Atlas', 'Good Company', 'Northbound', 'Postcards', 'Sundown'] },
  { title: 'Ночной трамвай', artist: 'Анна Левина', year: 2022, hue: 262, bits: 24, rate: 48000, tracks: ['Депо', 'Ночной трамвай', 'Окно', 'Кольцо', 'Последняя остановка'] },
  { title: 'Slow Machines', artist: 'Orbit Hotel', year: 2018, hue: 328, bits: 24, rate: 192000, tracks: ['Start', 'Slow Machines', 'Glass Room', 'Relay', 'Signal', 'Shutdown'] },
  { title: 'Blue Hours', artist: 'Marek Trio', year: 2009, hue: 222, bits: 16, rate: 44100, tracks: ['Blue Hours', 'Waltz for M.', 'Corner Table', 'After Rain', 'Closing Time'] },
  { title: 'Поле', artist: 'Лес и река', year: 2020, hue: 96, bits: 16, rate: 44100, tracks: ['Утро', 'Поле', 'Ветер', 'Костёр', 'Звёзды'] },
  { title: 'Concrete Garden', artist: 'Nina Sol', year: 2017, hue: 8, bits: 16, rate: 44100, tracks: ['Seed', 'Concrete Garden', 'Rooftop', 'Stray Cat', 'Evening Shift'] },
  { title: 'Летняя кухня', artist: 'Разные исполнители', year: 2015, hue: 176, bits: 16, rate: 44100, tracks: ['Варенье', 'Окрошка', 'Дача', 'Гроза', 'Самовар'] },
  { title: 'Radio Silence', artist: 'Orbit Hotel', year: 2023, hue: 290, bits: 24, rate: 96000, tracks: ['Carrier', 'Radio Silence', 'Static', 'Antenna', 'Sign Off'] },
  { title: 'Старые письма', artist: 'Квартал 12', year: 2012, hue: 34, bits: 16, rate: 44100, tracks: ['Конверт', 'Старые письма', 'Почерк', 'Марка'] },
];

function cover(hue: number, index: number): string {
  const a = `hsl(${hue} 46% 46%)`;
  const b = `hsl(${(hue + 28) % 360} 52% 30%)`;
  const c = `hsl(${(hue + 340) % 360} 60% 72%)`;
  const shapes = [
    `<circle cx="150" cy="170" r="92" fill="${c}"/><rect x="0" y="210" width="300" height="90" fill="${b}"/>`,
    `<rect x="48" y="48" width="204" height="204" rx="102" fill="${b}"/><rect x="118" y="0" width="64" height="300" fill="${c}"/>`,
    `<path d="M0 300 L150 60 L300 300 Z" fill="${b}"/><circle cx="222" cy="84" r="34" fill="${c}"/>`,
    `<rect x="36" y="36" width="96" height="228" fill="${c}"/><rect x="168" y="36" width="96" height="228" fill="${b}"/>`,
  ][index % 4];
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 300 300"><rect width="300" height="300" fill="${a}"/>${shapes}</svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

let trackId = 1;
const details: AlbumDetail[] = seeds.map((seed, index) => {
  const artwork = cover(seed.hue, index);
  const tracks: Track[] = seed.tracks.map((title, n) => ({
    id: trackId++,
    path: `D:\\Music\\${seed.artist}\\${seed.title}\\${String(n + 1).padStart(2, '0')} ${title}.flac`,
    title,
    artist: seed.artist === 'Разные исполнители' ? ['Сёстры Орловы', 'Гриша Ветер', 'Двор'][n % 3] : seed.artist,
    album: seed.title,
    album_id: index + 1,
    disc_no: 1,
    track_no: n + 1,
    duration_ms: 152_000 + ((n * 37 + index * 53) % 190) * 1000,
    sample_rate: seed.rate,
    bit_depth: seed.bits,
    codec: 'FLAC',
    artwork,
  }));
  const album: Album = {
    id: index + 1,
    title: seed.title,
    artist: seed.artist,
    year: seed.year,
    track_count: tracks.length,
    duration_ms: tracks.reduce((sum, t) => sum + t.duration_ms, 0),
    artwork,
    color: `hsl(${seed.hue} 46% 46%)`,
  };
  return { album, tracks };
});

const albums = details.map((d) => d.album).sort((a, b) => a.artist.localeCompare(b.artist, 'ru') || (a.year ?? 0) - (b.year ?? 0));
const allTracks = details.flatMap((d) => d.tracks);

function artists(): Artist[] {
  const map = new Map<string, Artist>();
  for (const album of albums) {
    const entry = map.get(album.artist) ?? { name: album.artist, album_count: 0, track_count: 0, artwork: album.artwork };
    entry.album_count += 1;
    entry.track_count += album.track_count;
    map.set(album.artist, entry);
  }
  return [...map.values()];
}

const listeners = new Map<string, Set<(payload: unknown) => void>>();
function emit(event: string, payload: unknown) {
  listeners.get(event)?.forEach((handler) => handler(payload));
}

export function on(event: string, handler: (payload: unknown) => void): () => void {
  if (!listeners.has(event)) listeners.set(event, new Set());
  listeners.get(event)!.add(handler);
  return () => listeners.get(event)?.delete(handler);
}

const player = {
  queue: [] as Track[],
  index: null as number | null,
  status: 'stopped' as Snapshot['status'],
  position: 0,
  volume: 0.8,
  exclusive: true,
  context: '',
};

function snapshot(): Snapshot {
  const track = player.index === null ? null : (player.queue[player.index] ?? null);
  return {
    status: player.status,
    index: player.index,
    track,
    position_ms: player.position,
    duration_ms: track?.duration_ms ?? 0,
    volume: player.volume,
    exclusive: player.exclusive,
    queue_len: player.queue.length,
    context: player.context,
    signal: track
      ? player.exclusive
        ? { device: 'MONITOR L/R (Volt 276)', exclusive: true, bit_perfect: true, source_rate: track.sample_rate ?? 44100, source_bits: track.bit_depth, output_rate: track.sample_rate ?? 44100, output_bits: track.bit_depth, reasons: [] }
        : { device: 'MONITOR L/R (Volt 276)', exclusive: false, bit_perfect: false, source_rate: track.sample_rate ?? 44100, source_bits: track.bit_depth, output_rate: 48000, output_bits: null, reasons: [{ kind: 'shared_mode' }, { kind: 'float' }, { kind: 'sample_rate', from: track.sample_rate ?? 44100, to: 48000 }] }
      : null,
  };
}

setInterval(() => {
  if (player.status !== 'playing' || player.index === null) return;
  player.position += 250;
  const track = player.queue[player.index];
  if (player.position >= track.duration_ms) {
    if (player.index + 1 < player.queue.length) {
      player.index += 1;
      player.position = 0;
    } else {
      player.status = 'stopped';
      player.index = 0;
      player.position = 0;
    }
    emit('player', snapshot());
    return;
  }
  emit('position', { index: player.index, position_ms: player.position });
}, 250);

function start(queue: Track[], index: number, context: string) {
  Object.assign(player, { queue, index, status: 'playing', position: 0, context });
  emit('player', snapshot());
  emit('queue-changed', null);
}

export function call(command: string, args: Record<string, unknown>): unknown {
  switch (command) {
    case 'library_status':
      if (location.hash.includes('empty')) {
        return { roots: [], counts: { tracks: 0, albums: 0, artists: 0 }, scanning: false, suggested_root: 'C:\\Users\\Guest\\Music' };
      }
      return { roots: ['D:\\Music'], counts: { tracks: allTracks.length, albums: albums.length, artists: artists().length }, scanning: false, suggested_root: 'C:\\Users\\Guest\\Music' };
    case 'albums':
      return albums;
    case 'recent_albums':
      return albums.slice(0, args.limit as number);
    case 'album':
      return details.find((d) => d.album.id === args.id) ?? null;
    case 'artists':
      return artists();
    case 'artist_albums':
      return albums.filter((a) => a.artist === args.name);
    case 'tracks':
      return allTracks.slice(args.offset as number, (args.offset as number) + (args.limit as number));
    case 'search': {
      const needle = String(args.text).trim().toLowerCase();
      if (!needle) return { albums: [], artists: [], tracks: [] };
      return {
        albums: albums.filter((a) => `${a.title} ${a.artist}`.toLowerCase().includes(needle)),
        artists: artists().filter((a) => a.name.toLowerCase().includes(needle)),
        tracks: allTracks.filter((t) => `${t.title} ${t.artist} ${t.album}`.toLowerCase().includes(needle)),
      };
    }
    case 'play_album': {
      const detail = details.find((d) => d.album.id === args.id);
      if (detail) start(detail.tracks, args.index as number, detail.album.title);
      return null;
    }
    case 'play_tracks':
      start(allTracks.filter((t) => (args.ids as number[]).includes(t.id)), args.index as number, String(args.context));
      return null;
    case 'toggle_pause':
      if (player.queue.length) player.status = player.status === 'playing' ? 'paused' : 'playing';
      emit('player', snapshot());
      return null;
    case 'next_track':
    case 'previous_track':
    case 'jump_to': {
      if (player.index === null) return null;
      const target = command === 'jump_to' ? (args.index as number) : player.index + (command === 'next_track' ? 1 : -1);
      if (target >= 0 && target < player.queue.length) {
        player.index = target;
        player.position = 0;
        if (player.status === 'stopped') player.status = 'playing';
      }
      emit('player', snapshot());
      return null;
    }
    case 'seek':
      player.position = (args.seconds as number) * 1000;
      emit('position', { index: player.index, position_ms: player.position });
      return null;
    case 'set_volume':
      player.volume = args.volume as number;
      return null;
    case 'player_snapshot':
      return snapshot();
    case 'queue':
      return player.queue;
    case 'output_devices':
      return {
        devices: [
          { id: 'volt', name: 'MONITOR L/R (Volt 276)', is_default: true },
          { id: 'realtek', name: 'Realtek Digital Output', is_default: false },
        ],
        device_id: null,
        exclusive: player.exclusive,
      };
    case 'set_output':
      player.exclusive = args.exclusive as boolean;
      emit('player', snapshot());
      return null;
    default:
      return null;
  }
}
