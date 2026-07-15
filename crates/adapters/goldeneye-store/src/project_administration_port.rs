use goldeneye_domain::ProjectId;
use goldeneye_ports::{PortError, ProjectAdministrationRepository};

use crate::Store;

impl ProjectAdministrationRepository for Store {
    fn delete_project(&mut self, project: &ProjectId) -> Result<bool, PortError> {
        Store::delete_project(self, project).map_err(PortError::new)
    }
}
