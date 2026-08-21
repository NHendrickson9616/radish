//! Outline of the operation/workflow model.
//!
//! This is intentionally only a design skeleton. The missing pieces are left visible so we can
//! discuss the execution shape before choosing concrete storage or persistence details.

use crate::import::AnalysisMode;
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

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

struct OperationContext<Op> {
    pending: Vec<Op>,
    next_workflow_id: Arc<AtomicU64>,
}

impl<Op> OperationContext<Op> {
    fn enqueue(&mut self, operation: Op) {
        self.pending.push(operation);
    }

    fn create_workflow_id(&self) -> WorkflowId {
        WorkflowId(self.next_workflow_id.fetch_add(1, Ordering::Relaxed))
    }
}

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

/// Shared source of workflow IDs.
///
/// Clones of this are cheap and can be handed to concurrently running tasks. Allocated IDs are
/// unique but not necessarily contiguous; a failed task may allocate an ID that is never committed.
#[derive(Clone)]
struct WorkflowIdGenerator {
    next: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl WorkflowIdGenerator {
    fn new(first: u64) -> Self {
        Self {
            next: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(first)),
        }
    }

    fn next(&self) -> WorkflowId {
        WorkflowId(self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }
}

/// Task-facing handle for publishing future operations.
///
/// Operations build their own local `Vec<Op>` and publish it as one MPSC batch after successful
/// execution. This helper only provides the sender and collision-free workflow ID allocation.
struct FutureOperationProducer<Op> {
    sender: std::sync::mpsc::Sender<Vec<Op>>,
    workflow_ids: WorkflowIdGenerator,
}

impl<Op> FutureOperationProducer<Op> {
    fn new(sender: std::sync::mpsc::Sender<Vec<Op>>, workflow_ids: WorkflowIdGenerator) -> Self {
        Self {
            sender,
            workflow_ids,
        }
    }

    fn create_workflow_id(&self) -> WorkflowId {
        self.workflow_ids.next()
    }

    fn publish(&self, operations: Vec<Op>) -> Result<(), OperationError> {
        self.sender.send(operations).map_err(|_| OperationError)
    }
}

/// Owns workflows and coordinates sequential lanes and cross-workflow joins.
struct Coordinator<Op> {
    workflows: Vec<VecDeque<Op>>,
    workflow_ids: WorkflowIdGenerator,
    future_operation_sender: std::sync::mpsc::Sender<Vec<Op>>,
    future_operation_receiver: std::sync::mpsc::Receiver<Vec<Op>>,
}

impl<Op> Coordinator<Op> {
    fn new() -> Self {
        let (future_operation_sender, future_operation_receiver) = std::sync::mpsc::channel();
        Self {
            workflows: Vec::new(),
            workflow_ids: WorkflowIdGenerator::new(1),
            future_operation_sender,
            future_operation_receiver,
        }
    }

    fn future_operations(&self) -> FutureOperationProducer<Op> {
        FutureOperationProducer::new(
            self.future_operation_sender.clone(),
            self.workflow_ids.clone(),
        )
    }

    fn drain_future_operations(&mut self) {
        while let Ok(operations) = self.future_operation_receiver.try_recv() {
            for operation in operations {
                // Route by operation.workflow_id() once the concrete `Operation` dispatch exists.
                // If the workflow does not exist yet, create its lane here.
                let _ = operation;
            }
        }
    }

    fn next_ready(&mut self) -> Option<Op> {
        self.drain_future_operations();

        // A normal operation is ready when its workflow has its expected latest output.
        // A join operation is ready when every WorkflowHandle in `wait_for` is available.
        // Operations that are not ready stay queued while other workflows make progress.
        todo!()
    }

    fn run(&mut self) -> Result<(), OperationError> {
        while let Some(operation) = self.next_ready() {
            let future_operations = self.future_operations();

            // Static dispatch over Operation's variants happens here (or in an Operation method):
            //
            // 1. Take the operation's input from its own workflow, or gather its `wait_for`
            //    outputs from other workflows.
            // 2. Pass a `FutureOperationProducer` to the operation.
            // 3. The operation builds a local Vec<Op> for future work.
            // 4. If the operation succeeds, record its output and publish that Vec as one MPSC batch.
            // 5. If the operation fails, discard the local Vec and report the error.
            //
            // Published batches contain ordinary operation values. Their workflow IDs decide which
            // workflow queue the coordinator appends them to.
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
