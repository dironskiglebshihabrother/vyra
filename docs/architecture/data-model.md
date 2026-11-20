# Данные: SQLite, профили, резервные копии

## Что лежит на диске

```
%LOCALAPPDATA%\VYRA\
  app.json                      активный профиль, версия формата данных, флаг чистого завершения
  library\
    library.db                  общая медиатека
    artwork\ab\cd\<hash>_<size>.webp
  profiles\
    <profile-id>\
      profile.db
      themes\   fonts\
      plugin-data\<plugin-id>\
  plugins\<plugin-id>\<version>\  установленные пакеты плагинов
  backups\
  logs\
```

Файл `vyra.portable` рядом с исполняемым файлом включает переносной режим: все данные уходят в `.\data`. Нужен ли он, записано в решении Р13.

## Две базы вместо одной

`library.db` описывает общую физическую коллекцию. `profile.db` хранит всё личное. Соединение профиля открывает свой `profile.db` и подключает медиатеку через `ATTACH`, поэтому запросы умных плейлистов соединяют рейтинги профиля с треками одной командой SQL.

Что это даёт:

- файл чужого профиля просто не открыт, записать в него по ошибке нельзя;
- копия или перенос профиля сводится к одному файлу и паре папок;
- удаление профиля равно удалению каталога.

Чем приходится платить. Между базами нет внешних ключей. Транзакция, которая пишет в обе базы, в режиме WAL не атомарна при сбое питания. Поэтому профиль ссылается на треки по стабильному uid, фоновая сверка раз в сутки находит осиротевшие ссылки, а операций с записью в обе базы сразу в проекте почти нет.

Вариант с одной базой и колонкой `profile_id` в каждой таблице отклонён. В нём изоляция профилей держится на аккуратности каждого `WHERE`, а копия профиля превращается в выборочный экспорт.

## Доступ к SQLite

Используется `rusqlite` со встроенной сборкой SQLite и FTS5. У каждой базы один пишущий поток, который принимает команды через канал, и пул читающих соединений. Режим журнала WAL, `synchronous=NORMAL`, `foreign_keys=ON`.

`sqlx` рассматривался. Его проверка запросов на этапе компиляции полезна, но асинхронный драйвер локальному приложению скорости не добавляет, а `ATTACH`, online backup и пользовательские SQL-функции в `rusqlite` доступны напрямую.

Миграции лежат в репозитории как пронумерованные `.sql` файлы, версия схемы хранится в `PRAGMA user_version`, применяет их `rusqlite_migration`. Перед миграцией база копируется в `backups\pre-migration`. Понижение версии не поддерживается: старая сборка, встретив базу новее себя, отказывается её открывать и объясняет причину.

## Идентификаторы

Внутри `library.db` связи идут по целочисленным `id`, индексы получаются компактными. Наружу, то есть в профили, интерфейс, плагины и будущую синхронизацию, выдаётся `uid`: UUIDv7 в 16 байтах.

Uid трека закрепляется при первом импорте. Вместе с ним сохраняется отпечаток `audio_fp`: BLAKE3 от длины аудиоданных и их первых и последних 256 КБ, теги в расчёт не входят. Правка тегов отпечаток не меняет. Когда файл переименован или перенесён в обход VYRA, сканер видит пропажу и новый файл с тем же отпечатком, связывает их, и uid остаётся прежним вместе с рейтингами и историей.

Полный хеш файла считается лениво и нужен только поиску дубликатов.

## library.db

