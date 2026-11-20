// Small building blocks and text helpers used on every screen.

import { useEffect, useState, type CSSProperties } from 'react';
import { coverUrl, type Signal, type Track } from './api';
import { useStore } from './store';

/** Russian plural: pick the form for 1, 2 to 4, and everything else. */
export function plural(count: number, one: string, few: string, many: string): string {
  const tens = count % 100;
  const units = count % 10;
  if (tens >= 11 && tens <= 14) return many;
  if (units === 1) return one;
  if (units >= 2 && units <= 4) return few;
  return many;
}

const number = new Intl.NumberFormat('ru-RU');

export const count = {
  tracks: (n: number) => `${number.format(n)} ${plural(n, 'трек', 'трека', 'треков')}`,
  albums: (n: number) => `${number.format(n)} ${plural(n, 'альбом', 'альбома', 'альбомов')}`,
  artists: (n: number) => `${number.format(n)} ${plural(n, 'исполнитель', 'исполнителя', 'исполнителей')}`,
};

export function formatNumber(n: number): string {
  return number.format(n);
}

/** Track time as m:ss, or h:mm:ss for long recordings. */
export function clock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${s}` : `${m}:${s}`;
}

/** Length of an album in words: 54 мин, 1 ч 12 мин. */
export function length(ms: number): string {
  const minutes = Math.max(1, Math.round(ms / 60000));
  if (minutes < 60) return `${minutes} мин`;
  return `${Math.floor(minutes / 60)} ч ${minutes % 60} мин`;
}

/** 44100 becomes «44,1 кГц», 96000 becomes «96 кГц». */
export function rate(hz: number): string {
  const khz = hz / 1000;
  return `${Number.isInteger(khz) ? khz : khz.toFixed(1).replace('.', ',')} кГц`;
}

/** Format of a file in one short line: FLAC · 24 бит · 96 кГц. */
export function quality(track: Pick<Track, 'codec' | 'bit_depth' | 'sample_rate'>): string {
  return [track.codec, track.bit_depth ? `${track.bit_depth} бит` : null, track.sample_rate ? rate(track.sample_rate) : null]
    .filter(Boolean)
    .join(' · ');
}

export const unknownArtist = 'Неизвестный исполнитель';

/** Why playback is not bit-perfect, one phrase per reason. */
export function reasons(signal: Signal): string[] {
  return signal.reasons.map((reason) => {
    switch (reason.kind) {
      case 'lossy_source':
        return 'формат файла с потерями';
      case 'shared_mode':
        return 'звук идёт через микшер Windows';
      case 'sample_rate':
        return `частота меняется с ${rate(reason.from)} на ${rate(reason.to)}`;
      case 'channels':
        return `число каналов меняется с ${reason.from} на ${reason.to}`;
      case 'float':
        return 'семплы переводятся в числа с плавающей точкой';
      case 'bit_depth':
        return `разрядность снижается с ${reason.from} до ${reason.to} бит`;
    }
  });
}

/** The two bars of the VYRA mark. `splash` plays the opening from a pause sign into a V. */
export function Mark({ size = 24, splash = false }: { size?: number; splash?: boolean }) {
  return (
    <svg className={splash ? 'mark mark--splash' : 'mark'} width={size} height={size} viewBox="0 0 96 96" aria-hidden="true">
      <rect className="mark__left" x="37" y="12" width="22" height="72" rx="11" />
      <rect className="mark__right" x="37" y="12" width="22" height="72" rx="11" />
    </svg>
  );
}

/** A square cover. Without an image it shows the mark on a quiet surface. */
export function Cover({ artwork, size = 400, className = '', style }: { artwork: string | null; size?: 400 | 1000; className?: string; style?: CSSProperties }) {
  const url = coverUrl(artwork, size);
  const [failed, setFailed] = useState(false);
  useEffect(() => setFailed(false), [url]);
  return (
    <div className={`cover ${className}`} style={style}>
      {url && !failed ? (
        <img src={url} alt="" loading="lazy" decoding="async" draggable={false} onError={() => setFailed(true)} />
      ) : (
        <Mark size={40} />
      )}
    </div>
  );
}

/** Position of the current track, moving smoothly between reports of the core. */
export function usePosition(): number {
  const playing = useStore((s) => s.player.status === 'playing');
  const base = useStore((s) => s.positionMs);
  const at = useStore((s) => s.positionAt);
  const duration = useStore((s) => s.player.duration_ms);
  const [now, setNow] = useState(() => performance.now());

  useEffect(() => {
    if (!playing) return;
    let frame = requestAnimationFrame(function tick() {
      setNow(performance.now());
      frame = requestAnimationFrame(tick);
    });
    return () => cancelAnimationFrame(frame);
  }, [playing]);

  const position = playing ? base + Math.max(0, now - at) : base;
  return duration ? Math.min(position, duration) : position;
}

/** Three small bars that move while a track plays and rest when it is paused. */
export function Playing({ active }: { active: boolean }) {
  return (
    <span className={active ? 'playing playing--active' : 'playing'} aria-hidden="true">
      <i />
      <i />
      <i />
    </span>
  );
}
