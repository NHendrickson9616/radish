//! Outline of the operation/workflow model.
//!
//! This is intentionally only a design skeleton. The missing pieces are left visible so we can
//! discuss the execution shape before choosing concrete storage or persistence details.

use crate::import::AnalysisMode;
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
// Capabilities used to connect operations.

trait Paths {
    fn paths(&self) -> &[PathBuf];
}
trait HasScanParams {
    fn max_depth(&self) -> usize;
    fn follow_symlinks(&self) -> bool;
}
trait HasAnalysisMode {
    fn analysis_mode(&self) -> AnalysisMode;
}
trait FileToImport: HasAnalysisMode {
    fn path(&self) -> &Path;
}
trait ModeledFileData {}
trait HasFingerprint {}
trait DuplicateGroup {}
trait SavedFile {}

struct ScanParams {
    max_depth: usize,
    follow_symlinks: bool,
}

impl HasScanParams for ScanParams {
    fn max_depth(&self) -> usize {
        self.max_depth
    }

    fn follow_symlinks(&self) -> bool {
        self.follow_symlinks
    }
}

struct GeneralImportRequest {
    scan_params: ScanParams,
    paths: Vec<PathBuf>,
}

impl Paths for GeneralImportRequest {
    fn paths(&self) -> &[PathBuf] {
        &self.paths
    }
}

impl HasScanParams for GeneralImportRequest {
    fn max_depth(&self) -> usize {
        self.scan_params.max_depth
    }

    fn follow_symlinks(&self) -> bool {
        self.scan_params.follow_symlinks
    }
}

struct FileImportRequest {
    analysis: AnalysisMode,
    path: Path,
}

impl HasAnalysisMode for FileImportRequest {
    fn analysis_mode(&self) -> AnalysisMode {
        self.analysis
    }
}

impl FileToImport for FileImportRequest {
    fn path(&self) -> &Path {
        &self.path
    }
}

// Typed operation interface.

trait Runnable {
    type Input;
    type Output;

