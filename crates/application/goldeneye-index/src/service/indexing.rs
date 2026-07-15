use super::{
    BTreeMap, Candidate, ChangeSet, ExtractedFile, FileRecord, FileSyntaxDiagnostics, IndexError,
    IndexRepository, IndexResult, IndexService, IndexStatus, Path, ProjectRecord,
    ProjectRelativePath, RepositorySourceFile, canonical_project, classify_changes,
};

struct IndexPlan {
    project: ProjectRecord,
    stored_project: bool,
    candidates: Vec<Candidate>,
    existing_files: Vec<FileRecord>,
    existing: BTreeMap<ProjectRelativePath, FileRecord>,
    changes: ChangeSet,
}

impl IndexPlan {
    fn is_unchanged(&self) -> bool {
        self.stored_project
            && self.changes.new_files == 0
            && self.changes.changed_files == 0
            && self.changes.deleted_files == 0
    }
}

struct ParsedIndex {
    parsed_files: usize,
    parsed_by_path: BTreeMap<ProjectRelativePath, ExtractedFile>,
    diagnostics: Vec<FileSyntaxDiagnostics>,
    rejects_existing_graph: bool,
}

impl<R> IndexService<R>
where
    R: IndexRepository,
{
    /// Discovers and indexes a repository, reusing unchanged persisted file graphs.
    ///
    /// # Errors
    ///
    /// Returns a typed discovery, I/O, syntax, identity, cancellation, bound, or store error.
    /// No graph mutation occurs until all required parses and graph validation succeed.
    pub fn index_repository(&mut self, root: impl AsRef<Path>) -> Result<IndexResult, IndexError> {
        self.ensure_not_cancelled()?;
        let report = self
            .discovery
            .discover(root.as_ref(), &self.options.discovery)?;
        self.enforce_file_limit(report.files.len())?;
        self.ensure_not_cancelled()?;

        let plan = self.prepare_index(root.as_ref(), &report.files)?;
        if plan.is_unchanged() {
            return self.unchanged_index_result(plan, report.warnings);
        }

        let parsed = self.parse_index(&plan)?;
        if parsed.rejects_existing_graph {
            return self.rejected_index_result(plan, parsed, report.warnings);
        }
        self.commit_index(plan, parsed, report.warnings)
    }

    fn prepare_index(
        &self,
        root: &Path,
        files: &[RepositorySourceFile],
    ) -> Result<IndexPlan, IndexError> {
        let mut project = canonical_project(root)?;
        if let Some(project_id) = &self.options.project_id_override {
            project = ProjectRecord::new(project_id.clone(), project.root_path.clone())?;
        }
        let stored_project = self
            .repository
            .get_project(&project.id)
            .map_err(IndexError::Repository)?;
        if let Some(stored) = &stored_project {
            project.generation = stored.generation;
        }
        let candidates = self.read_candidates(files, &project.id)?;
        let existing_files = if stored_project.is_some() {
            self.repository
                .list_files(&project.id)
                .map_err(IndexError::Repository)?
        } else {
            Vec::new()
        };
        let existing = existing_files
            .iter()
            .cloned()
            .map(|file| (file.id.path.clone(), file))
            .collect::<BTreeMap<_, _>>();
        let changes = classify_changes(&candidates, &existing);
        Ok(IndexPlan {
            project,
            stored_project: stored_project.is_some(),
            candidates,
            existing_files,
            existing,
            changes,
        })
    }

    fn parse_index(&self, plan: &IndexPlan) -> Result<ParsedIndex, IndexError> {
        // Hybrid resolution depends on pending facts from every source file. Reparse the
        // discovered set whenever the project changes so calls in otherwise unchanged files
        // cannot retain stale targets after definitions move, disappear, or become ambiguous.
        let parsed_files = plan.changes.parse_inputs.len();
        let parsed = self.parse_candidates(plan.candidates.clone())?;
        let mut parsed_by_path = BTreeMap::new();
        let mut diagnostics = Vec::new();
        for extracted in parsed {
            if let Some(file_diagnostics) = &extracted.diagnostics {
                diagnostics.push(file_diagnostics.clone());
            }
            parsed_by_path.insert(extracted.record.id.path.clone(), extracted);
        }
        diagnostics.sort_by(|left, right| left.path.cmp(&right.path));
        let rejects_existing_graph = diagnostics
            .iter()
            .any(|diagnostic| plan.existing.contains_key(&diagnostic.path));
        Ok(ParsedIndex {
            parsed_files,
            parsed_by_path,
            diagnostics,
            rejects_existing_graph,
        })
    }

    fn unchanged_index_result(
        &self,
        plan: IndexPlan,
        warnings: Vec<String>,
    ) -> Result<IndexResult, IndexError> {
        let changes = plan.changes;
        Ok(IndexResult {
            status: IndexStatus::Unchanged,
            counts: self
                .repository
                .counts(&plan.project.id)
                .map_err(IndexError::Repository)?,
            project: plan.project,
            discovered_files: plan.candidates.len(),
            new_files: changes.new_files,
            changed_files: changes.changed_files,
            deleted_files: changes.deleted_files,
            unchanged_files: changes.unchanged_files,
            parsed_files: 0,
            reused_files: changes.unchanged_files,
            diagnostics: Vec::new(),
            warnings,
        })
    }

    fn rejected_index_result(
        &self,
        plan: IndexPlan,
        parsed: ParsedIndex,
        warnings: Vec<String>,
    ) -> Result<IndexResult, IndexError> {
        let changes = plan.changes;
        Ok(IndexResult {
            status: IndexStatus::RejectedSyntax,
            counts: self
                .repository
                .counts(&plan.project.id)
                .map_err(IndexError::Repository)?,
            project: plan.project,
            discovered_files: changes.discovered_paths.len(),
            new_files: changes.new_files,
            changed_files: changes.changed_files,
            deleted_files: changes.deleted_files,
            unchanged_files: changes.unchanged_files,
            parsed_files: parsed.parsed_files,
            reused_files: plan.existing_files.len(),
            diagnostics: parsed.diagnostics,
            warnings,
        })
    }

    fn commit_index(
        &mut self,
        mut plan: IndexPlan,
        mut parsed: ParsedIndex,
        warnings: Vec<String>,
    ) -> Result<IndexResult, IndexError> {
        let records = plan
            .candidates
            .into_iter()
            .map(|candidate| candidate.record)
            .collect::<Vec<_>>();
        let (files, nodes, edges) =
            self.assemble_project_graph(&plan.project, records, &mut parsed.parsed_by_path)?;
        self.ensure_not_cancelled()?;
        plan.project.generation = self
            .repository
            .replace_project_graph(&plan.project, files, nodes, edges)
            .map_err(IndexError::Repository)?;
        goldeneye_crosslink::rebuild(&mut self.repository)?;
        let counts = self
            .repository
            .counts(&plan.project.id)
            .map_err(IndexError::Repository)?;
        let changes = plan.changes;
        Ok(IndexResult {
            status: IndexStatus::Indexed,
            project: plan.project,
            discovered_files: changes.discovered_paths.len(),
            new_files: changes.new_files,
            changed_files: changes.changed_files,
            deleted_files: changes.deleted_files,
            unchanged_files: changes.unchanged_files,
            parsed_files: parsed.parsed_files,
            reused_files: changes.unchanged_files,
            counts,
            diagnostics: parsed.diagnostics,
            warnings,
        })
    }
}
