use std::error::Error;
use std::fs::File as FsFile;
use std::io::Read;
use std::path::Path;

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::prelude::Accessor;

use crate::data_model::{Composition, File, FileHash, Recording, Release, ReleaseTrack, Version};
use crate::import::AnalysisMode;
use crate::operation_model::{FieldValue, Fields};

fn hash_file(path: &Path) -> Result<FileHash, Box<dyn Error + Send + Sync>> {
    let mut file = FsFile::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(FileHash(hasher.finalize().to_hex().to_string()))
}

pub fn make_music_fields(
    path: &Path,
    _analysis: AnalysisMode,
) -> Result<Fields, Box<dyn Error + Send + Sync>> {
    let tagged_file = lofty::read_from_path(path)?;
    let properties = tagged_file.properties();
    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag());
    let fs_metadata = std::fs::metadata(path)?;

    let title = tag
        .and_then(|tag| tag.title().map(|title| title.to_string()))
        .or_else(|| {
            path.file_stem()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    let album = tag.and_then(|tag| tag.album().map(|album| album.to_string()));
    let track_number = tag.and_then(|tag| tag.track());
    let disc_number = tag.and_then(|tag| tag.disk());

    let mut fields = Fields::default();
    fields.insert(
        "radish.composition",
        FieldValue::Composition(Composition {
            name: title.clone(),
        }),
    );
    fields.insert(
        "radish.recording",
        FieldValue::Recording(Recording {
            name: title.clone(),
        }),
    );
    fields.insert(
        "radish.version",
        FieldValue::Version(Version {
            name: String::new(),
        }),
    );
    fields.insert(
        "radish.file",
        FieldValue::File(File {
            path: path.to_path_buf(),
            file_type: tagged_file.file_type(),
            codec: None,
            file_hash: hash_file(path)?,
            audio_hash: None,
            size_bytes: Some(fs_metadata.len()),
            duration_millis: properties
                .duration()
                .as_millis()
                .try_into()
                .expect("duration too long (longer than 584 million years)"),
        }),
    );

    if let Some(album) = album {
        fields.insert(
            "radish.release",
            FieldValue::Release(Release { name: album }),
        );
        fields.insert(
            "radish.release_track",
            FieldValue::ReleaseTrack(ReleaseTrack {
                disc_number,
                track_number,
                title,
            }),
        );
    }

    Ok(fields)
}
