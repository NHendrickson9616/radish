use std::boxed::Box;
use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;

use clap::ValueEnum;
use lofty::probe::Probe;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::config::ImportConfig;
use crate::metadata::make_music_fields;
use crate::operation_model::{
    FieldName, FieldValue, Fields, FutureOperationProducer, Operation, OperationError,
    OperationResult, QueuedOperation, ScanForMerges, WorkflowId,
};

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AnalysisMode {
    /// Only extract tags, file/container facts, and the full-file hash.
    Fast,
    /// Fast analysis plus decoded-audio facts such as codec and audio hash.
    #[default]
    Basic,
    /// Basic analysis plus expensive fingerprinting and loudness analysis.
    Full,
}

pub struct Import;

impl Operation for Import {
    fn requires(&self) -> Vec<FieldName> {
        vec!["radish.path".into(), "radish.analysis".into()]
    }

    fn produces(&self) -> Vec<FieldName> {
        vec![
            "radish.composition".into(),
            "radish.recording".into(),
            "radish.version".into(),
            "radish.file".into(),
            "radish.release".into(),
            "radish.release_track".into(),
        ]
    }

    fn run(
        &self,
        _workflow_id: WorkflowId,
        inputs: Vec<Fields>,
        _future_operations: &FutureOperationProducer,
    ) -> OperationResult<Fields> {
        let input = inputs
            .first()
            .ok_or_else(|| OperationError::Failed("import received no input".into()))?;
        let path: PathBuf = serde_yml::from_value(
            input
                .get("radish.path")
                .and_then(FieldValue::as_value)
                .cloned()
                .ok_or_else(|| OperationError::MissingField("radish.path".into()))?,
        )?;
        let analysis: AnalysisMode = serde_yml::from_value(
            input
                .get("radish.analysis")
                .and_then(FieldValue::as_value)
                .cloned()
                .ok_or_else(|| OperationError::MissingField("radish.analysis".into()))?,
        )?;

        make_music_fields(&path, analysis)
    }
}

pub struct ScanDirAndFiles;

impl Operation for ScanDirAndFiles {
    fn requires(&self) -> Vec<FieldName> {
        vec![
            "radish.paths".into(),
            "radish.max_depth".into(),
            "radish.follow_symlinks".into(),
        ]
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
        let input = inputs
            .first()
            .ok_or_else(|| OperationError::Failed("scan received no input".into()))?;
        let paths: Vec<PathBuf> = serde_yml::from_value(
            input
                .get("radish.paths")
                .and_then(FieldValue::as_value)
                .cloned()
                .ok_or_else(|| OperationError::MissingField("radish.paths".into()))?,
        )?;
        let max_depth: usize = serde_yml::from_value(
            input
                .get("radish.max_depth")
                .and_then(FieldValue::as_value)
                .cloned()
                .ok_or_else(|| OperationError::MissingField("radish.max_depth".into()))?,
        )?;
        let follow_symlinks: bool = serde_yml::from_value(
            input
                .get("radish.follow_symlinks")
                .and_then(FieldValue::as_value)
                .cloned()
                .ok_or_else(|| OperationError::MissingField("radish.follow_symlinks".into()))?,
        )?;

        let mut files = Vec::new();
        for path in paths {
            if path.is_dir() {
                for entry in WalkDir::new(path)
                    .max_depth(max_depth)
                    .follow_links(follow_symlinks)
                    .into_iter()
                    .filter_map(Result::ok)
                {
                    if entry.file_type().is_file()
                        && Probe::open(entry.path())
                            .ok()
                            .and_then(|probe| probe.guess_file_type().ok())
                            .is_some()
                    {
                        files.push(entry.into_path());
                    }
                }
            } else {
                files.push(path);
            }
        }

        let mut operations = HashMap::new();
        let mut wait_for = Vec::new();
        for file in files {
            let child_id = future_operations.create_workflow_id();
            wait_for.push(child_id);

            let mut fields = Fields::default();
            fields.insert("radish.path", serde_yml::to_value(file)?);
            operations.insert(
                child_id,
                vec![QueuedOperation {
                    operation: Box::new(Import),
                    fields,
                    wait_for: Vec::new(),
                }],
            );
        }

        operations.insert(
            workflow_id,
            vec![QueuedOperation {
                operation: Box::new(ScanForMerges),
                fields: Fields::default(),
                wait_for,
            }],
        );
        future_operations.publish(operations)?;
        Ok(Fields::default())
    }
}

fn discover_audio_files(
    paths: &[PathBuf],
    config: &ImportConfig,
) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            let walker = WalkDir::new(path)
                .follow_links(config.follow_symlinks)
                .max_depth(config.max_depth);

            for entry in walker.into_iter().filter_map(Result::ok) {
                if entry.file_type().is_file()
                    && Probe::open(entry.path())
                        .ok()
                        .and_then(|probe| probe.guess_file_type().ok())
                        .is_some()
                {
                    files.push(entry.path().to_path_buf());
                }
            }
        } else {
            files.push(path.to_path_buf());
        }
    }
    Ok(files)
}

pub fn import_paths(paths: &[PathBuf], config: &ImportConfig) -> Result<(), Box<dyn Error>> {
    let files = discover_audio_files(paths, config)?;

    for file in files {
        let _fields = make_music_fields(&file, config.analysis)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
    }
    Ok(())
}

pub fn import_db(_path: &PathBuf, _config: &ImportConfig) -> Result<(), Box<dyn Error>> {
    unimplemented!();
}
