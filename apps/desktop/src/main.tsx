import '@fontsource-variable/inter';
import '@fontsource-variable/jetbrains-mono';
import './styles.css';

import { createRoot } from 'react-dom/client';
import { App } from './App';
import { call, inApp } from './api';
import { connect, useStore } from './store';

// The window is an application, not a page: no browser menu and no accidental reloads.
window.addEventListener('contextmenu', (event) => event.preventDefault());

connect();
createRoot(document.getElementById('root')!).render(<App />);

// In a plain browser the address can ask for a screen, for looking at the interface
// without clicking: #route=album:3&play=3:1&now&queue
if (!inApp && location.hash.length > 1) {
  const params = new URLSearchParams(location.hash.slice(1));
  const { go, openNowPlaying, toggleQueue } = useStore.getState();
  const [name, value = ''] = (params.get('route') ?? '').split(':');
  if (name === 'album') go({ name, id: Number(value) });
  else if (name === 'artist') go({ name, artist: value });
  else if (name === 'search') go({ name, text: value });
  else if (name === 'artists' || name === 'tracks' || name === 'settings') go({ name });
  const play = params.get('play')?.split(':').map(Number);
  // The demo core answers before the window has subscribed to it, so playback waits a moment.
  setTimeout(() => {
    if (play) call('play_album', { id: play[0], index: play[1] ?? 0 });
    if (params.has('pause')) setTimeout(() => call('toggle_pause'), 300);
    if (params.has('shared')) call('set_output', { deviceId: null, exclusive: false });
    if (params.has('now')) openNowPlaying(true);
    if (params.has('queue')) toggleQueue();
  }, 500);
}
