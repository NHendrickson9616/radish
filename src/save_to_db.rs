use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{OptionalExtension, Transaction, params};

use crate::data_model::{AudioCodec, VersionKind};
use crate::database;
use crate::merge::{MergedGroup, ReleaseAppearance};
use crate::operation_model::{
    FieldName, FieldValue, Fields, FutureOperationProducer, Operation, OperationError,
    OperationResult, WorkflowId,
};

pub struct SaveToDb {
    expected_groups: usize,
}

impl SaveToDb {
    pub(crate) fn new(expected_groups: usize) -> Self {
        Self { expected_groups }
    }
}

impl Operation for SaveToDb {
    fn requires(&self) -> Vec<FieldName> {
        vec![
            "radish.database".into(),
            "radish.dry_run".into(),
            "radish.merged_group".into(),
        ]
    }

    fn produces(&self) -> Vec<FieldName> {
        Vec::new()
    }

    fn run(
        &self,
        _workflow_id: WorkflowId,
        inputs: Vec<Fields>,
        _future_operations: &FutureOperationProducer,
    ) -> OperationResult<Fields> {
        if inputs.len() != self.expected_groups {
            return Err(OperationError::Failed(format!(
                "save expected {} merged groups but received {}; no database changes were made",
                self.expected_groups,
                inputs.len()
            ))
            .into());
        }
        let first = inputs
            .first()
            .ok_or_else(|| OperationError::Failed("save received no merged groups".into()))?;
        if dry_run(first)? {
            return Ok(Fields::default());
        }

        let path = database_path(first)?;
        let groups = inputs
            .iter()
            .map(merged_group)
            .collect::<OperationResult<Vec<_>>>()?;
        save_groups(&path, &groups)?;
        Ok(Fields::default())
    }
}

fn save_groups(path: &Path, groups: &[&MergedGroup]) -> OperationResult<()> {
    let mut connection = database::initialize(path)?;
    let transaction = connection.transaction()?;
    for group in groups {
        save_group(&transaction, group)?;
    }
    transaction.commit()?;
    Ok(())
}

fn save_group(transaction: &Transaction<'_>, group: &MergedGroup) -> OperationResult<()> {
    validate_existing_files(transaction, &group.existing_file_ids)?;
    let (composition_id, recording_id, version_id) = hierarchy_ids(transaction, group)?;
    let timestamp = timestamp_millis()?;

    for file in &group.files {
        let size_bytes = file
            .size_bytes
            .map(i64::try_from)
            .transpose()
            .map_err(|_| OperationError::Failed("file size exceeds SQLite integer range".into()))?;
        let codec = file.codec.as_ref().map(codec_name);
        transaction.execute(
            "INSERT INTO files (
                version_id, path, file_type, codec, file_hash, audio_hash,
                size_bytes, duration_millis, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
             ON CONFLICT(path) DO UPDATE SET
                version_id = excluded.version_id,
                file_type = excluded.file_type,
                codec = excluded.codec,
                file_hash = excluded.file_hash,
                audio_hash = excluded.audio_hash,
                size_bytes = excluded.size_bytes,
                duration_millis = excluded.duration_millis,
                updated_at = excluded.updated_at",
            params![
                version_id,
                database::path_to_blob(&file.path),
                format!("{:?}", file.file_type).to_lowercase(),
                codec,
                &file.file_hash.0,
                file.audio_hash.as_ref().map(|hash| &hash.0),
                size_bytes,
                i64::try_from(file.duration_millis).map_err(|_| OperationError::Failed(
                    "audio duration exceeds SQLite integer range".into()
                ))?,
                timestamp,
            ],
        )?;
    }

    for appearance in &group.releases {
        save_release_appearance(transaction, appearance, version_id, timestamp)?;
    }

    let hierarchy_exists: bool = transaction.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM versions v
            JOIN recordings r ON r.id = v.recording_id
            WHERE v.id = ?1 AND r.id = ?2 AND r.composition_id = ?3
         )",
        params![version_id, recording_id, composition_id],
        |row| row.get(0),
    )?;
    if !hierarchy_exists {
        return Err(OperationError::Failed("saved hierarchy failed validation".into()).into());
    }

    Ok(())
}

