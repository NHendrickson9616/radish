use std::error::Error;
use std::path::Path;

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::prelude::Accessor;
use std::fs::File;
use std::io::Read;

use crate::data_model::{FileHash, ImportedFileDraft};
use crate::import::{AnalysisMode, ImportOptions};

fn hash_file(path: &Path) -> Result<FileHash, Box<dyn Error>> {
    let mut file = File::open(path)?;
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

pub fn make_imported_file_draft(
    path: &Path,
    options: &ImportOptions,
) -> Result<ImportedFileDraft, Box<dyn Error>> {
    let tagged_file = lofty::read_from_path(path)?;
    let properties = tagged_file.properties();
    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag());

    let fs_metadata = std::fs::metadata(path)?;

    //let mut codec = None;
    //let mut audio_hash = None;

    if options.analysis == AnalysisMode::Basic {
        unimplemented!();
    }

    Ok(ImportedFileDraft {
        path: path.to_path_buf(),
        file_type: tagged_file.file_type(),
        file_hash: hash_file(path)?,
        size_bytes: fs_metadata.len(),
        duration_millis: properties
            .duration()
            .as_millis()
            .try_into()
            .expect("duration too long (longer than 584 million years)"),
        codec: None,
        audio_hash: None,
        title: tag.and_then(|tag| tag.title().map(|s| s.to_string())),
        album: tag.and_then(|tag| tag.album().map(|s| s.to_string())),
        track_number: tag.and_then(|tag| tag.track()),
        disc_number: tag.and_then(|tag| tag.disk()),
    })
}

// create data structures based on the metadata. do it by file
// Need some way to keep account of what is there... Don't really want to hold it all in memory
// but also needs to be dry run compatible and aware of what already exists.
// would like to remove caps and such for storage but would be difficult, some people prefer that level of detail
// but that means I need to scan over and have a simplification option.
// definitely need a merge option
