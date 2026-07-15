use goldeneye_domain::ProjectId;

use crate::PortError;

/// Application-facing ADR content without adapter-owned persistence metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdrDocument {
    pub content: String,
}

/// One runtime caller/callee observation submitted for durable validation and aggregation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeTraceObservation {
    pub caller: String,
    pub callee: String,
    pub count: u64,
}

/// ADR and runtime-trace persistence required by service use cases.
pub trait AdrTraceRepository: Send {
    /// Reads the project's durable ADR when one exists.
    ///
    /// # Errors
    ///
    /// Returns an error when ADR persistence cannot be read.
    fn get_adr(&self, project: &ProjectId) -> Result<Option<AdrDocument>, PortError>;

    /// Stores or replaces the project's durable ADR.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is missing or the ADR cannot be persisted.
    fn store_adr(&mut self, project: &ProjectId, content: &str) -> Result<(), PortError>;

    /// Atomically aggregates validated runtime observations for one project.
    ///
    /// # Errors
    ///
    /// Returns an error when validation, aggregation, or persistence fails.
    fn ingest_runtime_traces(
        &mut self,
        project: &ProjectId,
        traces: &[RuntimeTraceObservation],
    ) -> Result<usize, PortError>;
}

impl<T> AdrTraceRepository for Box<T>
where
    T: AdrTraceRepository + ?Sized,
{
    fn get_adr(&self, project: &ProjectId) -> Result<Option<AdrDocument>, PortError> {
        self.as_ref().get_adr(project)
    }

    fn store_adr(&mut self, project: &ProjectId, content: &str) -> Result<(), PortError> {
        self.as_mut().store_adr(project, content)
    }

    fn ingest_runtime_traces(
        &mut self,
        project: &ProjectId,
        traces: &[RuntimeTraceObservation],
    ) -> Result<usize, PortError> {
        self.as_mut().ingest_runtime_traces(project, traces)
    }
}
