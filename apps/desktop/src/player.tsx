// Everything about playback: the bar at the bottom, the full screen view and the queue.

import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react';
import { ChevronDown, ListMusic, Pause, Play, SkipBack, SkipForward, Volume1, Volume2, VolumeX, X } from 'lucide-react';
import { call, type Signal } from './api';
import { WindowButtons } from './shell';
import { useStore } from './store';
import { Cover, Playing, clock, quality, rate, reasons, unknownArtist, usePosition } from './ui';

type SliderProps = {
  /** Current value from 0 to 1. */
  value: number;
  label: string;
  disabled?: boolean;
  /** Called while the handle moves. */
  onChange?: (value: number) => void;
  /** Called once when the user lets go. */
  onCommit?: (value: number) => void;
  /** Step for the arrow keys, as a share of the whole range. */
  step?: number;
};

/** A thin horizontal slider. Only transforms change while it moves. */
function Slider({ value, label, disabled, onChange, onCommit, step = 0.05 }: SliderProps) {
  const element = useRef<HTMLDivElement>(null);
  const [dragging, setDragging] = useState<number | null>(null);
  const shown = Math.min(1, Math.max(0, dragging ?? value));

  const fromPointer = (event: PointerEvent) => {
    const box = element.current!.getBoundingClientRect();
    return Math.min(1, Math.max(0, (event.clientX - box.left) / box.width));
  };

  const down = (event: PointerEvent<HTMLDivElement>) => {
    if (disabled) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    const next = fromPointer(event);
    setDragging(next);
    onChange?.(next);
  };
  const move = (event: PointerEvent<HTMLDivElement>) => {
    if (dragging === null) return;
    const next = fromPointer(event);
    setDragging(next);
    onChange?.(next);
  };
  const up = () => {
    if (dragging === null) return;
    onCommit?.(dragging);
    setDragging(null);
  };
  const key = (event: KeyboardEvent) => {
    if (disabled) return;
    const delta = event.key === 'ArrowRight' ? step : event.key === 'ArrowLeft' ? -step : 0;
    if (!delta) return;
    event.preventDefault();
    const next = Math.min(1, Math.max(0, value + delta));
    onChange?.(next);
    onCommit?.(next);
  };

  return (
    <div
      ref={element}
      className={`slider${dragging !== null ? ' slider--dragging' : ''}${disabled ? ' slider--disabled' : ''}`}
      role="slider"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(shown * 100)}
      aria-disabled={disabled}
      tabIndex={disabled ? -1 : 0}
      onPointerDown={down}
      onPointerMove={move}
      onPointerUp={up}
      onPointerCancel={up}
      onKeyDown={key}
    >
      <div className="slider__rail">
        <div className="slider__fill" style={{ transform: `scaleX(${shown})` }} />
      </div>
      <div className="slider__thumb" style={{ transform: `translateX(${shown * (element.current?.clientWidth ?? 0)}px)` }} />
    </div>
  );
}

function Scrubber() {
  const duration = useStore((s) => s.player.duration_ms);
  const active = useStore((s) => s.player.status !== 'stopped' && s.player.track !== null);
  const position = usePosition();
  const [preview, setPreview] = useState<number | null>(null);
  const shown = preview ?? position;

  return (
    <div className="scrub mono">
      <span className="scrub__time">{clock(shown)}</span>
      <Slider
        label="Позиция в треке"
        value={duration ? shown / duration : 0}
        disabled={!active || !duration}
        step={duration ? 5000 / duration : 0.05}
        onChange={(value) => setPreview(value * duration)}
        onCommit={(value) => {
          useStore.setState({ positionMs: value * duration, positionAt: performance.now() });
          call('seek', { seconds: (value * duration) / 1000 });
          setPreview(null);
        }}
      />
      <span className="scrub__time">{clock(duration)}</span>
    </div>
  );
}

function Transport() {
  const status = useStore((s) => s.player.status);
  const hasQueue = useStore((s) => s.player.queue_len > 0);
  const index = useStore((s) => s.player.index ?? 0);
  const last = useStore((s) => s.player.queue_len - 1);
  return (
    <div className="transport">
      <button className="icon-button" onClick={() => call('previous_track')} disabled={!hasQueue} aria-label="Предыдущий трек" title="Предыдущий трек">
        <SkipBack size={18} strokeWidth={1.5} fill="currentColor" />
      </button>
      <button className="transport__play" onClick={() => call('toggle_pause')} disabled={!hasQueue} aria-label={status === 'playing' ? 'Пауза' : 'Играть'} title={status === 'playing' ? 'Пауза' : 'Играть'}>
        {status === 'playing' ? <Pause size={18} strokeWidth={1.5} fill="currentColor" /> : <Play size={18} strokeWidth={1.5} fill="currentColor" style={{ marginLeft: 2 }} />}
      </button>
      <button className="icon-button" onClick={() => call('next_track')} disabled={!hasQueue || index >= last} aria-label="Следующий трек" title="Следующий трек">
        <SkipForward size={18} strokeWidth={1.5} fill="currentColor" />
      </button>
    </div>
  );
}

