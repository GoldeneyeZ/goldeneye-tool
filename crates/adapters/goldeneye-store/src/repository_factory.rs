use std::path::Path;

use goldeneye_ports::{
    AdrTraceRepository, CrossLinkRepository, EditRepository, IndexRepository, PortError,
    ProjectAdministrationRepository, QueryRepository, RepositoryFactory,
};

use crate::Store;

/// Creates `SQLite`-backed application repository ports.
#[derive(Debug, Default, Clone, Copy)]
pub struct SqliteRepositoryFactory;

impl RepositoryFactory for SqliteRepositoryFactory {
    fn initialize(&self, path: &Path) -> Result<(), PortError> {
        drop(Store::open(path).map_err(PortError::new)?);
        Ok(())
    }

    fn open_query(&self, path: &Path) -> Result<Box<dyn QueryRepository>, PortError> {
        Store::open_read_only(path)
            .map(|repository| Box::new(repository) as Box<dyn QueryRepository>)
            .map_err(PortError::new)
    }

    fn open_index(&self, path: &Path) -> Result<Box<dyn IndexRepository>, PortError> {
        Store::open(path)
            .map(|repository| Box::new(repository) as Box<dyn IndexRepository>)
            .map_err(PortError::new)
    }

    fn open_edit(&self, path: &Path) -> Result<Box<dyn EditRepository>, PortError> {
        Store::open(path)
            .map(|repository| Box::new(repository) as Box<dyn EditRepository>)
            .map_err(PortError::new)
    }

    fn open_crosslink(&self, path: &Path) -> Result<Box<dyn CrossLinkRepository>, PortError> {
        Store::open(path)
            .map(|repository| Box::new(repository) as Box<dyn CrossLinkRepository>)
            .map_err(PortError::new)
    }

    fn open_project_administration(
        &self,
        path: &Path,
    ) -> Result<Box<dyn ProjectAdministrationRepository>, PortError> {
        Store::open(path)
            .map(|repository| Box::new(repository) as Box<dyn ProjectAdministrationRepository>)
            .map_err(PortError::new)
    }

    fn open_adr_traces(&self, path: &Path) -> Result<Box<dyn AdrTraceRepository>, PortError> {
        Store::open(path)
            .map(|repository| Box::new(repository) as Box<dyn AdrTraceRepository>)
            .map_err(PortError::new)
    }
}
