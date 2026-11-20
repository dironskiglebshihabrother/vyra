// The window frame: title bar with our own buttons, the sidebar, the opening and toasts.

import { useEffect, useRef, useState } from 'react';
import { Disc3, ListMusic, Search, Settings, Users } from 'lucide-react';
import { inApp } from './api';
import { useStore, type Route } from './store';
import { Mark, formatNumber } from './ui';

/** Minimise, maximise and close, drawn by the application instead of Windows. */
export function WindowButtons() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!inApp) return;
    let stop: (() => void) | undefined;
    let cancelled = false;
    import('@tauri-apps/api/window').then(({ getCurrentWindow }) => {
      const current = getCurrentWindow();
      const update = () => current.isMaximized().then((value) => !cancelled && setMaximized(value));
      update();
      current.onResized(update).then((unlisten) => (cancelled ? unlisten() : (stop = unlisten)));
    });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, []);

  const act = (action: 'minimize' | 'toggleMaximize' | 'close') => async () => {
    if (!inApp) return;
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    await getCurrentWindow()[action]();
  };

  return (
    <div className="window-buttons">
      <button onClick={act('minimize')} aria-label="Свернуть" title="Свернуть">
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M0 5h10" stroke="currentColor" />
        </svg>
      </button>
      <button onClick={act('toggleMaximize')} aria-label={maximized ? 'Свернуть в окно' : 'Развернуть'} title={maximized ? 'Свернуть в окно' : 'Развернуть'}>
        {maximized ? (
          <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
            <path d="M2.5 2.5v-2h7v7h-2M.5 2.5h7v7h-7z" stroke="currentColor" />
          </svg>
        ) : (
          <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
            <path d="M.5.5h9v9h-9z" stroke="currentColor" />
          </svg>
        )}
      </button>
      <button className="close" onClick={act('close')} aria-label="Закрыть" title="Закрыть">
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M0 0l10 10M10 0L0 10" stroke="currentColor" />
        </svg>
      </button>
    </div>
  );
}

export function TitleBar() {
  return (
    <div className="titlebar">
      <div className="titlebar__drag" data-tauri-drag-region />
      <WindowButtons />
    </div>
  );
}

/** Plays once at start: the two bars stand as a pause sign and lean into the V. */
export function Splash() {
  const [done, setDone] = useState(false);
  useEffect(() => {
    const timer = setTimeout(() => setDone(true), 1400);
    return () => clearTimeout(timer);
  }, []);
  if (done) return null;
  return (
    <div className="splash">
      <Mark size={88} splash />
    </div>
  );
}

const sections: { route: Route; label: string; icon: typeof Disc3 }[] = [
  { route: { name: 'albums' }, label: 'Альбомы', icon: Disc3 },
  { route: { name: 'artists' }, label: 'Исполнители', icon: Users },
  { route: { name: 'tracks' }, label: 'Треки', icon: ListMusic },
];

function isActive(current: Route, item: Route): boolean {
  if (item.name === 'albums') return current.name === 'albums' || current.name === 'album';
  if (item.name === 'artists') return current.name === 'artists' || current.name === 'artist';
  return current.name === item.name;
}

function ScanStatus() {
  const scan = useStore((s) => s.scan);
  const scanning = useStore((s) => s.status?.scanning ?? false);
  if (!scanning || !scan || scan.phase === 'done') return null;

  let text = '';
  let progress = 0;
  if (scan.phase === 'listing') {
    text = `Ищу файлы: ${formatNumber(scan.files)}`;
  } else if (scan.phase === 'reading') {
    text = `Читаю теги: ${formatNumber(scan.done)} из ${formatNumber(scan.total)}`;
    progress = scan.total ? scan.done / scan.total : 0;
  } else {
    text = `Готовлю обложки: ${formatNumber(scan.done)} из ${formatNumber(scan.total)}`;
    progress = scan.total ? scan.done / scan.total : 0;
  }
  return (
    <div className="scan" role="status">
      {text}
      <div className="scan__bar">
        <i style={{ transform: `scaleX(${progress})` }} />
      </div>
    </div>
  );
}

export function Sidebar() {
  const route = useStore((s) => s.route);
  const go = useStore((s) => s.go);
  const setSearch = useStore((s) => s.setSearch);
  const input = useRef<HTMLInputElement>(null);
  const [text, setText] = useState('');

  // The field empties when the user leaves the search page by other means.
  useEffect(() => {
    if (route.name !== 'search') setText('');
  }, [route.name]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'f') {
        event.preventDefault();
        input.current?.focus();
        input.current?.select();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  return (
    <aside className="sidebar">
      <div className="brand" data-tauri-drag-region>
        <Mark size={24} />
        <span className="brand__name" data-tauri-drag-region>
          VYRA
        </span>
      </div>

      <label className="search">
        <Search size={16} strokeWidth={1.5} />
        <input
          ref={input}
          value={text}
          placeholder="Поиск"
          spellCheck={false}
          onChange={(event) => {
            setText(event.target.value);
            setSearch(event.target.value);
          }}
          onKeyDown={(event) => {
            if (event.key === 'Escape') {
              setText('');
              setSearch('');
              event.currentTarget.blur();
            }
          }}
        />
      </label>

      <div className="nav-label">Медиатека</div>
      <nav className="nav">
        {sections.map(({ route: target, label, icon: Icon }) => (
          <button key={label} className={isActive(route, target) ? 'nav__item nav__item--active' : 'nav__item'} onClick={() => go(target)}>
            <Icon size={18} strokeWidth={1.5} />
            {label}
          </button>
        ))}
      </nav>

      <div className="sidebar__spacer" data-tauri-drag-region />
      <ScanStatus />
      <nav className="nav">
        <button className={route.name === 'settings' ? 'nav__item nav__item--active' : 'nav__item'} onClick={() => go({ name: 'settings' })}>
          <Settings size={18} strokeWidth={1.5} />
          Настройки
        </button>
      </nav>
    </aside>
  );
}

export function Toasts() {
  const toasts = useStore((s) => s.toasts);
  if (!toasts.length) return null;
  return (
    <div className="toasts" role="status">
      {toasts.map((toast) => (
        <div key={toast.id} className="toast">
          {toast.text}
        </div>
      ))}
    </div>
  );
}
