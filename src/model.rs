use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CompositionId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReleaseId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReleaseTrackId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RecordingId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VersionId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileHash(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AudioHash(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Encoding {
    Flac,
    Wav,
    Mp3,
    Ogg,
    Mp4,
    Aac,
    Opus,
    Aiff,
    M4a,
    Other(String),
}

/// A composition. This can have multiple releases that in turn have multiple recordings
/// that in turn have different mixes that in turn have different file formats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composition {
    pub id: CompositionId,
    pub name: String,
}

/// A recording
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recording {
    pub id: RecordingId,
    pub composition_id: CompositionId,
    pub name: String,
}

/// A version, either a mix or a master of the recording
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub id: VersionId,
    pub recording_id: RecordingId,
    pub name: String,
}

/// A file
#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub id: FileId,
    pub version_id: VersionId,
    pub path: PathBuf,
    pub encoding: lofty::file::FileType,
    pub file_hash: FileHash,
    pub audio_hash: AudioHash,
    pub size_bytes: Option<u64>,
    pub duration_millis: u64,
}

/// An album, EP, single, compilation, etc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub id: ReleaseId,
    pub name: String,
}

/// One track entry on a release.
///
/// This is the join between a release's tracklist and the actual audio version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseTrack {
    pub id: ReleaseTrackId,
    pub release_id: ReleaseId,
    pub version_id: VersionId,
    pub disc_number: Option<u32>,
    pub track_number: Option<u32>,
    pub title: String,
}

pub struct ImportedFileDraft {
    pub path: PathBuf,
    pub encoding: lofty::file::FileType,
    pub file_hash: FileHash,
    pub size_bytes: u64,
    pub duration_millis: u64,
    pub title: Option<String>,
    pub album: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
}
