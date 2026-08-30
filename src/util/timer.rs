//! Wall-clock and CPU time accounting for `--perf`.

use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Debug)]
struct Record {
    name: String,
    start: Instant,
    end: Option<Instant>,
    user_start: f64,
    sys_start: f64,
    user: f64,
    sys: f64,
    parent: Option<usize>,
    children: Vec<usize>,
}

/// Collects timing records for the passes of a link. Cloning shares the
/// underlying records.
#[derive(Clone, Debug, Default)]
pub struct Timers {
    records: Arc<Mutex<Vec<Record>>>,
}

/// A running timer, stopped when dropped.
pub struct Timer {
    timers: Timers,
    index: usize,
    stopped: bool,
}

/// A clonable handle for starting timers beneath a running timer.
#[derive(Clone)]
pub struct TimerHandle {
    timers: Timers,
    parent: usize,
}

fn rusage() -> (f64, f64) {
    // SAFETY: `usage` is a valid, writable rusage struct.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    let to_secs = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1_000_000.0;
    (to_secs(usage.ru_utime), to_secs(usage.ru_stime))
}

impl Timers {
    pub fn new() -> Self {
        Timers::default()
    }

    /// Starts a timer, nested in the timer still running that started last.
    pub fn start(&self, name: &str) -> Timer {
        let records = self.records.lock().unwrap();
        let parent = records.iter().rposition(|r| r.end.is_none());
        drop(records);
        self.start_child(name, parent)
    }

    fn start_child(&self, name: &str, parent: Option<usize>) -> Timer {
        let (user, sys) = rusage();
        let mut records = self.records.lock().unwrap();
        let index = records.len();
        records.push(Record {
            name: name.to_string(),
            start: Instant::now(),
            end: None,
            user_start: user,
            sys_start: sys,
            user: 0.0,
            sys: 0.0,
            parent,
            children: Vec::new(),
        });
        if let Some(parent) = parent {
            records[parent].children.push(index);
        }
        Timer {
            timers: self.clone(),
            index,
            stopped: false,
        }
    }

    fn stop(&self, index: usize) {
        let (user, sys) = rusage();
        let mut records = self.records.lock().unwrap();
        let record = &mut records[index];
        if record.end.is_none() {
            record.end = Some(Instant::now());
            record.user = user - record.user_start;
            record.sys = sys - record.sys_start;
        }
    }

    pub fn print(&self) {
        let mut records = self.records.lock().unwrap();
        let now = Instant::now();
        for r in records.iter_mut() {
            if r.end.is_none() {
                r.end = Some(now);
            }
        }

        fn print_rec(records: &[Record], i: usize, indent: usize) {
            let r = &records[i];
            let real = r.end.unwrap().duration_since(r.start).as_secs_f64();
            println!(
                " {:8.3} {:8.3} {:8.3}  {}{}",
                r.user,
                r.sys,
                real,
                " ".repeat(indent * 2),
                r.name
            );
            let mut children = r.children.clone();
            children.sort_by_key(|&c| records[c].start);
            for child in children {
                print_rec(records, child, indent + 1);
            }
        }

        println!("     User   System     Real  Name");
        for i in 0..records.len() {
            if records[i].parent.is_none() {
                print_rec(&records, i, 0);
            }
        }
    }
}

impl Timer {
    /// Starts a timer nested in this one. Timers started from parallel
    /// tasks name their parent this way, since which timer started last
    /// says nothing about nesting then.
    pub fn child(&self, name: &str) -> Timer {
        self.timers.start_child(name, Some(self.index))
    }

    pub fn handle(&self) -> TimerHandle {
        TimerHandle {
            timers: self.timers.clone(),
            parent: self.index,
        }
    }

    pub fn stop(&mut self) {
        if !self.stopped {
            self.stopped = true;
            self.timers.stop(self.index);
        }
    }
}

impl TimerHandle {
    pub fn child(&self, name: &str) -> Timer {
        self.timers.start_child(name, Some(self.parent))
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        self.stop();
    }
}
