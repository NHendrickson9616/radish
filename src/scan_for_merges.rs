use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use rusqlite::{Connection, params};

use crate::data_model::File;
use crate::database;
use crate::merge::Merge;
use crate::operation_model::{
    FieldName, FieldValue, Fields, FutureOperationProducer, Operation, OperationError,
    OperationResult, QueuedOperation, WorkflowId,
};
use crate::save_to_db::SaveToDb;

pub struct ScanForMerges;

impl Operation for ScanForMerges {
    fn requires(&self) -> Vec<FieldName> {
        vec![
            "radish.database".into(),
            "radish.dry_run".into(),
            "radish.composition".into(),
            "radish.recording".into(),
            "radish.version".into(),
            "radish.file".into(),
        ]
    }

    fn optional(&self) -> Vec<FieldName> {
        vec!["radish.release".into(), "radish.release_track".into()]
    }

    fn produces(&self) -> Vec<FieldName> {
        Vec::new()
    }

    fn run(
        &self,
        workflow_id: WorkflowId,
        inputs: Vec<Fields>,
        future_operations: &FutureOperationProducer,
    ) -> OperationResult<Fields> {
        if inputs.is_empty() {
            return Ok(Fields::default());
        }

        let database_path = database_path(&inputs[0])?;
        let connection = database::open_for_scan(&database_path, dry_run(&inputs[0])?)?;
        let candidates = load_candidates(inputs, &connection)?;
        let groups = group_candidates(candidates);

        let expected_groups = groups.len();
        let mut operations = HashMap::new();
        let mut merge_workflows = Vec::with_capacity(expected_groups);
        for group in groups {
            let merge_workflow_id = future_operations.create_workflow_id();
            merge_workflows.push(merge_workflow_id);
            operations.insert(
                merge_workflow_id,
                vec![QueuedOperation {
                    operation: Box::new(Merge::new(
                        group.imports,
                        group.database_file_ids.into_iter().collect(),
                        group.database_version_ids.into_iter().collect(),
                    )),
                    fields: Fields::default(),
                    wait_for: Vec::new(),
                }],
            );
        }
        operations.insert(
            workflow_id,
            vec![QueuedOperation {
                operation: Box::new(SaveToDb::new(expected_groups)),
                fields: Fields::default(),
                wait_for: merge_workflows,
            }],
        );
        future_operations.publish(operations)?;

        Ok(Fields::default())
    }
}

struct Candidate {
    fields: Fields,
    file_hash: String,
    audio_hash: Option<String>,
    database_file_ids: BTreeSet<i64>,
    database_version_ids: BTreeSet<i64>,
}

struct MergeGroup {
    imports: Vec<Fields>,
    database_file_ids: BTreeSet<i64>,
    database_version_ids: BTreeSet<i64>,
}

fn dry_run(fields: &Fields) -> OperationResult<bool> {
    let value = fields
        .get("radish.dry_run")
        .and_then(FieldValue::as_value)
        .cloned()
        .ok_or_else(|| OperationError::MissingField("radish.dry_run".into()))?;
    Ok(serde_yml::from_value(value)?)
}

fn database_path(fields: &Fields) -> OperationResult<PathBuf> {
    let value = fields
        .get("radish.database")
        .and_then(FieldValue::as_value)
        .cloned()
        .ok_or_else(|| OperationError::MissingField("radish.database".into()))?;
    Ok(serde_yml::from_value(value)?)
}

fn load_candidates(
    inputs: Vec<Fields>,
    connection: &Connection,
) -> OperationResult<Vec<Candidate>> {
    let mut statement = connection.prepare(
        "SELECT id, version_id FROM files
         WHERE file_hash = ?1 OR audio_hash = ?2 OR path = ?3",
    )?;
    let mut candidates = Vec::with_capacity(inputs.len());

    for fields in inputs {
        let file = file(&fields)?;
        let file_hash = file.file_hash.0.clone();
        let audio_hash = file.audio_hash.as_ref().map(|hash| hash.0.clone());
        let path = database::path_to_blob(&file.path);
        let matches = statement.query_map(params![&file_hash, &audio_hash, &path], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut database_file_ids = BTreeSet::new();
        let mut database_version_ids = BTreeSet::new();
        for matched in matches {
            let (file_id, version_id) = matched?;
            database_file_ids.insert(file_id);
            database_version_ids.insert(version_id);
        }

        candidates.push(Candidate {
            fields,
            file_hash,
            audio_hash,
            database_file_ids,
            database_version_ids,
        });
    }

    Ok(candidates)
}

fn file(fields: &Fields) -> OperationResult<&File> {
    match fields.get("radish.file") {
        Some(FieldValue::File(file)) => Ok(file),
        Some(_) => Err(OperationError::Failed("`radish.file` has the wrong type".into()).into()),
        None => Err(OperationError::MissingField("radish.file".into()).into()),
    }
}

fn group_candidates(candidates: Vec<Candidate>) -> Vec<MergeGroup> {
    let mut sets = DisjointSets::new(candidates.len());
    for left in 0..candidates.len() {
        for right in (left + 1)..candidates.len() {
            if should_merge(&candidates[left], &candidates[right]) {
                sets.join(left, right);
            }
        }
    }

    let mut groups = BTreeMap::<usize, MergeGroup>::new();
    for (index, candidate) in candidates.into_iter().enumerate() {
        let group = groups
            .entry(sets.root(index))
            .or_insert_with(|| MergeGroup {
                imports: Vec::new(),
                database_file_ids: BTreeSet::new(),
                database_version_ids: BTreeSet::new(),
            });
        group.imports.push(candidate.fields);
        group.database_file_ids.extend(candidate.database_file_ids);
        group
            .database_version_ids
            .extend(candidate.database_version_ids);
    }
    groups.into_values().collect()
}

fn should_merge(left: &Candidate, right: &Candidate) -> bool {
    left.file_hash == right.file_hash
        || left
            .audio_hash
            .as_ref()
            .zip(right.audio_hash.as_ref())
            .is_some_and(|(left, right)| left == right)
        || !left
            .database_version_ids
            .is_disjoint(&right.database_version_ids)
}

struct DisjointSets {
    parents: Vec<usize>,
}

impl DisjointSets {
    fn new(length: usize) -> Self {
        Self {
            parents: (0..length).collect(),
        }
    }

    fn root(&mut self, index: usize) -> usize {
        if self.parents[index] != index {
            self.parents[index] = self.root(self.parents[index]);
        }
        self.parents[index]
    }

    fn join(&mut self, left: usize, right: usize) {
        let left = self.root(left);
        let right = self.root(right);
        self.parents[right] = left;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(
        file_hash: &str,
        audio_hash: Option<&str>,
        database_versions: &[i64],
    ) -> Candidate {
        Candidate {
            fields: Fields::default(),
            file_hash: file_hash.into(),
            audio_hash: audio_hash.map(Into::into),
            database_file_ids: BTreeSet::new(),
            database_version_ids: database_versions.iter().copied().collect(),
        }
    }

    #[test]
    fn groups_imports_connected_by_hashes_or_database_versions() {
        let groups = group_candidates(vec![
            candidate("file-a", Some("audio-a"), &[]),
            candidate("file-b", Some("audio-a"), &[]),
            candidate("file-c", Some("audio-c"), &[10]),
            candidate("file-d", Some("audio-d"), &[10]),
            candidate("file-e", Some("audio-e"), &[]),
        ]);

        let mut sizes = groups
            .into_iter()
            .map(|group| group.imports.len())
            .collect::<Vec<_>>();
        sizes.sort_unstable();
        assert_eq!(sizes, vec![1, 2, 2]);
    }
}