```sql
CREATE TABLE roots (
  id        INTEGER PRIMARY KEY,
  path      TEXT NOT NULL UNIQUE,
  kind      TEXT NOT NULL,          -- fixed | removable | network
  volume_id TEXT,                   -- серийный номер тома: диск узнаётся под другой буквой
  watch     INTEGER NOT NULL DEFAULT 1,
  status    TEXT NOT NULL           -- online | offline
);

CREATE TABLE folders (
  id        INTEGER PRIMARY KEY,
  root_id   INTEGER NOT NULL REFERENCES roots(id),
  parent_id INTEGER REFERENCES folders(id),
  rel_path  TEXT NOT NULL,
  mtime     INTEGER,
  UNIQUE (root_id, rel_path)
);

CREATE TABLE files (
  id            INTEGER PRIMARY KEY,
  folder_id     INTEGER NOT NULL REFERENCES folders(id),
  name          TEXT NOT NULL,
  size          INTEGER NOT NULL,
  mtime         INTEGER NOT NULL,
  container     TEXT,
  codec         TEXT,
  audio_fp      BLOB,
  full_hash     BLOB,
  status        TEXT NOT NULL,      -- present | missing | unavailable | unreadable
  missing_since INTEGER,
  UNIQUE (folder_id, name)
);

CREATE TABLE albums (
  id                INTEGER PRIMARY KEY,
  uid               BLOB NOT NULL UNIQUE,
  title             TEXT NOT NULL,
  sort_title        TEXT,
  artist_display    TEXT,
  year              INTEGER,
  original_year     INTEGER,
  edition           TEXT,           -- «Remastered 2011», «Deluxe»
  is_compilation    INTEGER NOT NULL DEFAULT 0,
  total_discs       INTEGER,
  mb_release_id     TEXT,
  album_group_id    INTEGER,        -- связь изданий одной записи
  folder_id         INTEGER REFERENCES folders(id),
  grouping_source   TEXT NOT NULL,  -- manual | mbid | folder
  confidence        INTEGER NOT NULL,
  ordering_conflict INTEGER NOT NULL DEFAULT 0,
  artwork_id        INTEGER REFERENCES artwork(id),
  rg_album_gain     REAL,
  rg_album_peak     REAL
);

CREATE TABLE tracks (
  id            INTEGER PRIMARY KEY,
  uid           BLOB NOT NULL UNIQUE,
  file_id       INTEGER NOT NULL REFERENCES files(id),
  start_ms      INTEGER,            -- границы трека внутри образа с листом CUE
  end_ms        INTEGER,
  cue_file_id   INTEGER REFERENCES files(id),  -- лист, из которого взяты границы
  title         TEXT,
  sort_title    TEXT,
  album_id      INTEGER REFERENCES albums(id),
  disc_no       INTEGER,
  track_no      INTEGER,
  year          INTEGER,
  composer      TEXT,
  comment       TEXT,
  duration_ms   INTEGER NOT NULL,
  sample_rate   INTEGER,
  bit_depth     INTEGER,
  channels      INTEGER,
  bitrate       INTEGER,
  lossless      INTEGER,
  rg_track_gain REAL,
  rg_track_peak REAL,
  extra_tags    TEXT,               -- JSON: поля, для которых нет колонок
  artwork_id    INTEGER REFERENCES artwork(id),
  added_at      INTEGER NOT NULL
);

CREATE TABLE artists (
  id        INTEGER PRIMARY KEY,
  uid       BLOB NOT NULL UNIQUE,
  name      TEXT NOT NULL,
  sort_name TEXT,
  name_key  TEXT NOT NULL UNIQUE,   -- нормализованное имя
  mbid      TEXT
);

CREATE TABLE track_artists (
  track_id  INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
  artist_id INTEGER NOT NULL REFERENCES artists(id),
  role      TEXT NOT NULL,          -- artist | album_artist | composer | featured
  position  INTEGER NOT NULL,
  PRIMARY KEY (track_id, role, position)
);

CREATE TABLE genres (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);
CREATE TABLE track_genres (
  track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
  genre_id INTEGER NOT NULL REFERENCES genres(id),
  PRIMARY KEY (track_id, genre_id)
);

CREATE TABLE artwork (
  id      INTEGER PRIMARY KEY,
  hash    BLOB NOT NULL UNIQUE,     -- BLAKE3 исходного изображения
  width   INTEGER,
  height  INTEGER,
  palette TEXT                      -- JSON: 4–6 цветов для акцентов и фона
);

CREATE TABLE track_lyrics (        -- отдельно от tracks: тексты большие и нужны редко
  track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
  text     TEXT NOT NULL,
  synced   INTEGER NOT NULL DEFAULT 0   -- 1: текст с метками времени
);

CREATE TABLE scan_rules (          -- что сканер пропускает
  id    INTEGER PRIMARY KEY,
  kind  TEXT NOT NULL,              -- folder | extension | hidden
  value TEXT
);

CREATE TABLE album_overrides (     -- ручные объединения и разделения
  track_uid BLOB PRIMARY KEY,
  album_uid BLOB NOT NULL
);

CREATE TABLE library_issues (      -- найденные проблемы и предложения
  id         INTEGER PRIMARY KEY,
  kind       TEXT NOT NULL,         -- duplicate_track_no | missing_album_artist | possible_same_album ...
  entity     TEXT NOT NULL,
  entity_uid BLOB NOT NULL,
  details    TEXT,                  -- JSON с предложением исправления
  status     TEXT NOT NULL          -- open | ignored | resolved
);

CREATE TABLE file_ops (            -- журнал файловых операций и правок тегов
  id         INTEGER PRIMARY KEY,
  batch_id   BLOB NOT NULL,
  kind       TEXT NOT NULL,         -- rename | move | copy | delete | write_tags
  src        TEXT,
  dst        TEXT,
  before     TEXT,                  -- JSON: полный снимок прежнего тега для отката
  status     TEXT NOT NULL,         -- planned | done | failed | undone
  profile_id BLOB,
  created_at INTEGER NOT NULL
);

CREATE VIRTUAL TABLE search USING fts5(
  title, artist, album, album_artist, composer, genre, filename,
  content='', tokenize='unicode61 remove_diacritics 2'
);
```