fn hierarchy_ids(
    transaction: &Transaction<'_>,
    group: &MergedGroup,
) -> OperationResult<(i64, i64, i64)> {
    match (
        group.existing_composition_id,
        group.existing_recording_id,
        group.existing_version_id,
    ) {
        (Some(composition_id), Some(recording_id), Some(version_id)) => {
            let exists: bool = transaction.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM versions v
                    JOIN recordings r ON r.id = v.recording_id
                    WHERE v.id = ?1 AND r.id = ?2 AND r.composition_id = ?3
                 )",
                params![version_id, recording_id, composition_id],
                |row| row.get(0),
            )?;
            if !exists {
                return Err(OperationError::Failed(
                    "existing composition, recording, and version no longer agree".into(),
                )
                .into());
            }
            Ok((composition_id, recording_id, version_id))
        }
        (None, None, None) => {
            let timestamp = timestamp_millis()?;
            transaction.execute(
                "INSERT INTO compositions (name, created_at, updated_at) VALUES (?1, ?2, ?2)",
                params![&group.composition.name, timestamp],
            )?;
            let composition_id = transaction.last_insert_rowid();
            transaction.execute(
                "INSERT INTO recordings (composition_id, name, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?3)",
                params![composition_id, &group.recording.name, timestamp],
            )?;
            let recording_id = transaction.last_insert_rowid();
            transaction.execute(
                "INSERT INTO versions (recording_id, kind, name, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4)",
                params![
                    recording_id,
                    version_kind_name(&group.version.kind),
                    &group.version.name,
                    timestamp,
                ],
            )?;
            Ok((
                composition_id,
                recording_id,
                transaction.last_insert_rowid(),
            ))
        }
        _ => Err(OperationError::Failed(
            "merged group contains an incomplete existing hierarchy".into(),
        )
        .into()),
    }
}

fn validate_existing_files(transaction: &Transaction<'_>, file_ids: &[i64]) -> OperationResult<()> {
    for file_id in file_ids {
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM files WHERE id = ?1)",
            [file_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(OperationError::Failed(format!(
                "matched database file {file_id} no longer exists"
            ))
            .into());
        }
    }
    Ok(())
}

fn save_release_appearance(
    transaction: &Transaction<'_>,
    appearance: &ReleaseAppearance,
    version_id: i64,
    timestamp: i64,
) -> OperationResult<()> {
    let release_id = release_id(transaction, &appearance.release.name, timestamp)?;

    if let (Some(disc), Some(track)) = (appearance.track.disc_number, appearance.track.track_number)
    {
        let existing = transaction
            .query_row(
                "SELECT version_id, title FROM release_tracks
                 WHERE release_id = ?1 AND disc_number = ?2 AND track_number = ?3",
                params![release_id, disc, track],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        if let Some((existing_version, existing_title)) = existing {
            if existing_version == version_id
                && normalize(&existing_title) == normalize(&appearance.track.title)
            {
                return Ok(());
            }
            return Err(OperationError::Failed(format!(
                "release `{}` already has a different track at disc {disc}, track {track}",
                appearance.release.name
            ))
            .into());
        }
    } else {
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM release_tracks
                WHERE release_id = ?1 AND version_id = ?2
                  AND disc_number IS ?3 AND track_number IS ?4
                  AND title = ?5 COLLATE NOCASE
             )",
            params![
                release_id,
                version_id,
                appearance.track.disc_number,
                appearance.track.track_number,
                &appearance.track.title,
            ],
            |row| row.get(0),
        )?;
        if exists {
            return Ok(());
        }
    }

    transaction.execute(
        "INSERT INTO release_tracks (
            release_id, version_id, disc_number, track_number, title, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![
            release_id,
            version_id,
            appearance.track.disc_number,
            appearance.track.track_number,
            &appearance.track.title,
            timestamp,
        ],
    )?;
    Ok(())
}

