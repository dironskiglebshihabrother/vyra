<p align="center">
  <img src="docs/assets/mark.svg" width="96" height="96" alt="VYRA mark: two lilac bars that cross into the letter V">
</p>

<h1 align="center">VYRA</h1>

<p align="center">A local music center for Windows. Your music, your space.</p>

<p align="center">English · <a href="README.ru.md">Русский</a></p>

## Status

Early development. The application opens, scans a music folder and plays it; the customization features from the plan are not built yet.

- The architecture is written down in [docs/architecture](docs/architecture/README.md).
- The audio engine core is in [crates/vyra-audio](crates/vyra-audio): decoding, a direct path that keeps samples untouched, gapless joins, exact seeking, WASAPI output in shared and exclusive modes. Tests compare the output with the source bit for bit.
- The library in [crates/vyra-library](crates/vyra-library) scans folders, reads tags, groups tracks into albums and caches covers.
- The application in [apps/desktop](apps/desktop) is a Tauri window with its own title bar buttons: albums, artists, tracks, search, a player with a queue and a signal path line.
- The system icon set is built by a script in [tools/icons](tools/icons).
- The current state of every part is listed in the [roadmap](docs/architecture/roadmap.md).

<p>
  <img src="docs/assets/screen-album.png" width="800" alt="The album page of VYRA: cover, track list, the player bar with the signal path line">
</p>

<p>
  <img src="docs/assets/screen-now.png" width="800" alt="The now playing screen: a large cover and the signal path card that reports bit-perfect playback">
</p>

The screenshots show a demo collection with generated covers.

## What VYRA is going to be

VYRA plays a music collection that lives on your own disk. It works without an internet connection.

| Area | Plan |
| --- | --- |
| Audio engine | Rust, direct WASAPI output in shared and exclusive modes, gapless playback, device switching without a restart |
| Bit-perfect | The engine reports the whole signal path. When playback is not bit-perfect, it names the stage that changes the sound |
| Playback context | A track opened from an album continues the album. A single file opened from Explorer plays alone and stops |
| Library | Several root folders, background scanning, albums told apart by edition, CUE sheets, smart playlists |
| Tag editor | Batch editing on the level of Mp3tag: format strings, tag and filename converters, actions, numbering. Nothing is written to a file without a command |
| Interface | Every page is a layout document. A layout builder, a sidebar editor, workspaces and a theme engine let you rearrange and restyle all of it |
| Profiles | One shared collection with separate playlists, ratings, history and settings for each person |
| Plugins | WebAssembly plugins with declared permissions for the interface, the core and the DSP chain |

MP3, FLAC, WAV, AIFF, ALAC, AAC, OGG Vorbis and Opus are supported from the start. WMA, APE, WavPack, TAK, TTA, MKA, AC3 and DTS come with an optional FFmpeg pack. DSD follows after version 1.0.

## Architecture

```mermaid
flowchart LR
  ui["Interface<br/>React in WebView2"]
  subgraph app["vyra.exe"]
    core["Core services<br/>library, playback, profiles"]
    audio["Audio engine<br/>decode, process, WASAPI"]
    db[("SQLite")]
  end
  ui <-->|"commands and events"| core
  core --> audio
  core --> db
```

The audio engine runs on its own threads inside the Rust process. The interface lives in separate WebView2 processes, so a frozen or closed window does not stop the music.

Stack: Rust, Tauri 2, React, TypeScript, SQLite, Symphonia, WASAPI.

## Icons

<p>
  <img src="docs/assets/file-icons.png" width="656" alt="File type icons for FLAC, MP3, WAV, OPUS, M4A and DSF: an ivory sheet with the VYRA mark and a lilac band with the format name">
</p>

The mark is two translucent bars that cross into a V. The app icon, the tray icon and fifteen file type icons all come from one vector geometry, fitted separately to every size from 16 to 256 px. The rules are in [docs/design/icons.md](docs/design/icons.md).

## Documentation

The design documents are written in Russian.

| Document | Contents |
| --- | --- |
| [Overview](docs/architecture/README.md) | system layout, stack, Rust crates, repository structure |
| [Audio engine](docs/architecture/audio-engine.md) | pipeline, decoders, WASAPI, gapless, bit-perfect |
| [Playback context](docs/architecture/playback-context.md) | context rules, queue, album detection |
| [Data model](docs/architecture/data-model.md) | SQLite schema, profiles, crash recovery, backups |
| [Metadata editor](docs/architecture/metadata-editor.md) | fields, format strings, actions, tag backends |
| [Interface](docs/architecture/ui-customization.md) | layouts, widgets, themes, interface copy rules |
| [Windows integration](docs/architecture/windows-integration.md) | frameless window, installer, file types |
| [Plugins](docs/architecture/plugins.md) | three plugin kinds, permissions, versioning |
| [Roadmap](docs/architecture/roadmap.md) | prototypes, MVP, versions up to 1.0 |
| [Decisions](docs/architecture/decisions.md) | open and settled decisions |

## Roadmap

1. Prototypes: WASAPI output, gapless, bit-perfect check, interface speed, scanner, frameless window, installer.
2. MVP: playback, library, playback context, dark theme.
3. Version 0.2: layout builder, sidebar editor, workspaces, themes.
4. Version 0.3: tag editor, file manager, CUE sheets, smart playlists.
5. Version 0.4: equalizer, DSP chain, FFmpeg pack.
6. Version 0.5: profiles, backups.
7. Version 1.0: installer, file associations, updates.

Plugins, local sync, DSD and ASIO follow after 1.0.

## Building

The application needs Rust, Node and the Microsoft C++ build tools:

```
cd apps/desktop
npm install
npx tauri build --no-bundle
```

The result is `target/release/vyra.exe`. The engine and the library are tested without the window:

```
cargo test
cargo run -p vyra-cli -- devices
cargo run -p vyra-cli -- play "D:\Music\Album"
cargo run -p vyra-cli -- play --exclusive "D:\Music\Album"
```

`play` takes files or a folder. With `--exclusive` the device is opened in the format of the track, and the tool prints whether the path is bit-perfect.

The icon set is built with Node:

```
cd tools/icons
npm install
npm run build
```

## License

VYRA is licensed under [GPL-3.0](LICENSE).

Plugins that use only the published plugin API may carry any license. The draft wording is in [PLUGIN-EXCEPTION.md](PLUGIN-EXCEPTION.md).

## Credits

The interface design follows the [unslop](https://github.com/heydamir/unslop) guidelines by Damir Sokolovsky.
