use rayon::prelude::*;
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// An owned Rayon job whose result is needed by a later linker pass.
pub struct Background<T> {
    receiver: std::sync::mpsc::Receiver<T>,
    name: &'static str,
}

impl<T: Send + 'static> Background<T> {
    pub fn spawn(name: &'static str, run: impl FnOnce() -> T + Send + 'static) -> Self {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        rayon::spawn(move || {
            let _ = sender.send(run());
        });
        Self { receiver, name }
    }

    pub fn join(self) -> T {
        use std::sync::mpsc::TryRecvError;
        loop {
            match self.receiver.try_recv() {
                Ok(value) => return value,
                Err(TryRecvError::Disconnected) => panic!("{} task failed", self.name),
                Err(TryRecvError::Empty) => {
                    // A blocking receive would deadlock a one-worker pool.
                    if !matches!(rayon::yield_now(), Some(rayon::Yield::Executed)) {
                        std::thread::yield_now();
                    }
                }
            }
        }
    }
}

/// Keeps idle workers polling for work between passes while it is alive.
///
/// An idle Rayon worker yields a few dozen times and then sleeps, and a
/// parallel loop wakes sleeping workers one at a time as it splits, so a
/// loop that starts while the pool sleeps has all of its threads running
/// only after several wake-ups in a row. The linker runs dozens of short
/// parallel passes with serial code in between, which is long enough for
/// the workers to fall asleep before nearly every pass. Workers that keep
/// polling start on the next pass at once, as TBB's do in C++ mold.
///
/// A polling worker yields the CPU whenever it finds no work and returns
/// to Rayon, which puts it to sleep, after `IDLE_LIMIT` without work, so a
/// long serial stretch does not keep the CPUs busy. [`rearm`] at the start
/// of the next pass wakes all of them at once.
pub struct KeepWarm {
    generation: usize,
}

const IDLE_LIMIT: Duration = Duration::from_millis(2);

// Advances when a KeepWarm starts or stops. A polling loop ends when it
// changes.
static GENERATION: AtomicUsize = AtomicUsize::new(0);
static POLLING: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    // Set on the thread that runs the passes, which never polls.
    static IS_OWNER: Cell<bool> = const { Cell::new(false) };
    static IS_POLLING: Cell<bool> = const { Cell::new(false) };
}

impl KeepWarm {
    /// Starts keeping the pool warm. Call this on the pool thread that runs
    /// the passes.
    pub fn start() -> Self {
        let generation = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
        IS_OWNER.set(true);
        broadcast_poll(generation);
        Self { generation }
    }
}

impl Drop for KeepWarm {
    fn drop(&mut self) {
        IS_OWNER.set(false);
        GENERATION.store(self.generation + 1, Ordering::Relaxed);
    }
}

/// Wakes the workers that stopped polling for work. A pass calls this as it
/// starts; it costs a few loads when all workers are polling.
pub fn rearm() {
    if IS_OWNER.get() && POLLING.load(Ordering::Relaxed) + 1 < rayon::current_num_threads() {
        broadcast_poll(GENERATION.load(Ordering::Relaxed));
    }
}

fn broadcast_poll(generation: usize) {
    rayon::spawn_broadcast(move |_| {
        // A worker that is polling already picks up its own new job through
        // yield_now.
        if IS_OWNER.get() || IS_POLLING.get() {
            return;
        }
        IS_POLLING.set(true);
        POLLING.fetch_add(1, Ordering::Relaxed);
        let mut last_work = Instant::now();
        while GENERATION.load(Ordering::Relaxed) == generation {
            if matches!(rayon::yield_now(), Some(rayon::Yield::Executed)) {
                last_work = Instant::now();
            } else if last_work.elapsed() > IDLE_LIMIT {
                break;
            } else {
                std::thread::yield_now();
            }
        }
        POLLING.fetch_sub(1, Ordering::Relaxed);
        IS_POLLING.set(false);
    });
}

/// Stably partitions a slice, returning the number of matching elements.
/// Each block scatters into disjoint ranges computed from its match count.
pub fn stable_partition<T: Copy + Send + Sync>(
    values: &mut [T],
    pred: impl Fn(&T) -> bool + Sync,
) -> usize {
    const BLOCK: usize = 16384;
    let matches: Vec<bool> = if values.len() <= BLOCK {
        values.iter().map(&pred).collect()
    } else {
        values.par_iter().map(&pred).collect()
    };
    let counts: Vec<usize> =
        matches.par_chunks(BLOCK).map(|chunk| chunk.iter().filter(|&&v| v).count()).collect();
    let num_matches = counts.iter().sum();
    if num_matches == 0 || num_matches == values.len() {
        return num_matches;
    }

    let mut output = values.to_vec();
    let (mut yes, mut no) = output.split_at_mut(num_matches);
    let parts: Vec<_> = counts
        .iter()
        .zip(matches.chunks(BLOCK))
        .map(|(&count, flags)| {
            let matched = yes.split_off_mut(..count).unwrap();

            let unmatched = no.split_off_mut(..flags.len() - count).unwrap();

            (matched, unmatched)
        })
        .collect();
    parts.into_par_iter().zip(values.par_chunks(BLOCK)).zip(matches.par_chunks(BLOCK)).for_each(
        |(((yes, no), input), flags)| {
            let (mut y, mut n) = (0, 0);
            for (&value, &matched) in input.iter().zip(flags) {
                if matched {
                    yes[y] = value;
                    y += 1;
                } else {
                    no[n] = value;
                    n += 1;
                }
            }
        },
    );
    values
        .par_chunks_mut(BLOCK)
        .zip(output.par_chunks(BLOCK))
        .for_each(|(dst, src)| dst.copy_from_slice(src));
    num_matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_job_joins_on_one_worker() {
        rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap().install(|| {
            let job = Background::spawn("test", || (0..100usize).into_par_iter().sum::<usize>());
            assert_eq!(job.join(), 4950);
        });
    }

    #[test]
    fn parallel_work_finishes_while_workers_are_kept_warm() {
        for threads in [1, 2, 4] {
            rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap().install(|| {
                let warm = KeepWarm::start();
                let job =
                    Background::spawn("test", || (0..100usize).into_par_iter().sum::<usize>());
                assert_eq!((0..10000usize).into_par_iter().sum::<usize>(), 49995000);
                // Let the workers go idle, then wake them for another loop.
                std::thread::sleep(IDLE_LIMIT * 2);
                rearm();
                assert_eq!((0..10000usize).into_par_iter().sum::<usize>(), 49995000);
                assert_eq!(job.join(), 4950);
                drop(warm);
            });
        }
    }

    #[test]
    fn preserves_both_groups_across_blocks() {
        for len in [0, 1, 16383, 16384, 16385, 65539] {
            for divisor in [1, 2, 7, 100000] {
                let mut values: Vec<_> = (0..len).collect();
                let (yes, no): (Vec<_>, Vec<_>) =
                    values.iter().copied().partition(|v| v % divisor == 0);
                let count = stable_partition(&mut values, |v| v % divisor == 0);
                assert_eq!(count, yes.len());
                assert_eq!(&values[..count], yes);
                assert_eq!(&values[count..], no);
                assert_eq!(stable_partition(&mut values, |_| false), 0);
            }
        }
    }
}
