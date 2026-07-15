use goldeneye_domain::{ProjectId, ProjectRelativePath};
use goldeneye_ports::{EditIndexer, EditRefreshResult, EditRefreshStatus, PortError};
use goldeneye_syntax::GrammarProvider;

use crate::{FileRefreshStatus, IndexService};

impl<P> EditIndexer for IndexService<P>
where
    P: GrammarProvider + Clone + Send + Sync,
{
    fn refresh_file(
        &mut self,
        project: &ProjectId,
        path: &ProjectRelativePath,
    ) -> Result<EditRefreshResult, PortError> {
        let result = IndexService::refresh_file(self, project, path).map_err(PortError::new)?;
        Ok(EditRefreshResult {
            status: match result.status {
                FileRefreshStatus::Updated => EditRefreshStatus::Updated,
                FileRefreshStatus::Deleted => EditRefreshStatus::Deleted,
                FileRefreshStatus::Unchanged => EditRefreshStatus::Unchanged,
                FileRefreshStatus::RejectedSyntax => EditRefreshStatus::RejectedSyntax,
            },
            generation: result.generation,
            diagnostics: result.diagnostics.len(),
        })
    }
}
