use std::sync::{Mutex, MutexGuard};

// Keep independent workers' locks and buffers off the same cache line.
#[repr(align(128))]
struct Worker<T>(Mutex<T>);

/// Persistent scratch storage per Rayon worker, plus a slot for callers
/// outside the pool. Look up and lock once per file, not once per symbol.
pub struct WorkerLocal<T>(Vec<Worker<T>>);

impl<T> WorkerLocal<T> {
    pub fn new(init: impl Fn() -> T) -> Self {
        Self((0..=rayon::current_num_threads()).map(|_| Worker(Mutex::new(init()))).collect())
    }

    pub fn get(&self) -> MutexGuard<'_, T> {
        let fallback = self.0.len() - 1;
        let worker = rayon::current_thread_index().unwrap_or(fallback).min(fallback);
        self.0[worker].0.lock().unwrap()
    }

    pub fn into_values(self) -> impl Iterator<Item = T> {
        self.0.into_iter().map(|worker| worker.0.into_inner().unwrap())
    }

    pub fn take(&mut self) -> impl Iterator<Item = T> + '_
    where
        T: Default,
    {
        self.0.iter_mut().map(|worker| std::mem::take(worker.0.get_mut().unwrap()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rayon::prelude::*;

    #[test]
    fn reuses_worker_storage_and_keeps_an_outside_slot() {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(2).build().unwrap();
        let mut slots = pool.install(|| {
            let slots = WorkerLocal::new(Vec::new);
            for batch in 0..3 {
                (0..100).into_par_iter().for_each(|i| slots.get().push(batch * 100 + i));
            }
            slots
        });
        slots.get().push(300);
        let mut values: Vec<_> = slots.take().flatten().collect();
        values.sort_unstable();
        assert_eq!(values, (0..301).collect::<Vec<_>>());
        assert!(slots.into_values().all(|slot| slot.is_empty()));
    }
}
