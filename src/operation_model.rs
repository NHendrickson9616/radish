//! Outline of the operation/workflow model.
//!
//! This is intentionally only a design skeleton. The missing pieces are left visible so we can
//! discuss the execution shape before choosing concrete storage or persistence details.

use std::collections::VecDeque;
use std::marker::PhantomData;

// Capabilities used to connect operations.

trait Paths {}
trait ScanParams {}
trait FileToImport {}
trait ModeledFileData {}
trait HasFingerprint {}
trait DuplicateGroup {}
trait SavedFile {}

// Typed operation interface.

trait Runnable {
    type Input;
    type Output;

    fn workflow_id(&self) -> WorkflowId;
    fn run(&self, input: Self::Input) -> Result<Self::Output, OperationError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct WorkflowId(u64);

/// Identifies the output a workflow will have after a particular portion of its queue finishes.
///
/// This is a checkpoint rather than a permanent "end of workflow": more work may be appended to
/// the workflow after another operation has consumed this output.
struct WorkflowHandle<T> {
    workflow_id: WorkflowId,
    output: PhantomData<fn() -> T>,
}

struct OperationError;

// Operation declarations. Each declaration says which workflow it belongs to and uses generic
// bounds to describe what it can consume and produce.

struct ScanDirAndFiles<P, F> {
    workflow_id: WorkflowId,
    params: P,
    output: PhantomData<fn() -> F>,
}

impl<P, F> Runnable for ScanDirAndFiles<P, F>
where
    P: Paths + ScanParams,
    F: FileToImport,
{
    type Input = P;
    type Output = Vec<F>;

    fn workflow_id(&self) -> WorkflowId {
        self.workflow_id
    }

    fn run(&self, _input: P) -> Result<Vec<F>, OperationError> {
        todo!("scan directories and return the files that need child workflows")
    }
}

struct Import<F, M> {
    workflow_id: WorkflowId,
    signature: PhantomData<fn(F) -> M>,
}

impl<F, M> Runnable for Import<F, M>
where
    F: FileToImport,
    M: ModeledFileData,
{
    type Input = F;
    type Output = M;

    fn workflow_id(&self) -> WorkflowId {
        self.workflow_id
    }

    fn run(&self, _input: F) -> Result<M, OperationError> {
        todo!("import one file")
    }
}

struct Fingerprint<M, F> {
    workflow_id: WorkflowId,
    signature: PhantomData<fn(M) -> F>,
}

impl<M, F> Runnable for Fingerprint<M, F>
where
    M: ModeledFileData,
    F: ModeledFileData + HasFingerprint,
{
    type Input = M;
    type Output = F;

    fn workflow_id(&self) -> WorkflowId {
        self.workflow_id
    }

    fn run(&self, _input: M) -> Result<F, OperationError> {
        todo!("add a fingerprint while preserving the modeled-file capabilities")
    }
}

/// Runs in the scan workflow, but waits for outputs from the spawned import workflows.
struct ScanForMerges<F, D> {
    workflow_id: WorkflowId,
    wait_for: Vec<WorkflowHandle<F>>,
    output: PhantomData<fn() -> D>,
}

impl<F, D> Runnable for ScanForMerges<F, D>
where
    F: ModeledFileData + HasFingerprint,
    D: DuplicateGroup,
{
    type Input = Vec<F>;
    type Output = Vec<D>;

    fn workflow_id(&self) -> WorkflowId {
        self.workflow_id
    }

    fn run(&self, _input: Vec<F>) -> Result<Vec<D>, OperationError> {
        todo!("find groups that should produce merge operations")
    }
}

struct Merge<D, M> {
    workflow_id: WorkflowId,
    signature: PhantomData<fn(D) -> M>,
}

impl<D, M> Runnable for Merge<D, M>
where
    D: DuplicateGroup,
    M: ModeledFileData,
{
    type Input = D;
    type Output = M;

    fn workflow_id(&self) -> WorkflowId {
        self.workflow_id
    }

    fn run(&self, _input: D) -> Result<M, OperationError> {
        todo!("merge a duplicate group")
    }
}

struct SaveToDb<M, S> {
    workflow_id: WorkflowId,
    signature: PhantomData<fn(M) -> S>,
}

impl<M, S> Runnable for SaveToDb<M, S>
where
    M: ModeledFileData,
    S: SavedFile,
{
    type Input = M;
    type Output = S;

    fn workflow_id(&self) -> WorkflowId {
        self.workflow_id
    }

    fn run(&self, _input: M) -> Result<S, OperationError> {
        todo!("idempotently save the modeled file")
    }
}

/// The common, statically dispatched type stored in the queue.
///
/// The generic arguments select one concrete type for each capability role when a coordinator is
/// constructed. There are no trait objects and operation outputs are not erased.
enum Operation<P, FI, M, FP, D, MR, S>
where
    P: Paths + ScanParams,
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
/// There is deliberately no queued/running/done status. A checkpoint is complete exactly when its
/// output is present. Operations can be retried because their effects must be idempotent.
struct Workflow {
    id: WorkflowId,
    next_checkpoint: u64,
    // TODO: Store the latest output/checkpoint without erasing its type. This choice should be
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
        // Reserve the next checkpoint in operation.workflow_id().
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
            //    checkpoints from other workflows.
            // 2. Call that variant's typed Runnable implementation.
            // 3. Record the output as the workflow's latest completed checkpoint.
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
//     +-- create Workflow 2: Import(file A) -> Fingerprint(file A) -- checkpoint A
//     +-- create Workflow 3: Import(file B) -> Fingerprint(file B) -- checkpoint B
//     +-- create Workflow 4: Import(file C) -> Fingerprint(file C) -- checkpoint C
//     |
//   ScanForMerges(wait_for: [checkpoint A, checkpoint B, checkpoint C])
//     |
//     +-- enqueue Merge operations into the selected child workflow(s)
//
// Per-workflow order supplies normal dependencies. WorkflowHandle handles are only needed for
// cross-workflow coordination such as ScanForMerges.
