use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileHash(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AudioHash(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AudioCodec {
    Flac,
    Pcm,
    Mp3,
    Vorbis,
    Aac,
    Alac,
    Opus,
    Other(String),
    Unknown,
}

/// A composition. This can have multiple releases that in turn have multiple recordings
/// that in turn have different mixes that in turn have different file formats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composition {
    pub name: String,
}

/// A recording
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recording {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionKind {
    Unknown,
    Master,
    Remix,
    Edit,
    AlternateMix,
    Instrumental,
    Other(String),
}

/// A master, remix, edit, or other version of a recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub kind: VersionKind,
    pub name: Option<String>,
}

/// A file
#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub path: PathBuf,
    pub file_type: lofty::file::FileType,
    pub codec: Option<AudioCodec>,
    pub file_hash: FileHash,
    pub audio_hash: Option<AudioHash>,
    pub size_bytes: Option<u64>,
    pub duration_millis: u64,
}

/// An album, EP, single, compilation, etc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub name: String,
}

/// One track entry on a release.
///
/// This is the join between a release's tracklist and the actual audio version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseTrack {
    pub disc_number: Option<u32>,
    pub track_number: Option<u32>,
    pub title: String,
}
