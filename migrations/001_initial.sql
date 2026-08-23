PRAGMA foreign_keys = ON;
PRAGMA application_id = 1380009033; -- ASCII "RADI"
PRAGMA user_version = 1;

CREATE TABLE compositions (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
) STRICT;

-- A recording is one performance of a composition.
CREATE TABLE recordings (
    id              INTEGER PRIMARY KEY,
    composition_id  INTEGER NOT NULL REFERENCES compositions(id),
    name            TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
) STRICT;

-- A version is a mix, edit, or master of a recording.
CREATE TABLE versions (
    id            INTEGER PRIMARY KEY,
    recording_id  INTEGER NOT NULL REFERENCES recordings(id),
    kind          TEXT NOT NULL DEFAULT 'unknown',
    name          TEXT,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
) STRICT;

-- One local encoding of a version. Hashes are indexed but not unique:
-- duplicate files must be detected and resolved before constraints discard information.
CREATE TABLE files (
    id               INTEGER PRIMARY KEY,
    version_id       INTEGER NOT NULL REFERENCES versions(id),
    path             BLOB NOT NULL UNIQUE,
    file_type        TEXT NOT NULL,
    codec            TEXT,
    file_hash        TEXT NOT NULL,
    audio_hash       TEXT,
    size_bytes       INTEGER CHECK (size_bytes IS NULL OR size_bytes >= 0),
    duration_millis  INTEGER NOT NULL CHECK (duration_millis >= 0),
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
) STRICT;

-- Album, EP, single, compilation, or another published tracklist.
CREATE TABLE releases (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
) STRICT;

-- A release position points to the audio version used for that appearance.
-- Disc and track numbers remain nullable because source metadata can omit them.
CREATE TABLE release_tracks (
    id            INTEGER PRIMARY KEY,
    release_id    INTEGER NOT NULL REFERENCES releases(id) ON DELETE CASCADE,
    version_id    INTEGER NOT NULL REFERENCES versions(id),
    disc_number   INTEGER CHECK (disc_number IS NULL OR disc_number >= 0),
    track_number  INTEGER CHECK (track_number IS NULL OR track_number >= 0),
    title         TEXT NOT NULL,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
) STRICT;

CREATE INDEX recordings_by_composition
    ON recordings(composition_id);
CREATE INDEX recordings_by_name
    ON recordings(name COLLATE NOCASE);

CREATE INDEX versions_by_recording
    ON versions(recording_id);

CREATE INDEX files_by_version
    ON files(version_id);
CREATE INDEX files_by_file_hash
    ON files(file_hash);
CREATE INDEX files_by_audio_hash
    ON files(audio_hash) WHERE audio_hash IS NOT NULL;

CREATE INDEX releases_by_name
    ON releases(name COLLATE NOCASE);

CREATE INDEX release_tracks_by_release_position
    ON release_tracks(release_id, disc_number, track_number);
CREATE UNIQUE INDEX release_tracks_unique_numbered_position
    ON release_tracks(release_id, disc_number, track_number)
    WHERE disc_number IS NOT NULL AND track_number IS NOT NULL;
CREATE INDEX release_tracks_by_version
    ON release_tracks(version_id);
