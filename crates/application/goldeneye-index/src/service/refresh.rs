use super::{
    BTreeMap, ExtractedFile, FileRecord, FileRefreshResult, FileRefreshStatus,
    FileSyntaxDiagnostics, Generation, IndexError, IndexRepository, IndexService, Path, ProjectId,
    ProjectRecord, ProjectRelativePath, RepositorySourceFile, relative_path,
};

#[allow(clippy::large_enum_variant)]
enum RefreshCandidate {
    Unchanged,
    Rejected(FileSyntaxDiagnostics),
    Parsed(ExtractedFile),
}

struct RefreshPlan {
    project: ProjectRecord,
    discovered: Vec<RepositorySourceFile>,
    discovered_target: Option<usize>,
    existing_files: Vec<FileRecord>,
    existing_target: Option<FileRecord>,
}

impl<R> IndexService<R>
where
    R: IndexRepository,
{
    /// Refreshes one project-relative file while preserving all other committed file graphs.
    ///
    /// A malformed-source result leaves the prior project generation unchanged.
    ///
    /// # Errors
    ///
    /// Returns a typed error for unknown projects, discovery/I/O failures, invalid graph facts,
    /// cancellation, or atomic persistence failure.
    pub fn refresh_file(
        &mut self,
        project_id: &ProjectId,
        path: &ProjectRelativePath,
    ) -> Result<FileRefreshResult, IndexError> {
        self.ensure_not_cancelled()?;
        let plan = self.prepare_refresh(project_id, path)?;
        let Some(discovered_target) = plan.discovered_target else {
            return self.refresh_missing_file(
                plan.project,
                path,
                &plan.discovered,
                plan.existing_files,
                plan.existing_target.is_some(),
            );
        };
        match self.parse_refresh_candidate(
            &plan.discovered[discovered_target],
            project_id,
            plan.existing_target.as_ref(),
        )? {
            RefreshCandidate::Unchanged => self.refresh_result(
                project_id,
                path,
                FileRefreshStatus::Unchanged,
                plan.project.generation,
                Vec::new(),
            ),
            RefreshCandidate::Rejected(diagnostics) => self.refresh_result(
                project_id,
                path,
                FileRefreshStatus::RejectedSyntax,
                plan.project.generation,
                vec![diagnostics],
            ),
            RefreshCandidate::Parsed(extracted) => self.refresh_updated_file(
                plan.project,
                path,
                &plan.discovered,
                plan.existing_files,
                extracted,
            ),
        }
    }

    fn prepare_refresh(
        &self,
        project_id: &ProjectId,
        path: &ProjectRelativePath,
    ) -> Result<RefreshPlan, IndexError> {
        let project = self
            .repository
            .get_project(project_id)
            .map_err(IndexError::Repository)?
            .ok_or_else(|| IndexError::ProjectNotFound(project_id.clone()))?;
        let report = self
            .discovery
            .discover(Path::new(&project.root_path), &self.options.discovery)?;
        self.enforce_file_limit(report.files.len())?;
        let discovered_target = report
            .files
            .iter()
            .position(|file| relative_path(&file.relative_path).is_ok_and(|value| value == *path));
        let existing_files = self
            .repository
            .list_files(project_id)
            .map_err(IndexError::Repository)?;
        let existing_target = existing_files
            .iter()
            .find(|file| file.id.path == *path)
            .cloned();
        Ok(RefreshPlan {
            project,
            discovered: report.files,
            discovered_target,
            existing_files,
            existing_target,
        })
    }

    fn parse_refresh_candidate(
        &self,
        discovered: &RepositorySourceFile,
        project: &ProjectId,
        existing: Option<&FileRecord>,
    ) -> Result<RefreshCandidate, IndexError> {
        let candidate = Self::read_candidate(discovered, project)?;
        if existing.is_some_and(|file| file.content_hash == candidate.record.content_hash) {
            return Ok(RefreshCandidate::Unchanged);
        }
        let mut extracted = self
            .parse_candidates(vec![candidate])?
            .into_iter()
            .next()
            .ok_or(IndexError::MissingWorkerResult)?;
        Ok(match extracted.diagnostics.take() {
            Some(diagnostics) => RefreshCandidate::Rejected(diagnostics),
            None => RefreshCandidate::Parsed(extracted),
        })
    }

    fn refresh_missing_file(
        &mut self,
        mut project: ProjectRecord,
        path: &ProjectRelativePath,
        discovered: &[RepositorySourceFile],
        existing_files: Vec<FileRecord>,
        existed: bool,
    ) -> Result<FileRefreshResult, IndexError> {
        if !existed {
            return self.refresh_result(
                &project.id,
                path,
                FileRefreshStatus::Unchanged,
                project.generation,
                Vec::new(),
            );
        }
        let records = existing_files
            .into_iter()
            .filter(|file| file.id.path != *path)
            .collect::<Vec<_>>();
        let parsed = self.parse_matching_records(discovered, &records, &project.id, None)?;
        self.commit_refresh_graph(&mut project, records, parsed)?;
        self.refresh_result(
            &project.id,
            path,
            FileRefreshStatus::Deleted,
            project.generation,
            Vec::new(),
        )
    }

    fn refresh_updated_file(
        &mut self,
        mut project: ProjectRecord,
        path: &ProjectRelativePath,
        discovered: &[RepositorySourceFile],
        existing_files: Vec<FileRecord>,
        extracted: ExtractedFile,
    ) -> Result<FileRefreshResult, IndexError> {
        let mut records = existing_files
            .into_iter()
            .filter(|file| file.id.path != *path)
            .collect::<Vec<_>>();
        records.push(extracted.record.clone());
        let mut parsed =
            self.parse_matching_records(discovered, &records, &project.id, Some(path))?;
        parsed.insert(path.clone(), extracted);
        self.commit_refresh_graph(&mut project, records, parsed)?;
        self.refresh_result(
            &project.id,
            path,
            FileRefreshStatus::Updated,
            project.generation,
            Vec::new(),
        )
    }

    fn commit_refresh_graph(
        &mut self,
        project: &mut ProjectRecord,
        records: Vec<FileRecord>,
        mut parsed: BTreeMap<ProjectRelativePath, ExtractedFile>,
    ) -> Result<(), IndexError> {
        let (files, nodes, edges) = self.assemble_project_graph(project, records, &mut parsed)?;
        self.ensure_not_cancelled()?;
        project.generation = self
            .repository
            .replace_project_graph(project, files, nodes, edges)
            .map_err(IndexError::Repository)?;
        goldeneye_crosslink::rebuild(&mut self.repository)?;
        Ok(())
    }

    fn refresh_result(
        &self,
        project: &ProjectId,
        path: &ProjectRelativePath,
        status: FileRefreshStatus,
        generation: Generation,
        diagnostics: Vec<FileSyntaxDiagnostics>,
    ) -> Result<FileRefreshResult, IndexError> {
        Ok(FileRefreshResult {
            project: project.clone(),
            path: path.clone(),
            status,
            generation,
            counts: self
                .repository
                .counts(project)
                .map_err(IndexError::Repository)?,
            diagnostics,
        })
    }
}
