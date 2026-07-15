use super::{
    Arc, BTreeMap, BTreeSet, Candidate, ContentHash, ExtractedFile, FileGraph, FileId, FileRecord,
    Generation, IndexError, IndexRepository, IndexService, IndexSyntaxExtractor, ProjectId,
    ProjectRelativePath, RepositorySourceFile, UNIX_EPOCH, fs, relative_path,
};

impl<R> IndexService<R>
where
    R: IndexRepository,
{
    pub(super) fn reuse_file_graph(&self, file: &FileId) -> Result<FileGraph, IndexError> {
        let mut nodes = self
            .repository
            .nodes_for_file(file)
            .map_err(IndexError::Repository)?;
        let mut node_ids = nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<BTreeSet<_>>();
        let mut identities = BTreeSet::new();
        let mut edges = Vec::new();
        for node in &nodes {
            for edge in self
                .repository
                .edges_from(&file.project, &node.id)
                .map_err(IndexError::Repository)?
            {
                if !node_ids.contains(&edge.source) {
                    continue;
                }
                let identity = (
                    edge.source.clone(),
                    edge.target.clone(),
                    edge.kind.clone(),
                    edge.discriminator.clone(),
                );
                if identities.insert(identity) {
                    edges.push(edge);
                }
            }
        }
        let referenced_modules = edges
            .iter()
            .filter(|edge| edge.kind.as_str() == "DEFINES" && !node_ids.contains(&edge.target))
            .map(|edge| edge.target.clone())
            .collect::<BTreeSet<_>>();
        for module_id in referenced_modules {
            let Some(module) = self
                .repository
                .get_node(&file.project, &module_id)
                .map_err(IndexError::Repository)?
            else {
                continue;
            };
            if module.label.as_str() != "Module" || !node_ids.insert(module.id.clone()) {
                continue;
            }
            nodes.push(module);
            for edge in self
                .repository
                .edges_from(&file.project, &module_id)
                .map_err(IndexError::Repository)?
            {
                if !node_ids.contains(&edge.target) {
                    continue;
                }
                let identity = (
                    edge.source.clone(),
                    edge.target.clone(),
                    edge.kind.clone(),
                    edge.discriminator.clone(),
                );
                if identities.insert(identity) {
                    edges.push(edge);
                }
            }
        }
        Ok(FileGraph { nodes, edges })
    }

    pub(super) fn parse_matching_records(
        &self,
        discovered: &[RepositorySourceFile],
        records: &[FileRecord],
        project: &ProjectId,
        excluded: Option<&ProjectRelativePath>,
    ) -> Result<BTreeMap<ProjectRelativePath, ExtractedFile>, IndexError> {
        let discovered_by_path = discovered
            .iter()
            .filter_map(|file| {
                relative_path(&file.relative_path)
                    .ok()
                    .map(|path| (path, file))
            })
            .collect::<BTreeMap<_, _>>();
        let records_by_path = records
            .iter()
            .map(|record| (record.id.path.clone(), record))
            .collect::<BTreeMap<_, _>>();
        let mut candidates = Vec::new();
        for (path, record) in records_by_path {
            if excluded.is_some_and(|excluded| excluded == &path) {
                continue;
            }
            let Some(discovered) = discovered_by_path.get(&path) else {
                continue;
            };
            let candidate = Self::read_candidate(discovered, project)?;
            if candidate.record.content_hash == record.content_hash {
                candidates.push(candidate);
            }
        }
        let mut parsed = BTreeMap::new();
        for extracted in self.parse_candidates(candidates)? {
            if extracted.diagnostics.is_none() {
                parsed.insert(extracted.record.id.path.clone(), extracted);
            }
        }
        Ok(parsed)
    }

    pub(super) fn read_candidates(
        &self,
        files: &[RepositorySourceFile],
        project: &ProjectId,
    ) -> Result<Vec<Candidate>, IndexError> {
        let supported_ids = self.extractor.supported_ids();
        files
            .iter()
            .filter(|file| supported_ids.contains(&file.language))
            .map(|file| {
                self.ensure_not_cancelled()?;
                Self::read_candidate(file, project)
            })
            .collect()
    }

    pub(super) fn read_candidate(
        file: &RepositorySourceFile,
        project: &ProjectId,
    ) -> Result<Candidate, IndexError> {
        let source = fs::read(&file.absolute_path).map_err(|source| IndexError::Io {
            path: file.absolute_path.clone(),
            source,
        })?;
        let metadata = fs::metadata(&file.absolute_path).map_err(|source| IndexError::Io {
            path: file.absolute_path.clone(),
            source,
        })?;
        let modified = metadata.modified().map_err(|source| IndexError::Io {
            path: file.absolute_path.clone(),
            source,
        })?;
        let modified_ns = modified
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let modified_ns = u64::try_from(modified_ns).map_err(|_| IndexError::MetadataOverflow {
            path: file.absolute_path.clone(),
            field: "modified_ns",
        })?;
        let byte_len = u64::try_from(source.len()).map_err(|_| IndexError::MetadataOverflow {
            path: file.absolute_path.clone(),
            field: "byte_len",
        })?;
        let path = relative_path(&file.relative_path)?;
        Ok(Candidate {
            record: FileRecord::new(
                FileId::new(project.clone(), path),
                ContentHash::of(&source),
                Generation::new(0),
                modified_ns,
                byte_len,
            ),
            language: file.language.clone(),
            source: Arc::from(source),
        })
    }
}
