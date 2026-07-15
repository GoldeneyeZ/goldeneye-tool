use goldeneye_domain::ProjectId;

use crate::PortError;

/// Project-registry mutations required by administration use cases.
pub trait ProjectAdministrationRepository: Send {
    /// Deletes one project and all project-scoped durable data.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the transactional cascade cannot complete.
    fn delete_project(&mut self, project: &ProjectId) -> Result<bool, PortError>;
}

impl<T> ProjectAdministrationRepository for Box<T>
where
    T: ProjectAdministrationRepository + ?Sized,
{
    fn delete_project(&mut self, project: &ProjectId) -> Result<bool, PortError> {
        self.as_mut().delete_project(project)
    }
}
