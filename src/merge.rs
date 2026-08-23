use std::collections::BTreeMap;
use std::path::PathBuf;

use rusqlite::Connection;

use crate::data_model::{
    Composition, File, Recording, Release, ReleaseTrack, Version, VersionKind,
};
use crate::database;
use crate::operation_model::{
    FieldName, FieldValue, Fields, FutureOperationProducer, Operation, OperationError,
    OperationResult, WorkflowId,
};

pub struct Merge {
    imports: Vec<Fields>,
    database_file_ids: Vec<i64>,
    database_version_ids: Vec<i64>,
}

impl Merge {
    pub(crate) fn new(
        imports: Vec<Fields>,
        database_file_ids: Vec<i64>,
        database_version_ids: Vec<i64>,
    ) -> Self {
        Self {
            imports,
            database_file_ids,
            database_version_ids,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReleaseAppearance {
    pub release: Release,
    pub track: ReleaseTrack,
}

#[derive(Clone, Debug)]
pub struct MergedGroup {
    pub existing_composition_id: Option<i64>,
    pub existing_recording_id: Option<i64>,
    pub existing_version_id: Option<i64>,
    pub existing_file_ids: Vec<i64>,
    pub composition: Composition,
    pub recording: Recording,
    pub version: Version,
    pub files: Vec<File>,
    pub releases: Vec<ReleaseAppearance>,
}

impl Operation for Merge {
    fn requires(&self) -> Vec<FieldName> {
        Vec::new()
    }

    fn produces(&self) -> Vec<FieldName> {
        vec!["radish.merged_group".into()]
    }

    fn run(
        &self,
        _workflow_id: WorkflowId,
        _inputs: Vec<Fields>,
        _future_operations: &FutureOperationProducer,
    ) -> OperationResult<Fields> {
        let existing = match self.database_version_ids.as_slice() {
            [] => None,
            [version_id] => {
                let path =
                    database_path(self.imports.first().ok_or_else(|| {
                        OperationError::Failed("merge received no imports".into())
                    })?)?;
                let connection = database::initialize(&path)?;
                Some(load_existing_hierarchy(&connection, *version_id)?)
            }
            version_ids => {
                return Err(OperationError::Failed(format!(
                    "merge matches multiple database versions: {version_ids:?}"
                ))
                .into());
            }
        };

        let merged = merge_imports(&self.imports, &self.database_file_ids, existing)?;
        let mut output = Fields::default();
        output.insert("radish.merged_group", FieldValue::MergedGroup(merged));
        Ok(output)
    }
}

struct ExistingHierarchy {
    composition_id: i64,
    recording_id: i64,
    version_id: i64,
    composition: Composition,
    recording: Recording,
    version: Version,
}

fn load_existing_hierarchy(
    connection: &Connection,
    version_id: i64,
) -> OperationResult<ExistingHierarchy> {
    let hierarchy = connection.query_row(
        "SELECT c.id, r.id, v.id, c.name, r.name, v.kind, v.name
         FROM versions v
         JOIN recordings r ON r.id = v.recording_id
         JOIN compositions c ON c.id = r.composition_id
         WHERE v.id = ?1",
        [version_id],
        |row| {
            let kind: String = row.get(5)?;
            Ok(ExistingHierarchy {
                composition_id: row.get(0)?,
                recording_id: row.get(1)?,
                version_id: row.get(2)?,
                composition: Composition { name: row.get(3)? },
                recording: Recording { name: row.get(4)? },
                version: Version {
                    kind: parse_version_kind(&kind),
                    name: row.get(6)?,
                },
            })
        },
    )?;
    Ok(hierarchy)
}

fn parse_version_kind(kind: &str) -> VersionKind {
    match kind {
        "unknown" => VersionKind::Unknown,
        "master" => VersionKind::Master,
        "remix" => VersionKind::Remix,
        "edit" => VersionKind::Edit,
        "alternate_mix" => VersionKind::AlternateMix,
        "instrumental" => VersionKind::Instrumental,
        other => VersionKind::Other(other.into()),
    }
}

fn merge_imports(
    imports: &[Fields],
    database_file_ids: &[i64],
    existing: Option<ExistingHierarchy>,
) -> OperationResult<MergedGroup> {
    let first = imports
        .first()
        .ok_or_else(|| OperationError::Failed("merge received no imports".into()))?;
    let mut composition = composition(first)?.clone();
    let mut merged_recording = recording(first)?.clone();
    let mut version = version(first)?.clone();
    let mut files = BTreeMap::<PathBuf, File>::new();
    let mut releases = Vec::<ReleaseAppearance>::new();

    for fields in imports {
        let imported_composition = composition_from(fields)?;
        if normalize(&imported_composition.name) != normalize(&composition.name) {
            return conflict("composition", &composition.name, &imported_composition.name);
        }

        let imported_recording = recording(fields)?;
        if normalize(&imported_recording.name) != normalize(&merged_recording.name) {
            return conflict(
                "recording",
                &merged_recording.name,
                &imported_recording.name,
            );
        }

        version = combine_versions(&version, version_from(fields)?)?;

        let imported_file = file(fields)?.clone();
        if let Some(previous) = files.get(&imported_file.path)
            && previous != &imported_file
        {
            return Err(OperationError::Failed(format!(
                "file path `{}` was imported with conflicting metadata",
                imported_file.path.display()
            ))
            .into());
        }
        files.insert(imported_file.path.clone(), imported_file);

        match (
            fields.get("radish.release"),
            fields.get("radish.release_track"),
        ) {
            (Some(FieldValue::Release(release)), Some(FieldValue::ReleaseTrack(track))) => {
                let appearance = ReleaseAppearance {
                    release: release.clone(),
                    track: track.clone(),
                };
                if !releases.iter().any(|existing| {
                    existing.release == appearance.release && existing.track == appearance.track
                }) {
                    releases.push(appearance);
                }
            }
            (None, None) => {}
            _ => {
                return Err(OperationError::Failed(
                    "release and release track metadata must appear together".into(),
                )
                .into());
            }
        }
    }

    let (composition_id, recording_id, version_id) = if let Some(existing) = existing {
        if version.kind != VersionKind::Unknown && version != existing.version {
            return Err(OperationError::Failed(
                "imported version metadata conflicts with the existing database version".into(),
            )
            .into());
        }
        composition = existing.composition;
        merged_recording = existing.recording;
        version = existing.version;
        (
            Some(existing.composition_id),
            Some(existing.recording_id),
            Some(existing.version_id),
        )
    } else {
        (None, None, None)
    };

    Ok(MergedGroup {
        existing_composition_id: composition_id,
        existing_recording_id: recording_id,
        existing_version_id: version_id,
        existing_file_ids: database_file_ids.to_vec(),
        composition,
        recording: merged_recording,
        version,
        files: files.into_values().collect(),
        releases,
    })
}

fn combine_versions(current: &Version, new: &Version) -> OperationResult<Version> {
    match (&current.kind, &new.kind) {
        (VersionKind::Unknown, _) => Ok(new.clone()),
        (_, VersionKind::Unknown) => Ok(current.clone()),
        _ if current == new => Ok(current.clone()),
        _ => Err(OperationError::Failed("imported version metadata conflicts".into()).into()),
    }
}

fn conflict<T>(field: &str, left: &str, right: &str) -> OperationResult<T> {
    Err(OperationError::Failed(format!(
        "conflicting {field} metadata: `{left}` and `{right}`"
    ))
    .into())
}

fn normalize(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn database_path(fields: &Fields) -> OperationResult<PathBuf> {
    let value = fields
        .get("radish.database")
        .and_then(FieldValue::as_value)
        .cloned()
        .ok_or_else(|| OperationError::MissingField("radish.database".into()))?;
    Ok(serde_yml::from_value(value)?)
}

fn composition(fields: &Fields) -> OperationResult<&Composition> {
    composition_from(fields)
}

fn composition_from(fields: &Fields) -> OperationResult<&Composition> {
    match fields.get("radish.composition") {
        Some(FieldValue::Composition(value)) => Ok(value),
        Some(_) => {
            Err(OperationError::Failed("`radish.composition` has the wrong type".into()).into())
        }
        None => Err(OperationError::MissingField("radish.composition".into()).into()),
    }
}

fn recording(fields: &Fields) -> OperationResult<&Recording> {
    match fields.get("radish.recording") {
        Some(FieldValue::Recording(value)) => Ok(value),
        Some(_) => {
            Err(OperationError::Failed("`radish.recording` has the wrong type".into()).into())
        }
        None => Err(OperationError::MissingField("radish.recording".into()).into()),
    }
}

fn version(fields: &Fields) -> OperationResult<&Version> {
    version_from(fields)
}

fn version_from(fields: &Fields) -> OperationResult<&Version> {
    match fields.get("radish.version") {
        Some(FieldValue::Version(value)) => Ok(value),
        Some(_) => Err(OperationError::Failed("`radish.version` has the wrong type".into()).into()),
        None => Err(OperationError::MissingField("radish.version".into()).into()),
    }
}

fn file(fields: &Fields) -> OperationResult<&File> {
    match fields.get("radish.file") {
        Some(FieldValue::File(value)) => Ok(value),
        Some(_) => Err(OperationError::Failed("`radish.file` has the wrong type".into()).into()),
        None => Err(OperationError::MissingField("radish.file".into()).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_model::{AudioCodec, AudioHash, FileHash};
    use lofty::file::FileType;

    fn imported(path: &str, title: &str) -> Fields {
        let mut fields = Fields::default();
        fields.insert(
            "radish.composition",
            FieldValue::Composition(Composition { name: title.into() }),
        );
        fields.insert(
            "radish.recording",
            FieldValue::Recording(Recording { name: title.into() }),
        );
        fields.insert(
            "radish.version",
            FieldValue::Version(Version {
                kind: VersionKind::Unknown,
                name: None,
            }),
        );
        fields.insert(
            "radish.file",
            FieldValue::File(File {
                path: path.into(),
                file_type: FileType::Wav,
                codec: Some(AudioCodec::Pcm),
                file_hash: FileHash("file-hash".into()),
                audio_hash: Some(AudioHash("audio-hash".into())),
                size_bytes: Some(100),
                duration_millis: 10,
            }),
        );
        fields
    }

    #[test]
    fn preserves_distinct_files_under_one_unknown_version() {
        let merged = merge_imports(
            &[imported("one.wav", "Song"), imported("two.wav", " song ")],
            &[],
            None,
        )
        .unwrap();

        assert_eq!(merged.files.len(), 2);
        assert_eq!(merged.version.kind, VersionKind::Unknown);
        assert_eq!(merged.existing_version_id, None);
    }

    #[test]
    fn deduplicates_the_same_file_path() {
        let merged = merge_imports(
            &[imported("one.wav", "Song"), imported("one.wav", "Song")],
            &[],
            None,
        )
        .unwrap();

        assert_eq!(merged.files.len(), 1);
    }

    #[test]
    fn rejects_conflicting_composition_metadata() {
        let error = merge_imports(
            &[imported("one.wav", "First"), imported("two.wav", "Second")],
            &[],
            None,
        )
        .unwrap_err();

        assert!(error.to_string().contains("composition metadata"));
    }
}