Статус `unavailable` означает, что недоступен весь корень: отключён внешний диск или сетевая папка. Такие файлы остаются в медиатеке и не попадают в список пропавших. Статус `missing` получает файл, которого нет при доступном корне. Статус `unreadable` получает файл, который не удалось разобрать: он попадает в журнал и в `library_issues`, сканирование идёт дальше.

Поиск по подстроке потребует второго индекса с токенизатором `trigram`. Его размер на 250 тысячах треков нужно замерить, прежде чем включать по умолчанию. При индексации буква «ё» приводится к «е».

## profile.db

```sql
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at INTEGER NOT NULL);

CREATE TABLE playlists (
  id         BLOB PRIMARY KEY,
  name       TEXT NOT NULL,
  kind       TEXT NOT NULL,         -- manual | smart
  parent_id  BLOB,                  -- папки плейлистов
  query      TEXT,                  -- JSON Query для умных плейлистов
  position   INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER
);

CREATE TABLE playlist_items (
  id          BLOB PRIMARY KEY,
  playlist_id BLOB NOT NULL REFERENCES playlists(id),
  track_uid   BLOB NOT NULL,
  position    REAL NOT NULL,        -- дробный индекс: вставка без перенумерации
  added_at    INTEGER NOT NULL,
  deleted_at  INTEGER
);

CREATE TABLE favorites (
  entity     TEXT NOT NULL,         -- track | album | artist
  entity_uid BLOB NOT NULL,
  added_at   INTEGER NOT NULL,
  deleted_at INTEGER,
  PRIMARY KEY (entity, entity_uid)
);

CREATE TABLE ratings (
  track_uid  BLOB PRIMARY KEY,
  rating     INTEGER NOT NULL,      -- 0..100, шкала в интерфейсе задаётся решением Р8
  updated_at INTEGER NOT NULL
);

CREATE TABLE play_events (
  id           INTEGER PRIMARY KEY,
  track_uid    BLOB NOT NULL,
  started_at   INTEGER NOT NULL,
  played_ms    INTEGER NOT NULL,
  completed    INTEGER NOT NULL,
  context_kind TEXT,
  context_ref  BLOB
);

CREATE TABLE track_stats (
  track_uid      BLOB PRIMARY KEY,
  play_count     INTEGER NOT NULL DEFAULT 0,
  skip_count     INTEGER NOT NULL DEFAULT 0,
  total_ms       INTEGER NOT NULL DEFAULT 0,
  first_played   INTEGER,
  last_played    INTEGER
);

CREATE TABLE playback_session (    -- одна строка
  id          INTEGER PRIMARY KEY CHECK (id = 1),
  context     TEXT,                 -- JSON ResolvedContext без списка треков
  queue_index INTEGER,
  position_ms INTEGER,
  shuffle     TEXT,                 -- seed и флаг
  repeat      TEXT,
  volume      REAL,
  updated_at  INTEGER NOT NULL
);

CREATE TABLE queue_items (
  position  INTEGER PRIMARY KEY,
  track_uid BLOB,
  path      TEXT,                   -- для файлов вне медиатеки
  layer     TEXT NOT NULL           -- context | user | history
);

CREATE TABLE resume_points (       -- где остановились в альбоме, плейлисте или папке
  context_kind TEXT NOT NULL,
  context_ref  BLOB NOT NULL,
  track_uid    BLOB NOT NULL,
  position_ms  INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL,
  PRIMARY KEY (context_kind, context_ref)
);

CREATE TABLE workspaces (
  id       BLOB PRIMARY KEY,
  name     TEXT NOT NULL,
  position INTEGER NOT NULL,
  doc      TEXT NOT NULL,           -- JSON: sidebar, страницы, плотность, тема
  schema   INTEGER NOT NULL
);

CREATE TABLE layouts (
  id         BLOB PRIMARY KEY,
  doc        TEXT NOT NULL,
  schema     INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE layout_revisions (    -- последние 20 сохранений каждого макета
  layout_id BLOB NOT NULL,
  saved_at  INTEGER NOT NULL,
  doc       TEXT NOT NULL,
  PRIMARY KEY (layout_id, saved_at)
);

CREATE TABLE themes  (id BLOB PRIMARY KEY, name TEXT NOT NULL, doc TEXT NOT NULL, schema INTEGER NOT NULL);
CREATE TABLE hotkeys (action TEXT PRIMARY KEY, accelerator TEXT NOT NULL, global INTEGER NOT NULL);

CREATE TABLE tag_actions (         -- сохранённые группы действий редактора метаданных
  id       BLOB PRIMARY KEY,
  name     TEXT NOT NULL,
  position INTEGER NOT NULL,
  doc      TEXT NOT NULL            -- JSON: список действий
);

CREATE TABLE plugin_state (
  plugin_id   TEXT PRIMARY KEY,
  enabled     INTEGER NOT NULL,
  permissions TEXT NOT NULL,        -- JSON: выданные разрешения
  settings    TEXT
);
```