function signalSummary(signal: Signal): string {
  if (signal.bit_perfect) return `Bit-perfect · ${rate(signal.source_rate)}`;
  if (!signal.exclusive) return `Общий режим · ${rate(signal.output_rate)}`;
  return `Монопольный · ${rate(signal.output_rate)}`;
}

/** The signal path line: the detail only a player with this engine can show. */
function SignalLine() {
  const signal = useStore((s) => s.player.signal);
  const playing = useStore((s) => s.player.status !== 'stopped');
  const open = useStore((s) => s.openNowPlaying);
  if (!signal || !playing) return null;
  const why = signal.bit_perfect ? 'Устройство получает семплы файла без изменений' : `Не bit-perfect: ${reasons(signal).join('; ')}`;
  return (
    <button className={signal.bit_perfect ? 'signal signal--perfect mono' : 'signal mono'} onClick={() => open(true)} title={why}>
      <span className="signal__dot" />
      <span className="ellipsis">{signalSummary(signal)}</span>
    </button>
  );
}

function Volume() {
  const volume = useStore((s) => s.player.volume);
  const exclusive = useStore((s) => s.player.exclusive);
  const [value, setValue] = useState(volume);
  useEffect(() => setValue(volume), [volume]);

  const Icon = value === 0 ? VolumeX : value < 0.5 ? Volume1 : Volume2;
  const apply = (next: number) => {
    setValue(next);
    // The curve follows hearing: the lower half of the slider covers the quiet range.
    call('set_volume', { volume: next * next });
  };

  return (
    <div className="volume" title={exclusive ? 'В монопольном режиме громкость регулируется на устройстве' : 'Громкость'}>
      <Icon size={18} strokeWidth={1.5} color="var(--t3)" />
      <Slider label="Громкость" value={exclusive ? 1 : Math.sqrt(value)} disabled={exclusive} onChange={apply} />
    </div>
  );
}

export function PlayerBar() {
  const track = useStore((s) => s.player.track);
  const open = useStore((s) => s.openNowPlaying);
  const nowPlayingOpen = useStore((s) => s.nowPlayingOpen);
  const queueOpen = useStore((s) => s.queueOpen);
  const toggleQueue = useStore((s) => s.toggleQueue);

  return (
    <footer className="player">
      {track ? (
        <button className="player__track" onClick={() => open(!nowPlayingOpen)} aria-label="Открыть экран воспроизведения">
          <Cover artwork={track.artwork} className="player__cover" />
          <span style={{ minWidth: 0 }}>
            <span className="player__title ellipsis" style={{ display: 'block' }}>
              {track.title}
            </span>
            <span className="player__artist ellipsis" style={{ display: 'block' }}>
              {track.artist || unknownArtist}
            </span>
          </span>
        </button>
      ) : (
        <div className="player__idle">Выберите альбом или трек</div>
      )}

      <div className="player__center">
        <Transport />
        <Scrubber />
      </div>

      <div className="player__side">
        <SignalLine />
        <Volume />
        <button className={queueOpen ? 'icon-button icon-button--on' : 'icon-button'} onClick={toggleQueue} aria-label="Очередь" aria-pressed={queueOpen} title="Очередь">
          <ListMusic size={18} strokeWidth={1.5} />
        </button>
      </div>
    </footer>
  );
}

