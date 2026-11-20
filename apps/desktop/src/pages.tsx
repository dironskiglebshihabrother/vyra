// Library screens: albums, one album, artists, tracks, search, settings and first run.

import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { ChevronLeft, FolderPlus, Play, RefreshCw, Trash2 } from 'lucide-react';
import { call, type Album, type AlbumDetail, type Artist, type OutputState, type SearchResults, type Track } from './api';
import { useStore } from './store';
import { Cover, Mark, Playing, clock, count, length, quality, unknownArtist } from './ui';

/** Loads data from the core and reloads it when the library changes. */
function useLibrary<T>(load: () => Promise<T>, deps: unknown[]): T | null {
  const version = useStore((s) => s.libraryVersion);
  const [data, setData] = useState<T | null>(null);
  useEffect(() => {
    let cancelled = false;
    load().then((value) => !cancelled && setData(value));
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [version, ...deps]);
  return data;
}

function Page({ title, meta, children }: { title: string; meta?: string; children: ReactNode }) {
  return (
    <div className="page">
      <div className="page__head">
        <h1 className="page__title">{title}</h1>
        {meta ? <span className="page__meta">{meta}</span> : null}
      </div>
      {children}
    </div>
  );
}

function Back() {
  const back = useStore((s) => s.back);
  return (
    <button className="icon-button back" onClick={back} aria-label="Назад" title="Назад">
      <ChevronLeft size={18} strokeWidth={1.5} />
    </button>
  );
}

function AlbumTile({ album }: { album: Album }) {
  const go = useStore((s) => s.go);
  return (
    <div className="tile">
      <div className="tile__art">
        <button onClick={() => go({ name: 'album', id: album.id })} aria-label={`${album.title}, ${album.artist || unknownArtist}`} style={{ display: 'block', width: '100%', borderRadius: 8 }}>
          <Cover artwork={album.artwork} />
        </button>
        <button className="tile__play" onClick={() => call('play_album', { id: album.id, index: 0 })} aria-label={`Слушать ${album.title}`} title="Слушать">
          <Play size={16} strokeWidth={1.5} fill="currentColor" />
        </button>
      </div>
      <div className="tile__title ellipsis">{album.title}</div>
      <div className="tile__sub ellipsis">{album.artist || unknownArtist}</div>
    </div>
  );
}

function AlbumGrid({ albums }: { albums: Album[] }) {
  return (
    <div className="grid">
      {albums.map((album) => (
        <AlbumTile key={album.id} album={album} />
      ))}
    </div>
  );
}

export function Albums() {
  const albums = useLibrary(() => call<Album[]>('albums'), []);
  const scanning = useStore((s) => s.status?.scanning ?? false);
  if (!albums) return null;
  return (
    <Page title="Альбомы" meta={albums.length ? count.albums(albums.length) : undefined}>
      {albums.length ? (
        <AlbumGrid albums={albums} />
      ) : (
        <div className="empty">{scanning ? 'Читаю папки с музыкой, альбомы появятся здесь' : 'В выбранных папках нет музыки, которую VYRA умеет играть'}</div>
      )}
    </Page>
  );
}

function TrackNumber({ track, index }: { track: Track; index: number }) {
  const current = useStore((s) => s.player.track?.id === track.id);
  const playing = useStore((s) => s.player.status === 'playing');
  return <span className="row__no">{current ? <Playing active={playing} /> : (track.track_no ?? index + 1)}</span>;
}

export function AlbumPage({ id }: { id: number }) {
  const detail = useLibrary(() => call<AlbumDetail | null>('album', { id }), [id]);
  const go = useStore((s) => s.go);
  const currentId = useStore((s) => s.player.track?.id);
  if (!detail) return null;

  const { album, tracks } = detail;
  const discs = new Set(tracks.map((t) => t.disc_no ?? 1));
  const first = tracks[0];
  const sameQuality = first && tracks.every((t) => quality(t) === quality(first));
  const meta = [album.year, count.tracks(album.track_count), length(album.duration_ms), sameQuality ? quality(first) : null].filter(Boolean).join(' · ');

  return (
    <div className="page">
      <Back />
      <div className="hero">
        <Cover artwork={album.artwork} size={1000} className="hero__cover" />
        <div className="hero__body">
          <div className="hero__kind">Альбом</div>
          <h1 className="hero__title">{album.title}</h1>
          {album.artist ? (
            <button className="hero__artist" onClick={() => go({ name: 'artist', artist: album.artist })}>
              {album.artist}
            </button>
          ) : (
            <div className="hero__artist">{unknownArtist}</div>
          )}
          <div className="hero__meta">{meta}</div>
          <div className="hero__actions">
            <button className="button button--primary" onClick={() => call('play_album', { id, index: 0 })}>
              <Play size={16} strokeWidth={1.5} fill="currentColor" />
              Слушать
            </button>
          </div>
        </div>
      </div>

      <div className="tracks">
        {tracks.map((track, index) => {
          const disc = track.disc_no ?? 1;
          const newDisc = discs.size > 1 && (index === 0 || (tracks[index - 1].disc_no ?? 1) !== disc);
          return (
            <div key={track.id}>
              {newDisc ? <div className="disc">Диск {disc}</div> : null}
              <button className={track.id === currentId ? 'row row--current' : 'row'} onClick={() => call('play_album', { id, index })}>
                <TrackNumber track={track} index={index} />
                <span className="row__title">
                  <span className="row__name ellipsis" style={{ display: 'block' }}>
                    {track.title}
                  </span>
                  {track.artist && track.artist !== album.artist ? <span className="row__sub ellipsis" style={{ display: 'block' }}>{track.artist}</span> : null}
                </span>
                <span className="row__time">{clock(track.duration_ms)}</span>
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function ArtistRow({ artist }: { artist: Artist }) {
  const go = useStore((s) => s.go);
  return (
    <button className="artist" onClick={() => go({ name: 'artist', artist: artist.name })}>
      <Cover artwork={artist.artwork} className="artist__art" />
      <span className="artist__name ellipsis">{artist.name || unknownArtist}</span>
      <span className="artist__meta">
        {count.albums(artist.album_count)} · {count.tracks(artist.track_count)}
      </span>
    </button>
  );
}

export function Artists() {
  const artists = useLibrary(() => call<Artist[]>('artists'), []);
  if (!artists) return null;
  return (
    <Page title="Исполнители" meta={artists.length ? count.artists(artists.length) : undefined}>
      {artists.length ? artists.map((artist) => <ArtistRow key={artist.name} artist={artist} />) : <div className="empty">Исполнители появятся после сканирования</div>}
    </Page>
  );
}

export function ArtistPage({ artist }: { artist: string }) {
  const albums = useLibrary(() => call<Album[]>('artist_albums', { name: artist }), [artist]);
  if (!albums) return null;
  const tracks = albums.reduce((sum, a) => sum + a.track_count, 0);
  return (
    <div className="page">
      <Back />
      <div className="page__head">
        <h1 className="page__title">{artist || unknownArtist}</h1>
        <span className="page__meta">
          {count.albums(albums.length)} · {count.tracks(tracks)}
        </span>
      </div>
      <AlbumGrid albums={albums} />
    </div>
  );
}

function WideRow({ track, onPlay }: { track: Track; onPlay: () => void }) {
  const current = useStore((s) => s.player.track?.id === track.id);
  const playing = useStore((s) => s.player.status === 'playing');
  return (
    <button className={current ? 'row row--wide row--current' : 'row row--wide'} onClick={onPlay}>
      {current ? (
        <span className="row__no" style={{ justifyItems: 'center' }}>
          <Playing active={playing} />
        </span>
      ) : (
        <Cover artwork={track.artwork} className="row__thumb" />
      )}
      <span className="row__name ellipsis">{track.title}</span>
      <span className="row__cell ellipsis">{track.artist || unknownArtist}</span>
      <span className="row__cell ellipsis">{track.album}</span>
      <span className="row__time">{clock(track.duration_ms)}</span>
    </button>
  );
}

const PAGE = 400;

export function Tracks() {
  const version = useStore((s) => s.libraryVersion);
  const total = useStore((s) => s.status?.counts.tracks ?? 0);
  const [tracks, setTracks] = useState<Track[]>([]);
  const parent = useRef<HTMLDivElement>(null);

  // The list arrives page by page, so the first rows show at once on a large library.
  useEffect(() => {
    let cancelled = false;
    setTracks([]);
    (async () => {
      for (let offset = 0; !cancelled; offset += PAGE) {
        const page = await call<Track[]>('tracks', { offset, limit: PAGE });
        if (cancelled || !page.length) break;
        setTracks((loaded) => (offset === 0 ? page : [...loaded, ...page]));
        if (page.length < PAGE) break;
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [version]);

  const virtual = useVirtualizer({
    count: tracks.length,
    getScrollElement: () => parent.current?.closest('.content') as HTMLElement | null,
    estimateSize: () => 46,
    overscan: 12,
    scrollMargin: parent.current?.offsetTop ?? 0,
  });

  // A track starts a queue of the list from that point on, as the context rules say.
  const play = (index: number) => {
    const ids = tracks.slice(index, index + 500).map((t) => t.id);
    call('play_tracks', { ids, index: 0, context: 'Все треки' });
  };

  return (
    <Page title="Треки" meta={total ? count.tracks(total) : undefined}>
      {tracks.length ? (
        <div ref={parent} style={{ height: virtual.getTotalSize(), position: 'relative' }}>
          {virtual.getVirtualItems().map((item) => (
            <div key={tracks[item.index].id} style={{ position: 'absolute', top: 0, left: 0, right: 0, transform: `translateY(${item.start - virtual.options.scrollMargin}px)` }}>
              <WideRow track={tracks[item.index]} onPlay={() => play(item.index)} />
            </div>
          ))}
        </div>
      ) : (
        <div className="empty">Треки появятся после сканирования</div>
      )}
    </Page>
  );
}

export function SearchPage({ text }: { text: string }) {
  const [results, setResults] = useState<SearchResults | null>(null);
  const version = useStore((s) => s.libraryVersion);

  useEffect(() => {
    let cancelled = false;
    const timer = setTimeout(() => {
      call<SearchResults>('search', { text }).then((value) => !cancelled && setResults(value));
    }, 120);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [text, version]);

  if (!results) return null;
  const nothing = !results.albums.length && !results.artists.length && !results.tracks.length;

  return (
    <Page title="Поиск" meta={`«${text.trim()}»`}>
      {nothing ? <div className="empty">Ничего не нашлось</div> : null}
      {results.artists.length ? (
        <>
          <h2 className="section-title">Исполнители</h2>
          {results.artists.slice(0, 6).map((artist) => (
            <ArtistRow key={artist.name} artist={artist} />
          ))}
        </>
      ) : null}
      {results.albums.length ? (
        <>
          <h2 className="section-title">Альбомы</h2>
          <AlbumGrid albums={results.albums} />
        </>
      ) : null}
      {results.tracks.length ? (
        <>
          <h2 className="section-title">Треки</h2>
          <div className="tracks">
            {results.tracks.map((track) => (
              // A track found by search plays alone: its neighbours share nothing but the query.
              <WideRow key={track.id} track={track} onPlay={() => call('play_tracks', { ids: [track.id], index: 0, context: 'Поиск' })} />
            ))}
          </div>
        </>
      ) : null}
    </Page>
  );
}

export function SettingsPage() {
  const status = useStore((s) => s.status);
  const refresh = useStore((s) => s.refreshStatus);
  const exclusive = useStore((s) => s.player.exclusive);
  const [output, setOutput] = useState<OutputState | null>(null);

  useEffect(() => {
    call<OutputState>('output_devices').then(setOutput);
  }, []);

  const chosen = output?.device_id ?? '';
  const setDevice = (deviceId: string | null, mode: boolean) => {
    setOutput((o) => (o ? { ...o, device_id: deviceId, exclusive: mode } : o));
    call('set_output', { deviceId, exclusive: mode });
  };

  return (
    <div className="page settings">
      <div className="page__head">
        <h1 className="page__title">Настройки</h1>
      </div>

      <h2 className="section-title">Папки с музыкой</h2>
      <div className="card">
        {status?.roots.length ? (
          status.roots.map((root) => (
            <div className="card__row" key={root}>
              <span className="card__path mono ellipsis">{root}</span>
              <button className="icon-button" onClick={() => call('remove_music_folder', { path: root }).then(refresh)} aria-label={`Убрать папку ${root}`} title="Убрать из медиатеки">
                <Trash2 size={16} strokeWidth={1.5} />
              </button>
            </div>
          ))
        ) : (
          <div className="card__row">
            <span className="card__hint">Папок пока нет</span>
          </div>
        )}
      </div>
      <div className="settings__actions">
        <button className="button button--secondary" onClick={() => call('add_music_folder', { path: null })}>
          <FolderPlus size={16} strokeWidth={1.5} />
          Добавить папку
        </button>
        <button className="button button--secondary" onClick={() => call('rescan')} disabled={status?.scanning}>
          <RefreshCw size={16} strokeWidth={1.5} />
          Обновить медиатеку
        </button>
      </div>
      <p className="card__hint" style={{ marginTop: 12 }}>
        Убрать папку означает убрать её из медиатеки. Файлы на диске остаются.
      </p>

      <h2 className="section-title">Вывод звука</h2>
      <div className="card">
        <div className="card__row">
          <div className="card__label">Устройство</div>
          <select className="select" value={chosen} onChange={(event) => setDevice(event.target.value || null, output?.exclusive ?? exclusive)}>
            <option value="">Как в Windows</option>
            {output?.devices.map((device) => (
              <option key={device.id} value={device.id}>
                {device.name}
              </option>
            ))}
          </select>
        </div>
        <div className="card__row">
          <div className="card__label">
            Режим
            <div className="card__hint">
              {exclusive
                ? 'Устройство открывается в формате трека. Другие программы в это время звук не получают, громкость регулируется на самом устройстве.'
                : 'Звук идёт через микшер Windows вместе с другими программами.'}
            </div>
          </div>
          <div className="segmented">
            <button aria-pressed={!exclusive} onClick={() => setDevice(output?.device_id ?? null, false)}>
              Общий
            </button>
            <button aria-pressed={exclusive} onClick={() => setDevice(output?.device_id ?? null, true)}>
              Монопольный
            </button>
          </div>
        </div>
      </div>

      <h2 className="section-title">О программе</h2>
      <div className="card">
        <div className="card__row">
          <div className="card__label">VYRA 0.1</div>
          <span className="card__hint">Лицензия GPL-3.0</span>
        </div>
      </div>
    </div>
  );
}

/** First run: no music folder yet. */
export function Welcome() {
  const suggested = useStore((s) => s.status?.suggested_root ?? null);
  const [busy, setBusy] = useState(false);
  const add = (path: string | null) => {
    setBusy(true);
    call('add_music_folder', { path }).finally(() => setBusy(false));
  };
  const name = useMemo(() => suggested?.split(/[\\/]/).filter(Boolean).pop() ?? null, [suggested]);

  return (
    <div className="welcome">
      <div className="welcome__box">
        <Mark size={72} />
        <h1 className="welcome__title">Добавьте папку с музыкой</h1>
        <p className="welcome__text">VYRA прочитает теги и обложки. Сами файлы остаются на месте и не меняются.</p>
        <div className="welcome__actions">
          <button className="button button--primary" onClick={() => add(null)} disabled={busy}>
            <FolderPlus size={16} strokeWidth={1.5} />
            Выбрать папку
          </button>
          {suggested ? (
            <button className="button button--secondary" onClick={() => add(suggested)} disabled={busy} title={suggested}>
              Папка «{name}»
            </button>
          ) : null}
        </div>
      </div>
    </div>
  );
}
