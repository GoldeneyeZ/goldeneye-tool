use goldeneye_domain::ProjectId;
use goldeneye_ports::{AdrDocument, AdrTraceRepository, PortError, RuntimeTraceObservation};

use crate::{RuntimeTrace, Store};

impl AdrTraceRepository for Store {
    fn get_adr(&self, project: &ProjectId) -> Result<Option<AdrDocument>, PortError> {
        Store::get_adr(self, project)
            .map(|record| {
                record.map(|record| AdrDocument {
                    content: record.content,
                })
            })
            .map_err(PortError::new)
    }

    fn store_adr(&mut self, project: &ProjectId, content: &str) -> Result<(), PortError> {
        Store::store_adr(self, project, content).map_err(PortError::new)
    }

    fn ingest_runtime_traces(
        &mut self,
        project: &ProjectId,
        traces: &[RuntimeTraceObservation],
    ) -> Result<usize, PortError> {
        let traces = traces
            .iter()
            .map(|trace| RuntimeTrace {
                caller: trace.caller.clone(),
                callee: trace.callee.clone(),
                count: trace.count,
            })
            .collect::<Vec<_>>();
        Store::ingest_runtime_traces(self, project, &traces).map_err(PortError::new)
    }
}
