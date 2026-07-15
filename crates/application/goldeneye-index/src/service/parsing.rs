use super::{
    Arc, AtomicUsize, Candidate, ExtractedFile, IndexError, IndexRepository, IndexService,
    IndexSyntaxExtractor, Ordering, mpsc,
};

impl<R> IndexService<R>
where
    R: IndexRepository,
{
    pub(super) fn parse_candidates(
        &self,
        candidates: Vec<Candidate>,
    ) -> Result<Vec<ExtractedFile>, IndexError> {
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        let candidates = Arc::new(candidates);
        let next = AtomicUsize::new(0);
        let (sender, receiver) = mpsc::channel();
        let workers = self.options.max_workers.get().min(candidates.len());
        let mode = self.options.discovery.mode;
        std::thread::scope(|scope| -> Result<(), IndexError> {
            let mut handles = Vec::with_capacity(workers);
            for _ in 0..workers {
                let sender = sender.clone();
                let candidates = Arc::clone(&candidates);
                let extractor = Arc::clone(&self.extractor);
                let cancellation = self.options.cancellation.clone();
                let next = &next;
                handles.push(scope.spawn(move || {
                    loop {
                        if cancellation.is_cancelled() {
                            break;
                        }
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(candidate) = candidates.get(index).cloned() else {
                            break;
                        };
                        let path = candidate.record.id.path.clone();
                        if sender
                            .send((index, path, extractor.extract(candidate, mode)))
                            .is_err()
                        {
                            break;
                        }
                    }
                }));
            }
            drop(sender);
            for handle in handles {
                handle.join().map_err(|_| IndexError::WorkerPanicked)?;
            }
            Ok(())
        })?;
        self.ensure_not_cancelled()?;
        let mut results = receiver.into_iter().collect::<Vec<_>>();
        results.sort_by_key(|(index, _, _)| *index);
        results
            .into_iter()
            .map(|(_, path, result)| result.map_err(|source| IndexError::Syntax { path, source }))
            .collect()
    }
}