export function QueuePanel() {
  const queue = useStore((s) => s.queue);
  const index = useStore((s) => s.player.index);
  const playing = useStore((s) => s.player.status === 'playing');
  const context = useStore((s) => s.player.context);
  const toggle = useStore((s) => s.toggleQueue);
  const current = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    current.current?.scrollIntoView({ block: 'nearest' });
  }, [index]);

  return (
    <aside className="queue">
      <div className="queue__head">
        <span className="queue__title">Очередь</span>
        <button className="icon-button" onClick={toggle} aria-label="Закрыть очередь" title="Закрыть">
          <X size={18} strokeWidth={1.5} />
        </button>
      </div>
      {queue.length ? (
        <>
          <div className="queue__context ellipsis">{context}</div>
          <div className="queue__list">
            {queue.map((track, position) => (
              <button key={`${track.id}-${position}`} ref={position === index ? current : undefined} className={position === index ? 'row row--current' : 'row'} onClick={() => call('jump_to', { index: position })}>
                <span className="row__no">{position === index ? <Playing active={playing} /> : position + 1}</span>
                <span className="row__title">
                  <span className="row__name ellipsis" style={{ display: 'block' }}>
                    {track.title}
                  </span>
                  <span className="row__sub ellipsis" style={{ display: 'block' }}>
                    {track.artist || unknownArtist}
                  </span>
                </span>
                <span className="row__time">{clock(track.duration_ms)}</span>
              </button>
            ))}
          </div>
        </>
      ) : (
        <div className="empty">Очередь пуста</div>
      )}
    </aside>
  );
}

function SignalCard({ signal }: { signal: Signal }) {
  const track = useStore((s) => s.player.track);
  const output = [rate(signal.output_rate), signal.output_bits ? `${signal.output_bits} бит` : 'с плавающей точкой'].join(' · ');
  return (
    <div className="path">
      <div className="path__row">
        <span className="path__key">Файл</span>
        <span className="path__value">{track ? quality(track) : rate(signal.source_rate)}</span>
      </div>
      <div className="path__row">
        <span className="path__key">Устройство</span>
        <span className="path__value">
          {signal.device}
          <br />
          {signal.exclusive ? 'Монопольный режим' : 'Общий режим'} · {output}
        </span>
      </div>
      <div className="path__row">
        <span className="path__key">Путь сигнала</span>
        {signal.bit_perfect ? (
          <span className="path__value path__value--perfect">Bit-perfect: устройство получает семплы файла без изменений</span>
        ) : (
          <span className="path__value">Не bit-perfect: {reasons(signal).join('; ')}</span>
        )}
      </div>
    </div>
  );
}

export function NowPlaying() {
  const track = useStore((s) => s.player.track);
  const signal = useStore((s) => s.player.signal);
  const context = useStore((s) => s.player.context);
  const index = useStore((s) => s.player.index);
  const queue = useStore((s) => s.queue);
  const open = useStore((s) => s.openNowPlaying);
  const go = useStore((s) => s.go);
  const [tint, setTint] = useState('#141415');

  // The cover lends its colour to the top of the screen, as a faint tint of the grey.
  const albumId = track?.album_id ?? null;
  useEffect(() => {
    let cancelled = false;
    if (albumId === null) return;
    call<{ album: { color: string | null } } | null>('album', { id: albumId }).then((detail) => {
      if (!cancelled) setTint(detail?.album.color ?? '#141415');
    });
    return () => {
      cancelled = true;
    };
  }, [albumId]);

  const close = useCallback(() => open(false), [open]);
  useEffect(() => {
    const onKey = (event: globalThis.KeyboardEvent) => event.key === 'Escape' && close();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [close]);

  if (!track) return null;
  const next = index === null ? [] : queue.slice(index + 1, index + 4);

  return (
    <section className="now" aria-label="Сейчас играет">
      <div className="now__tint" style={{ backgroundColor: tint }} />
      <div className="now__bar">
        <button className="icon-button" onClick={close} aria-label="Свернуть" title="Свернуть">
          <ChevronDown size={18} strokeWidth={1.5} />
        </button>
        <div className="titlebar__drag" data-tauri-drag-region style={{ height: '100%' }} />
        <WindowButtons />
      </div>

      <div className="now__body">
        <Cover artwork={track.artwork} size={1000} className="now__cover" />
        <div className="now__info">
          {context && context !== track.album ? <div className="now__context">{context}</div> : null}
          <h1 className="now__title">{track.title}</h1>
          <div className="now__artist">{track.artist || unknownArtist}</div>
          {track.album && track.album_id !== null ? (
            <button
              className="now__album"
              onClick={() => {
                go({ name: 'album', id: track.album_id! });
                close();
              }}
            >
              {track.album}
            </button>
          ) : null}

          {signal ? <SignalCard signal={signal} /> : null}

          {next.length ? (
            <div className="next">
              <div className="next__label">Далее</div>
              {next.map((item, offset) => (
                <button key={`${item.id}-${offset}`} className="row" onClick={() => call('jump_to', { index: index! + 1 + offset })}>
                  <span className="ellipsis">{item.title}</span>
                  <span className="row__time">{clock(item.duration_ms)}</span>
                </button>
              ))}
            </div>
          ) : null}
        </div>
      </div>
    </section>
  );
}
