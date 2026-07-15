use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use goldeneye_domain::{Generation, ProjectId};

use crate::types::QueryError;

use super::ProjectGraph;

#[derive(Default)]
pub struct QueryCache {
    graphs: Mutex<BTreeMap<ProjectId, Arc<ProjectGraph>>>,
}

impl QueryCache {
    /// Drops one project's cached graph even when its durable generation is unchanged.
    pub fn invalidate_project(&self, project: &ProjectId) {
        self.graphs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(project);
    }

    /// Drops every cached project graph after a multi-project derived-graph write.
    pub fn invalidate_all(&self) {
        self.graphs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    pub(super) fn get(
        &self,
        project: &ProjectId,
        generation: Generation,
    ) -> Option<Arc<ProjectGraph>> {
        self.graphs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(project)
            .filter(|graph| graph.generation == generation.value())
            .map(Arc::clone)
    }

    pub(super) fn get_or_load(
        &self,
        project: &ProjectId,
        generation: Generation,
        mut load: impl FnMut() -> Result<
            (
                Vec<goldeneye_domain::GraphNode>,
                Vec<goldeneye_domain::GraphEdge>,
            ),
            QueryError,
        >,
    ) -> Result<Arc<ProjectGraph>, QueryError> {
        let mut graphs = self
            .graphs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(graph) = graphs
            .get(project)
            .filter(|graph| graph.generation == generation.value())
        {
            return Ok(Arc::clone(graph));
        }
        let (nodes, edges) = load()?;
        let graph = Arc::new(ProjectGraph::new(generation, nodes, edges));
        graphs.insert(project.clone(), Arc::clone(&graph));
        Ok(graph)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, sync::Arc};

    use goldeneye_domain::{Generation, ProjectId};

    use crate::types::{SearchGraphPage, SearchGraphRequest};

    use super::QueryCache;
    use crate::engine::{ProjectGraph, search::SearchCacheKey};

    #[test]
    fn graph_cache_reuses_one_generation_and_reloads_the_next() {
        let cache = QueryCache::default();
        let project = ProjectId::new("demo").expect("project ID");
        let loads = Cell::new(0_u8);
        let mut load = || {
            loads.set(loads.get() + 1);
            Ok((Vec::new(), Vec::new()))
        };

        let first = cache
            .get_or_load(&project, Generation::new(1), &mut load)
            .expect("first graph load");
        let reused = cache
            .get_or_load(&project, Generation::new(1), &mut load)
            .expect("cached graph load");
        let replaced = cache
            .get_or_load(&project, Generation::new(2), &mut load)
            .expect("replacement graph load");

        assert!(Arc::ptr_eq(&first, &reused));
        assert!(!Arc::ptr_eq(&first, &replaced));
        assert!(std::ptr::eq(
            first.architecture_summary(),
            reused.architecture_summary()
        ));
        assert!(!std::ptr::eq(
            first.architecture_summary(),
            replaced.architecture_summary()
        ));
        assert_eq!(loads.get(), 2);
    }

    #[test]
    fn graph_cache_invalidation_reloads_an_unchanged_generation() {
        let cache = QueryCache::default();
        let first_project = ProjectId::new("first").expect("project ID");
        let second_project = ProjectId::new("second").expect("project ID");
        let loads = Cell::new(0_u8);
        let mut load = || {
            loads.set(loads.get() + 1);
            Ok((Vec::new(), Vec::new()))
        };

        cache
            .get_or_load(&first_project, Generation::new(1), &mut load)
            .expect("first graph load");
        cache.invalidate_project(&first_project);
        cache
            .get_or_load(&first_project, Generation::new(1), &mut load)
            .expect("invalidated graph reload");
        cache
            .get_or_load(&second_project, Generation::new(1), &mut load)
            .expect("second graph load");
        cache.invalidate_all();
        cache
            .get_or_load(&first_project, Generation::new(1), &mut load)
            .expect("all-invalidated first reload");
        cache
            .get_or_load(&second_project, Generation::new(1), &mut load)
            .expect("all-invalidated second reload");

        assert_eq!(loads.get(), 5);
    }

    #[test]
    fn search_page_cache_is_scoped_to_one_project_graph_generation() {
        let graph = ProjectGraph::new(Generation::new(1), Vec::new(), Vec::new());
        let mut request = SearchGraphRequest::new(ProjectId::new("demo").expect("project ID"));
        request.query = Some("fs search".to_owned());
        let key = SearchCacheKey::from(&request);
        let page = SearchGraphPage {
            project: "demo".to_owned(),
            results: Vec::new(),
            total: 0,
            has_more: false,
            next_cursor: None,
        };

        assert_eq!(graph.cached_search(&key), None);
        graph.cache_search(key.clone(), page.clone());
        assert_eq!(graph.cached_search(&key), Some(page));

        let next_generation = ProjectGraph::new(Generation::new(2), Vec::new(), Vec::new());
        assert_eq!(next_generation.cached_search(&key), None);
    }
}
