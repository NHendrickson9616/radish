use std::boxed::Box;
use std::error::Error;
use std::path::PathBuf;
use walkdir::WalkDir;

use crate::metadata::make_imported_file_draft;

#[derive(Debug, Clone)]
pub struct ImportOptions {
    pub max_depth: usize,
    pub follow_symlinks: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            max_depth: 10,
            follow_symlinks: false,
        }
    }
}

fn discover_audio_files(
    paths: &[PathBuf],
    options: &ImportOptions,
) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            let walker = WalkDir::new(path)
                .follow_links(options.follow_symlinks)
                .max_depth(options.max_depth);

            for entry in walker.into_iter().filter_map(Result::ok) {
                if entry.file_type().is_file() {
                    files.push(entry.path().to_path_buf());
                }
            }
        } else {
            files.push(path.to_path_buf());
        }
    }
    Ok(files)
}

pub fn import_paths(paths: &[PathBuf], options: &ImportOptions) -> Result<(), Box<dyn Error>> {
    let files = discover_audio_files(paths, options)?;

    for file in files {
        let _metadata = make_imported_file_draft(&file)?;
    }
    Ok(())
}

pub fn import_db(_path: &PathBuf, _options: &ImportOptions) -> Result<(), Box<dyn Error>> {
    unimplemented!();
}
