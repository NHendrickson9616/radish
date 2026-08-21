//! Outline of the operation/workflow model.
//!
//! This is intentionally only a design skeleton. The missing pieces are left visible so we can
//! discuss the execution shape before choosing concrete storage or persistence details.

use crate::import::AnalysisMode;
use std::collections::{HashMap, VecDeque};
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
    analysis: AnalysisMode,
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

impl HasAnalysisMode for GeneralImportRequest {
    fn analysis_mode(&self) -> AnalysisMode {
        self.analysis
    }
}

struct FileImportRequest {
    analysis: AnalysisMode,
    path: PathBuf,
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

trait Runnable<Op> {
    type Input;
    type Output;

    fn run(
        &self,
        workflow_id: WorkflowId,
        input: Self::Input,
        future_operations: &FutureOperationProducer<Op>,
    ) -> Result<Self::Output, OperationError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct WorkflowId(u64);

struct WorkflowHandle<T> {
    workflow_id: WorkflowId,
    output: PhantomData<fn() -> T>,
}

struct OperationError;

// Operation declarations use generic bounds to describe what they can consume and produce.
// Workflow association is supplied by the workflow queue that contains each operation.

struct ScanDirAndFiles<In, Out> {
    output: PhantomData<fn(In) -> Out>,
}

impl<Op, In, Out> Runnable<Op> for ScanDirAndFiles<In, Out>
where
    In: Paths + HasScanParams,
    Out: FileToImport,
{
    type Input = In;
    type Output = Vec<Out>;

    fn run(
        &self,
        _workflow_id: WorkflowId,
        _input: In,
        _future_operations: &FutureOperationProducer<Op>,
    ) -> Result<Vec<Out>, OperationError> {
        todo!("scan directories and return the files that need child workflows")
    }
}

struct Import<In, Out> {
    signature: PhantomData<fn(In) -> Out>,
}

impl<Op, In, Out> Runnable<Op> for Import<In, Out>
where
    In: FileToImport,
    Out: ModeledFileData,
{
    type Input = In;
    type Output = Out;

    fn run(
        &self,
        _workflow_id: WorkflowId,
        _input: In,
        _future_operations: &FutureOperationProducer<Op>,
    ) -> Result<Out, OperationError> {
        todo!("import one file")
    }
}

struct Fingerprint<In, Out> {
    signature: PhantomData<fn(In) -> Out>,
}

impl<Op, In, Out> Runnable<Op> for Fingerprint<In, Out>
where
    In: ModeledFileData,
    Out: ModeledFileData + HasFingerprint,
{
    type Input = In;
    type Output = Out;

    fn run(
        &self,
        _workflow_id: WorkflowId,
        _input: In,
        _future_operations: &FutureOperationProducer<Op>,
    ) -> Result<Out, OperationError> {
        todo!("add a fingerprint while preserving the modeled-file capabilities")
    }
}

/// Runs in the scan workflow, but waits for outputs from the spawned import workflows.
struct ScanForMerges<In, Out> {
    wait_for: Vec<WorkflowHandle<In>>,
    output: PhantomData<fn() -> Out>,
}

impl<Op, In, Out> Runnable<Op> for ScanForMerges<In, Out>
where
    In: ModeledFileData + HasFingerprint,
    Out: DuplicateGroup,
{
    type Input = Vec<In>;
    type Output = Vec<Out>;

    fn run(
        &self,
        _workflow_id: WorkflowId,
        _input: Vec<In>,
        _future_operations: &FutureOperationProducer<Op>,
    ) -> Result<Vec<Out>, OperationError> {
        todo!("find groups that should produce merge operations")
    }
}

struct Merge<In, Out> {
    signature: PhantomData<fn(In) -> Out>,
}

impl<Op, In, Out> Runnable<Op> for Merge<In, Out>
where
    In: DuplicateGroup,
    Out: ModeledFileData,
{
    type Input = In;
    type Output = Out;

    fn run(
        &self,
        _workflow_id: WorkflowId,
        _input: In,
        _future_operations: &FutureOperationProducer<Op>,
    ) -> Result<Out, OperationError> {
        todo!("merge a duplicate group")
    }
}

struct SaveToDb<In, Out> {
    signature: PhantomData<fn(In) -> Out>,
}

impl<Op, In, Out> Runnable<Op> for SaveToDb<In, Out>
where
    In: ModeledFileData,
    Out: SavedFile,
{
    type Input = In;
    type Output = Out;

    fn run(
        &self,
        _workflow_id: WorkflowId,
        _input: In,
        _future_operations: &FutureOperationProducer<Op>,
    ) -> Result<Out, OperationError> {
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

/// Task-facing handle for publishing future operations.
///
/// Operations group future work by workflow and publish it as one MPSC batch after successful
/// execution. This helper only provides the sender and collision-free workflow ID allocation.
struct FutureOperationProducer<Op> {
    sender: std::sync::mpsc::Sender<HashMap<WorkflowId, Vec<Op>>>,
    next_workflow_id: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl<Op> FutureOperationProducer<Op> {
    fn new(
        sender: std::sync::mpsc::Sender<HashMap<WorkflowId, Vec<Op>>>,
        next_workflow_id: std::sync::Arc<std::sync::atomic::AtomicU64>,
    ) -> Self {
        Self {
            sender,
            next_workflow_id,
        }
    }

    fn create_workflow_id(&self) -> WorkflowId {
        WorkflowId(
            self.next_workflow_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        )
    }

    fn publish(&self, operations: HashMap<WorkflowId, Vec<Op>>) -> Result<(), OperationError> {
        self.sender.send(operations).map_err(|_| OperationError)
    }
}

/// Owns workflows and coordinates sequential lanes and cross-workflow joins.
struct Coordinator<Op> {
    workflows: HashMap<WorkflowId, VecDeque<Op>>,
    next_workflow_id: std::sync::Arc<std::sync::atomic::AtomicU64>,
    future_operation_sender: std::sync::mpsc::Sender<HashMap<WorkflowId, Vec<Op>>>,
    future_operation_receiver: std::sync::mpsc::Receiver<HashMap<WorkflowId, Vec<Op>>>,
}

impl<Op> Coordinator<Op> {
    fn new() -> Self {
        let (future_operation_sender, future_operation_receiver) = std::sync::mpsc::channel();
        Self {
            workflows: HashMap::new(),
            next_workflow_id: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1)),
            future_operation_sender,
            future_operation_receiver,
        }
    }

    fn future_operations(&self) -> FutureOperationProducer<Op> {
        FutureOperationProducer::new(
            self.future_operation_sender.clone(),
            self.next_workflow_id.clone(),
        )
    }

    fn drain_future_operations(&mut self) {
        while let Ok(batch) = self.future_operation_receiver.try_recv() {
            for (workflow_id, operations) in batch {
                self.workflows
                    .entry(workflow_id)
                    .or_default()
                    .extend(operations);
            }
        }
    }

    fn next_ready(&mut self) -> Option<(WorkflowId, Op)> {
        self.drain_future_operations();

        // A normal operation is ready when its workflow has its expected latest output.
        // A join operation is ready when every WorkflowHandle in `wait_for` is available.
        // Operations that are not ready stay queued while other workflows make progress.
        todo!()
    }

    fn run(&mut self) -> Result<(), OperationError> {
        while let Some((workflow_id, operation)) = self.next_ready() {
            let future_operations = self.future_operations();

            // Static dispatch over Operation's variants happens here (or in an Operation method):
            //
            // 1. Take the operation's input from its own workflow, or gather its `wait_for`
            //    outputs from other workflows.
            // 2. Pass this workflow ID and a `FutureOperationProducer` to the operation.
            // 3. The operation groups future work in a HashMap<WorkflowId, Vec<Op>>.
            // 4. If the operation succeeds, record its output and publish that map as one MPSC batch.
            // 5. If the operation fails, discard the local map and report the error.
            let _ = workflow_id;
            let _ = operation;
            let _ = future_operations;
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