    fn workflow_id(&self) -> WorkflowId;
    fn run(&self, input: Self::Input) -> Result<Self::Output, OperationError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct WorkflowId(u64);

struct WorkflowHandle<T> {
    workflow_id: WorkflowId,
    output: PhantomData<fn() -> T>,
}

struct OperationError;

// Operation declarations. Each declaration says which workflow it belongs to and uses generic
// bounds to describe what it can consume and produce.

struct ScanDirAndFiles<In, Out> {
    workflow_id: WorkflowId,
    output: PhantomData<fn(In) -> Out>,
}

impl<In, Out> Runnable for ScanDirAndFiles<In, Out>
where
    In: Paths + HasScanParams,
    Out: FileToImport,
{
    type Input = In;
    type Output = Vec<Out>;

    fn workflow_id(&self) -> WorkflowId {
        self.workflow_id
    }

    fn run(&self, _input: In) -> Result<Vec<Out>, OperationError> {
        todo!("scan directories and return the files that need child workflows")
    }
}

struct Import<In, Out> {
    workflow_id: WorkflowId,
    signature: PhantomData<fn(In) -> Out>,
}

impl<In, Out> Runnable for Import<In, Out>
where
    In: FileToImport,
    Out: ModeledFileData,
{
    type Input = In;
    type Output = Out;

    fn workflow_id(&self) -> WorkflowId {
        self.workflow_id
    }

    fn run(&self, _input: In) -> Result<Out, OperationError> {
        todo!("import one file")
    }
}

struct Fingerprint<In, Out> {
    workflow_id: WorkflowId,
    signature: PhantomData<fn(In) -> Out>,
}

impl<In, Out> Runnable for Fingerprint<In, Out>
where
    In: ModeledFileData,
    Out: ModeledFileData + HasFingerprint,
{
    type Input = In;
    type Output = Out;

    fn workflow_id(&self) -> WorkflowId {
        self.workflow_id
    }

    fn run(&self, _input: In) -> Result<Out, OperationError> {
        todo!("add a fingerprint while preserving the modeled-file capabilities")
    }
}

/// Runs in the scan workflow, but waits for outputs from the spawned import workflows.
struct ScanForMerges<In, Out> {
    workflow_id: WorkflowId,
    wait_for: Vec<WorkflowHandle<In>>,
    output: PhantomData<fn() -> Out>,
}

impl<In, Out> Runnable for ScanForMerges<In, Out>
where
    In: ModeledFileData + HasFingerprint,
    Out: DuplicateGroup,
{
    type Input = Vec<In>;
    type Output = Vec<Out>;

    fn workflow_id(&self) -> WorkflowId {
        self.workflow_id
    }

    fn run(&self, _input: Vec<In>) -> Result<Vec<Out>, OperationError> {
        todo!("find groups that should produce merge operations")
    }
}

struct Merge<In, Out> {
    workflow_id: WorkflowId,
    signature: PhantomData<fn(In) -> Out>,
}

impl<In, Out> Runnable for Merge<In, Out>
where
    In: DuplicateGroup,
    Out: ModeledFileData,
{
    type Input = In;
    type Output = Out;

    fn workflow_id(&self) -> WorkflowId {
        self.workflow_id
    }

    fn run(&self, _input: In) -> Result<Out, OperationError> {
        todo!("merge a duplicate group")
    }
}

struct SaveToDb<In, Out> {
    workflow_id: WorkflowId,
    signature: PhantomData<fn(In) -> Out>,
}

impl<In, Out> Runnable for SaveToDb<In, Out>
where
    In: ModeledFileData,
    Out: SavedFile,
{
    type Input = In;
    type Output = Out;

    fn workflow_id(&self) -> WorkflowId {
        self.workflow_id
    }

    fn run(&self, _input: In) -> Result<Out, OperationError> {
        todo!("idempotently save the modeled file")
    }
}

/// The common, statically dispatched type stored in the queue.
///
/// The generic arguments select one concrete type for each capability role when a coordinator is
/// constructed. There are no trait objects and operation outputs are not erased.
enum Operation<P, FI, M, FP, D, MR, S>
where
    P: Paths + HasScanParams,
    FI: FileToImport,
    M: ModeledFileData,
    FP: ModeledFileData + HasFingerprint,
    D: DuplicateGroup,
    MR: ModeledFileData,
    S: SavedFile,
{
    Scan(ScanDirAndFiles<P, FI>),
    Import(Import<FI, M>),
    Fingerprint(Fingerprint<M, FP>),
    ScanForMerges(ScanForMerges<FP, D>),
    Merge(Merge<D, MR>),
    SaveToDb(SaveToDb<MR, S>),
}

/// Runtime information for one sequential workflow lane.
///
/// Operations can be retried because their effects must be idempotent.
struct Workflow {
    id: WorkflowId,
    // TODO: Store the latest output without erasing its type. This choice should be
    // made after the concrete application data types and transitions are clearer.
}

/// Owns the operation queue and coordinates sequential lanes and cross-workflow joins.
struct Coordinator<Op> {
    workflows: Vec<VecDeque<Op>>,
}

impl<Op> Coordinator<Op> {
    fn create_workflow<I>(&mut self, _initial_input: I) -> WorkflowHandle<I> {
        todo!("create a lane whose initial value is its first available output")
    }

    fn enqueue<O>(&mut self, _operation: O) -> WorkflowHandle<O::Output>
    where
        O: Runnable + Into<Op>,
    {
        // Push operation.into() onto the common queue.
        // Return a typed handle to the output that operation will eventually produce.
        todo!()
    }

    fn next_ready(&mut self) -> Option<Op> {
        // A normal operation is ready when its workflow has its expected latest output.
        // A join operation is ready when every WorkflowHandle in `wait_for` is available.
        // Operations that are not ready stay queued while other workflows make progress.
        todo!()
    }

    fn run(&mut self) -> Result<(), OperationError> {
        while let Some(operation) = self.next_ready() {
            // Static dispatch over Operation's variants happens here (or in an Operation method):
            //
            // 1. Take the operation's input from its own workflow, or gather its `wait_for`
            //    outputs from other workflows.
            // 2. Call that variant's typed Runnable implementation.
            // 3. Record the output as the workflow's latest completed output.
            // 4. Enqueue any workflows/operations produced by Scan or ScanForMerges.
            //
            // An execution error is returned for later user handling. No failure status is stored.
            let _ = operation;
        }

        Ok(())
    }
}

// Intended execution shape
// ========================
//
// Workflow 1
//   ScanDirAndFiles
//     |
//     +-- create Workflow 2: Import(file A) -> Fingerprint(file A) -- output A
//     +-- create Workflow 3: Import(file B) -> Fingerprint(file B) -- output B
//     +-- create Workflow 4: Import(file C) -> Fingerprint(file C) -- output C
//     |
//   ScanForMerges(wait_for: [output A, output B, output C])
//     |
//     +-- enqueue Merge operations into the selected child workflow(s)
//
// Per-workflow order supplies normal dependencies. WorkflowHandle handles are only needed for
// cross-workflow coordination such as ScanForMerges.