Рейтинги живут только в профиле. В теги они не пишутся: у нескольких профилей оценки одного трека разные, а режим метаданных ручной. Импорт оценок из тега `POPM` и экспорт в него можно добавить отдельной командой.

Прослушивание засчитывается, когда трек звучал не меньше половины длительности или четыре минуты. Это правило Last.fm, оно привычно пользователям и совпадёт с будущим плагином скробблинга.

Колонки `updated_at` и `deleted_at` стоят в таблицах, которые могут участвовать в синхронизации. Сейчас они почти ничего не стоят, а добавить их позже означало бы миграцию с потерей истории изменений.

## Единый язык запросов

Умные плейлисты, источники данных виджетов, расширенный поиск и плагины используют один формат запроса. Rust компилирует его в SQL. Имена полей берутся из белого списка, значения передаются только параметрами, сырой SQL из интерфейса и плагинов не принимается.

```json
{
  "entity": "track",
  "where": { "all": [
    { "field": "genre",       "op": "is",         "value": "Jazz" },
    { "field": "rating",      "op": "gte",        "value": 80 },
    { "field": "last_played", "op": "older_than", "value": "30d" }
  ]},
  "sort": [{ "field": "added_at", "dir": "desc" }],
  "limit": 50
}
```

Одна реализация вместо трёх сокращает код и даёт одинаковое поведение фильтров во всех частях программы.

## Профили

Профиль в памяти представлен объектом `ProfileSession`: соединение с `profile.db`, настройки, состояние воспроизведения, выданные плагинам разрешения.

Общее для всех профилей: корни медиатеки, `library.db`, кеш обложек, установленные пакеты плагинов, кеш возможностей аудиоустройств.

Личное: всё содержимое `profile.db`, включая выбранное устройство вывода, режим вывода и пресеты эквалайзера.

Переключение профиля проходит так: текущее состояние воспроизведения сохраняется, вывод останавливается, соединение закрывается, открывается другой профиль, интерфейс получает событие `profile:changed` и перечитывает рабочее пространство и тему. Музыка при этом останавливается, потому что у каждого профиля своё состояние воспроизведения. Это допущение вынесено в решение Р6.

Файловые операции и запись тегов меняют общую коллекцию, то есть видны всем профилям. Это заложено в требованиях и тоже вынесено в Р6.

## Восстановление после сбоя

Очередь и контекст сохраняются в `playback_session` и `queue_items` при каждом изменении. Позиция записывается раз в пять секунд, при паузе и при смене трека.

`app.json` при запуске получает `clean_shutdown: false`, при штатном выходе флаг возвращается в `true`. Если при запуске флаг сброшен, VYRA открывает последний профиль, восстанавливает очередь, трек и позицию и остаётся на паузе. Автоматически музыка не стартует: звук в неожиданный момент хуже лишнего нажатия.

Незавершённые записи журнала `file_ops` при запуске доводятся до конца либо откатываются. Целостность баз проверяется командой `PRAGMA quick_check`; при повреждении предлагается восстановление из последней автоматической копии.

Обработчик паники пишет лог и минидамп в `logs\crash`. Наружу ничего не отправляется.

## Резервные копии

В копию входят `library.db`, базы выбранных профилей, темы, макеты, шрифты, настройки и манифест с версиями схем и контрольными суммами. Кеш обложек по умолчанию не входит, он восстанавливается из файлов.

Снимок базы делается командой `VACUUM INTO` во временный файл и не мешает работе. Затем всё упаковывается в один архив `.vyrabackup`.

Автоматическая копия создаётся раз в семь дней, хранятся последние пять. Перед миграцией схемы копия обязательна. Оба числа настраиваются.

Под метаданными в копии понимаются поля из базы и журнал правок тегов с прежними значениями. Возврат тегов в файлы выполняется отдельной командой и только по запросу.

## Локальная синхронизация

Требование допускает два прочтения: перенос данных профиля между компьютерами или копирование музыки на устройства. Выбор записан в решении Р7. Сейчас в схеме заложено то, что понадобится в обоих случаях: uid вместо локальных номеров, отметки изменения и удаления, сопоставление треков между машинами по `audio_fp`.