fn release_id(transaction: &Transaction<'_>, name: &str, timestamp: i64) -> OperationResult<i64> {
    let mut statement = transaction.prepare(
        "SELECT id FROM releases WHERE trim(name) = trim(?1) COLLATE NOCASE ORDER BY id LIMIT 2",
    )?;
    let ids = statement
        .query_map([name], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    match ids.as_slice() {
        [] => {
            transaction.execute(
                "INSERT INTO releases (name, created_at, updated_at) VALUES (?1, ?2, ?2)",
                params![name, timestamp],
            )?;
            Ok(transaction.last_insert_rowid())
        }
        [id] => Ok(*id),
        _ => {
            Err(OperationError::Failed(format!("multiple database releases match `{name}`")).into())
        }
    }
}

fn merged_group(fields: &Fields) -> OperationResult<&MergedGroup> {
    match fields.get("radish.merged_group") {
        Some(FieldValue::MergedGroup(group)) => Ok(group),
        Some(_) => {
            Err(OperationError::Failed("`radish.merged_group` has the wrong type".into()).into())
        }
        None => Err(OperationError::MissingField("radish.merged_group".into()).into()),
    }
}

fn database_path(fields: &Fields) -> OperationResult<PathBuf> {
    let value = fields
        .get("radish.database")
        .and_then(FieldValue::as_value)
        .cloned()
        .ok_or_else(|| OperationError::MissingField("radish.database".into()))?;
    Ok(serde_yml::from_value(value)?)
}

fn dry_run(fields: &Fields) -> OperationResult<bool> {
    let value = fields
        .get("radish.dry_run")
        .and_then(FieldValue::as_value)
        .cloned()
        .ok_or_else(|| OperationError::MissingField("radish.dry_run".into()))?;
    Ok(serde_yml::from_value(value)?)
}

fn codec_name(codec: &AudioCodec) -> String {
    match codec {
        AudioCodec::Flac => "flac".into(),
        AudioCodec::Pcm => "pcm".into(),
        AudioCodec::Mp3 => "mp3".into(),
        AudioCodec::Vorbis => "vorbis".into(),
        AudioCodec::Aac => "aac".into(),
        AudioCodec::Alac => "alac".into(),
        AudioCodec::Opus => "opus".into(),
        AudioCodec::Other(name) => name.clone(),
        AudioCodec::Unknown => "unknown".into(),
    }
}

fn version_kind_name(kind: &VersionKind) -> &str {
    match kind {
        VersionKind::Unknown => "unknown",
        VersionKind::Master => "master",
        VersionKind::Remix => "remix",
        VersionKind::Edit => "edit",
        VersionKind::AlternateMix => "alternate_mix",
        VersionKind::Instrumental => "instrumental",
        VersionKind::Other(name) => name,
    }
}

fn normalize(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn timestamp_millis() -> OperationResult<i64> {
    Ok(
        i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis()).map_err(|_| {
            OperationError::Failed("system time exceeds SQLite integer range".into())
        })?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_model::{
        AudioHash, Composition, File, FileHash, Recording, Release, ReleaseTrack, Version,
    };
    use lofty::file::FileType;
    use rusqlite::Connection;

    fn group(path: &str, release: Option<(&str, u32)>) -> MergedGroup {
        MergedGroup {
            existing_composition_id: None,
            existing_recording_id: None,
            existing_version_id: None,
            existing_file_ids: Vec::new(),
            composition: Composition {
                name: "Composition".into(),
            },
            recording: Recording {
                name: "Recording".into(),
            },
            version: Version {
                kind: VersionKind::Unknown,
                name: None,
            },
            files: vec![File {
                path: path.into(),
                file_type: FileType::Wav,
                codec: Some(AudioCodec::Pcm),
                file_hash: FileHash(format!("file-{path}")),
                audio_hash: Some(AudioHash(format!("audio-{path}"))),
                size_bytes: Some(100),
                duration_millis: 10,
            }],
            releases: release
                .map(|(name, track)| {
                    vec![ReleaseAppearance {
                        release: Release { name: name.into() },
                        track: ReleaseTrack {
                            disc_number: Some(1),
                            track_number: Some(track),
                            title: "Track".into(),
                        },
                    }]
                })
                .unwrap_or_default(),
        }
    }

    #[test]
    fn saves_a_complete_merged_group() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(database::INITIAL_SCHEMA).unwrap();
        let transaction = connection.transaction().unwrap();

        save_group(&transaction, &group("one.wav", Some(("Release", 1)))).unwrap();
        transaction.commit().unwrap();

        for table in [
            "compositions",
            "recordings",
            "versions",
            "files",
            "releases",
            "release_tracks",
        ] {
            let count: i64 = connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 1, "unexpected row count in {table}");
        }
    }

    #[test]
    fn release_position_conflict_rolls_back_the_batch() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(database::INITIAL_SCHEMA).unwrap();
        let transaction = connection.transaction().unwrap();

        save_group(&transaction, &group("one.wav", Some(("Release", 1)))).unwrap();
        let error = save_group(&transaction, &group("two.wav", Some(("Release", 1))))
            .expect_err("different versions cannot occupy one release position");
        assert!(error.to_string().contains("different track"));
        transaction.rollback().unwrap();

        let count: i64 = connection
            .query_row("SELECT count(*) FROM compositions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
