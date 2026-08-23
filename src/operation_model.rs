//! Outline of the operation/workflow model.

use crate::data_model::{Composition, File, Recording, Release, ReleaseTrack, Version};
use crate::merge::MergedGroup;

use serde_yml::Value;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::PathBuf;

pub type FieldName = String;
pub type OperationResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync + 'static>>;

#[derive(Clone, Debug)]
pub enum FieldValue {
    Value(Value),
    Composition(Composition),
    Recording(Recording),
    Version(Version),
    File(File),
    Release(Release),
    ReleaseTrack(ReleaseTrack),
    MergedGroup(MergedGroup),
}

impl FieldValue {
    pub(crate) fn as_value(&self) -> Option<&Value> {
        match self {
            Self::Value(value) => Some(value),
            _ => None,
        }
    }
}

impl From<Value> for FieldValue {
    fn from(value: Value) -> Self {
        Self::Value(value)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Fields {
    values: BTreeMap<FieldName, FieldValue>,
}

impl Fields {
    pub fn get(&self, name: &str) -> Option<&FieldValue> {
        self.values.get(name)
    }

    pub fn insert(&mut self, name: impl Into<FieldName>, value: impl Into<FieldValue>) {
        self.values.insert(name.into(), value.into());
    }

    fn extend(&mut self, other: Fields) {
        self.values.extend(other.values);
    }

    fn project(
        &self,
        shared: &Fields,
        required: &[FieldName],
        optional: &[FieldName],
    ) -> OperationResult<Fields> {
        let mut projected = Fields::default();
        for name in required {
            let value = self
                .get(name)
                .or_else(|| shared.get(name))
                .ok_or_else(|| OperationError::MissingField(name.clone()))?;
            projected.insert(name.clone(), value.clone());
        }
        for name in optional {
            if let Some(value) = self.get(name).or_else(|| shared.get(name)) {
                projected.insert(name.clone(), value.clone());
            }
        }
        Ok(projected)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WorkflowId(u64);

#[derive(Debug)]
pub enum OperationError {
    MissingField(FieldName),
    UndeclaredOutput(FieldName),
    Failed(String),
}

impl std::fmt::Display for OperationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingField(field) => write!(formatter, "missing required field `{field}`"),
            Self::UndeclaredOutput(field) => write!(formatter, "undeclared output field `{field}`"),
            Self::Failed(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for OperationError {}

pub trait Operation: Send {
    fn requires(&self) -> Vec<FieldName>;

    fn optional(&self) -> Vec<FieldName> {
        Vec::new()
    }

    fn produces(&self) -> Vec<FieldName>;
    fn run(
        &self,
        workflow_id: WorkflowId,
        inputs: Vec<Fields>,
        future_operations: &FutureOperationProducer,
    ) -> OperationResult<Fields>;
}

/// Runtime description of one operation supplied by an external plugin.
pub struct PluginOperation {
    executable: PathBuf,
    operation: String,
    requires: Vec<FieldName>,
    produces: Vec<FieldName>,
}

impl Operation for PluginOperation {
    fn requires(&self) -> Vec<FieldName> {
        self.requires.clone()
    }

    fn produces(&self) -> Vec<FieldName> {
        self.produces.clone()
    }

    fn run(
        &self,
        _workflow_id: WorkflowId,
        _inputs: Vec<Fields>,
        _future_operations: &FutureOperationProducer,
    ) -> OperationResult<Fields> {
        let _ = (&self.executable, &self.operation);
        todo!("send only the projected fields to the plugin executable")
    }
}

/// Queue state for either a built-in or runtime-loaded operation.
pub struct QueuedOperation {
    pub(crate) operation: Box<dyn Operation>,
    pub(crate) fields: Fields,
    pub(crate) wait_for: Vec<WorkflowId>,
}

impl QueuedOperation {
    fn validate_output(&self, output: &Fields) -> OperationResult<()> {
        for name in output.values.keys() {
            if !self.operation.produces().contains(name) {
                return Err(OperationError::UndeclaredOutput(name.clone()).into());
            }
        }
        Ok(())
    }
}

/// A workflow is one sequential lane. Failure ends this lane but not the coordinator.
pub struct Workflow {
    operations: VecDeque<QueuedOperation>,
    outcome: Option<OperationResult<Fields>>,
}

/// Operations publish newly created workflows or append work to existing ones.
pub struct FutureOperationProducer {
    sender: std::sync::mpsc::Sender<HashMap<WorkflowId, Vec<QueuedOperation>>>,
    next_workflow_id: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl FutureOperationProducer {
    fn new(
        sender: std::sync::mpsc::Sender<HashMap<WorkflowId, Vec<QueuedOperation>>>,
        next_workflow_id: std::sync::Arc<std::sync::atomic::AtomicU64>,
    ) -> Self {
        Self {
            sender,
            next_workflow_id,
        }
    }

    pub fn create_workflow_id(&self) -> WorkflowId {
        WorkflowId(
            self.next_workflow_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        )
    }

    pub fn publish(
        &self,
        operations: HashMap<WorkflowId, Vec<QueuedOperation>>,
    ) -> OperationResult<()> {
        self.sender
            .send(operations)
            .map_err(|error| OperationError::Failed(error.to_string()).into())
    }
}

/// Owns workflow queues and coordinates sequential lanes and cross-workflow joins.
pub struct Coordinator {
    workflows: HashMap<WorkflowId, Workflow>,
    shared_fields: Fields,
    next_workflow_id: std::sync::Arc<std::sync::atomic::AtomicU64>,
    future_operation_sender: std::sync::mpsc::Sender<HashMap<WorkflowId, Vec<QueuedOperation>>>,
    future_operation_receiver: std::sync::mpsc::Receiver<HashMap<WorkflowId, Vec<QueuedOperation>>>,
}

impl Coordinator {
    pub fn new(shared_fields: Fields) -> Self {
        let (future_operation_sender, future_operation_receiver) = std::sync::mpsc::channel();
        Self {
            workflows: HashMap::new(),
            shared_fields,
            next_workflow_id: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1)),
            future_operation_sender,
            future_operation_receiver,
        }
    }

    pub fn future_operations(&self) -> FutureOperationProducer {
        FutureOperationProducer::new(
            self.future_operation_sender.clone(),
            self.next_workflow_id.clone(),
        )
    }

    pub fn enqueue(
        &self,
        operation: Box<dyn Operation>,
        fields: Fields,
    ) -> OperationResult<WorkflowId> {
        let future_operations = self.future_operations();
        let workflow_id = future_operations.create_workflow_id();
        future_operations.publish(HashMap::from([(
            workflow_id,
            vec![QueuedOperation {
                operation,
                fields,
                wait_for: Vec::new(),
            }],
        )]))?;
        Ok(workflow_id)
    }

    fn drain_future_operations(&mut self) {
        while let Ok(batch) = self.future_operation_receiver.try_recv() {
            for (workflow_id, operations) in batch {
                let workflow = self
                    .workflows
                    .entry(workflow_id)
                    .or_insert_with(|| Workflow {
                        operations: VecDeque::new(),
                        outcome: None,
                    });
                workflow.operations.extend(operations);
                workflow.outcome = None;
            }
        }
    }

    fn next_ready(&mut self) -> Option<(WorkflowId, QueuedOperation)> {
        self.drain_future_operations();

        let workflow_ids = self.workflows.keys().copied().collect::<Vec<_>>();
        for workflow_id in workflow_ids {
            let ready = self.workflows[&workflow_id]
                .operations
                .front()
                .is_some_and(|operation| {
                    operation.wait_for.iter().all(|wait_for| {
                        self.workflows
                            .get(wait_for)
                            .is_some_and(|workflow| workflow.outcome.is_some())
                    })
                });

            if ready {
                let operation = self
                    .workflows
                    .get_mut(&workflow_id)
                    .and_then(|workflow| workflow.operations.pop_front())?;
                return Some((workflow_id, operation));
            }
        }

        None
    }

    fn inputs_for(&self, operation: &QueuedOperation) -> OperationResult<Vec<Fields>> {
        if operation.wait_for.is_empty() {
            return Ok(vec![operation.fields.project(
                &self.shared_fields,
                &operation.operation.requires(),
                &operation.operation.optional(),
            )?]);
        }

        operation
            .wait_for
            .iter()
            .filter_map(|wait_for| match &self.workflows[wait_for].outcome {
                Some(Ok(fields)) => Some(fields.project(
                    &self.shared_fields,
                    &operation.operation.requires(),
                    &operation.operation.optional(),
                )),
                Some(Err(_)) => None,
                None => unreachable!("next_ready only returns resolved joins"),
            })
            .collect()
    }

    pub fn run(&mut self) -> OperationResult<()> {
        while let Some((workflow_id, operation)) = self.next_ready() {
            let result = self.inputs_for(&operation).and_then(|inputs| {
                let future_operations = self.future_operations();
                let output = operation
                    .operation
                    .run(workflow_id, inputs, &future_operations)?;
                operation.validate_output(&output)?;
                Ok(output)
            });

            let workflow = self
                .workflows
                .get_mut(&workflow_id)
                .expect("the selected workflow must still exist");

            match result {
                Ok(output) => {
                    let mut fields = operation.fields;
                    fields.extend(output);

                    if let Some(next) = workflow.operations.front_mut() {
                        let next_fields = std::mem::take(&mut next.fields);
                        fields.extend(next_fields);
                        next.fields = fields;
                    } else {
                        workflow.outcome = Some(Ok(fields));
                    }
                }
                Err(error) => {
                    workflow.operations.clear();
                    workflow.outcome = Some(Err(error));
                }
            }
        }

        let blocked = self
            .workflows
            .iter()
            .filter(|(_, workflow)| workflow.outcome.is_none() && !workflow.operations.is_empty())
            .map(|(workflow_id, _)| workflow_id.0.to_string())
            .collect::<Vec<_>>();
        if !blocked.is_empty() {
            return Err(OperationError::Failed(format!(
                "coordinator blocked waiting for workflows: {}",
                blocked.join(", ")
            ))
            .into());
        }

        let failures = self
            .workflows
            .iter()
            .filter_map(|(workflow_id, workflow)| match &workflow.outcome {
                Some(Err(error)) => Some(format!("workflow {}: {error}", workflow_id.0)),
                _ => None,
            })
            .collect::<Vec<_>>();
        if !failures.is_empty() {
            return Err(OperationError::Failed(failures.join("; ")).into());
        }

        Ok(())
    }
}

// A join becomes ready when all referenced workflows have an outcome. Failed workflows satisfy
// the wait but contribute no input fields, allowing a batch import to continue after file errors.

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct ProduceField {
        name: &'static str,
    }

    impl Operation for ProduceField {
        fn requires(&self) -> Vec<FieldName> {
            Vec::new()
        }

        fn produces(&self) -> Vec<FieldName> {
            vec![self.name.into()]
        }

        fn run(
            &self,
            _workflow_id: WorkflowId,
            _inputs: Vec<Fields>,
            _future_operations: &FutureOperationProducer,
        ) -> OperationResult<Fields> {
            let mut fields = Fields::default();
            fields.insert(self.name, serde_yml::to_value(true)?);
            Ok(fields)
        }
    }

    struct Fail;

    impl Operation for Fail {
        fn requires(&self) -> Vec<FieldName> {
            Vec::new()
        }

        fn produces(&self) -> Vec<FieldName> {
            Vec::new()
        }

        fn run(
            &self,
            _workflow_id: WorkflowId,
            _inputs: Vec<Fields>,
            _future_operations: &FutureOperationProducer,
        ) -> OperationResult<Fields> {
            Err(OperationError::Failed("intentional failure".into()).into())
        }
    }

    struct SpawnChildren {
        fail_one: bool,
        joined_inputs: Arc<Mutex<usize>>,
    }

    impl Operation for SpawnChildren {
        fn requires(&self) -> Vec<FieldName> {
            Vec::new()
        }

        fn produces(&self) -> Vec<FieldName> {
            Vec::new()
        }

        fn run(
            &self,
            workflow_id: WorkflowId,
            _inputs: Vec<Fields>,
            future_operations: &FutureOperationProducer,
        ) -> OperationResult<Fields> {
            let first = future_operations.create_workflow_id();
            let second = future_operations.create_workflow_id();
            let mut operations = HashMap::new();
            operations.insert(
                first,
                vec![QueuedOperation {
                    operation: Box::new(ProduceField { name: "child" }),
                    fields: Fields::default(),
                    wait_for: Vec::new(),
                }],
            );
            operations.insert(
                second,
                vec![QueuedOperation {
                    operation: if self.fail_one {
                        Box::new(Fail)
                    } else {
                        Box::new(ProduceField { name: "child" })
                    },
                    fields: Fields::default(),
                    wait_for: Vec::new(),
                }],
            );
            operations.insert(
                workflow_id,
                vec![QueuedOperation {
                    operation: Box::new(ObserveJoin(self.joined_inputs.clone())),
                    fields: Fields::default(),
                    wait_for: vec![first, second],
                }],
            );
            future_operations.publish(operations)?;
            Ok(Fields::default())
        }
    }

    struct ObserveJoin(Arc<Mutex<usize>>);

    impl Operation for ObserveJoin {
        fn requires(&self) -> Vec<FieldName> {
            vec!["child".into()]
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
            *self
                .0
                .lock()
                .expect("join counter lock should be available") = inputs.len();
            Ok(Fields::default())
        }
    }

    #[test]
    fn enqueue_seeds_and_completes_a_workflow() {
        let mut coordinator = Coordinator::new(Fields::default());
        let workflow_id = coordinator
            .enqueue(Box::new(ProduceField { name: "result" }), Fields::default())
            .expect("initial operation should enqueue");

        coordinator.run().expect("workflow should succeed");

        let outcome = coordinator.workflows[&workflow_id]
            .outcome
            .as_ref()
            .expect("workflow should have an outcome")
            .as_ref()
            .expect("workflow should have succeeded");
        assert!(outcome.get("result").is_some());
    }

    #[test]
    fn child_workflows_feed_a_join() {
        let joined_inputs = Arc::new(Mutex::new(0));
        let mut coordinator = Coordinator::new(Fields::default());
        coordinator
            .enqueue(
                Box::new(SpawnChildren {
                    fail_one: false,
                    joined_inputs: joined_inputs.clone(),
                }),
                Fields::default(),
            )
            .expect("initial operation should enqueue");

        coordinator.run().expect("workflows should succeed");

        assert_eq!(*joined_inputs.lock().unwrap(), 2);
    }

    #[test]
    fn failures_are_reported_after_other_workflows_continue() {
        let joined_inputs = Arc::new(Mutex::new(0));
        let mut coordinator = Coordinator::new(Fields::default());
        coordinator
            .enqueue(
                Box::new(SpawnChildren {
                    fail_one: true,
                    joined_inputs: joined_inputs.clone(),
                }),
                Fields::default(),
            )
            .expect("initial operation should enqueue");

        let error = coordinator
            .run()
            .expect_err("child failure should be reported");

        assert!(error.to_string().contains("intentional failure"));
        assert_eq!(*joined_inputs.lock().unwrap(), 1);
    }

    #[test]
    fn blocked_workflows_are_reported() {
        let mut coordinator = Coordinator::new(Fields::default());
        let future_operations = coordinator.future_operations();
        let workflow_id = future_operations.create_workflow_id();
        let missing_workflow = future_operations.create_workflow_id();
        future_operations
            .publish(HashMap::from([(
                workflow_id,
                vec![QueuedOperation {
                    operation: Box::new(ProduceField { name: "result" }),
                    fields: Fields::default(),
                    wait_for: vec![missing_workflow],
                }],
            )]))
            .expect("blocked operation should publish");

        let error = coordinator
            .run()
            .expect_err("blocked workflow should be reported");

        assert!(error.to_string().contains("coordinator blocked"));
    }
}
