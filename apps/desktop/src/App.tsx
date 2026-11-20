import { useEffect } from 'react';
import { call } from './api';
import { AlbumPage, Albums, ArtistPage, Artists, SearchPage, SettingsPage, Tracks, Welcome } from './pages';
import { NowPlaying, PlayerBar, QueuePanel } from './player';
import { Sidebar, Splash, TitleBar, Toasts } from './shell';
import { useStore, type Route } from './store';

function Screen({ route }: { route: Route }) {
  switch (route.name) {
    case 'albums':
      return <Albums />;
    case 'album':
      return <AlbumPage key={route.id} id={route.id} />;
    case 'artists':
      return <Artists />;
    case 'artist':
      return <ArtistPage key={route.artist} artist={route.artist} />;
    case 'tracks':
      return <Tracks />;
    case 'search':
      return <SearchPage text={route.text} />;
    case 'settings':
      return <SettingsPage />;
  }
}

export function App() {
  const route = useStore((s) => s.route);
  const status = useStore((s) => s.status);
  const nowPlayingOpen = useStore((s) => s.nowPlayingOpen);
  const queueOpen = useStore((s) => s.queueOpen);

  // Space pauses and resumes anywhere outside a text field.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement;
      if (event.code !== 'Space' || target.closest('input, select, textarea, button, [role="slider"]')) return;
      event.preventDefault();
      call('toggle_pause');
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const firstRun = status !== null && status.roots.length === 0 && route.name !== 'settings';

  return (
    <div className="app">
      <Sidebar />
      <main className={queueOpen ? 'main main--queue' : 'main'}>
        <TitleBar />
        {firstRun ? (
          <Welcome />
        ) : (
          <div className="content">
            <Screen route={route} />
          </div>
        )}
        {queueOpen ? <QueuePanel /> : null}
      </main>
      {nowPlayingOpen ? <NowPlaying /> : null}
      <PlayerBar />
      <Toasts />
      <Splash />
    </div>
  );
}
