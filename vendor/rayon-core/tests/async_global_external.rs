use rayon_core::{broadcast, current_num_threads, join, ThreadPoolBuilder};
use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;

#[test]
fn caller_outside_pool_can_submit_work_after_async_initialization() {
    let (done, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let release = Arc::new(Barrier::new(3));
        let workers = release.clone();
        ThreadPoolBuilder::new()
            .num_threads(2)
            .start_handler(move |_| {
                workers.wait();
            })
            .build_global_async()
            .unwrap();
        assert_eq!(current_num_threads(), 2);
        release.wait();
        assert_eq!(join(|| 19, || 23), (19, 23));
        assert_eq!(broadcast(|ctx| ctx.index()), vec![0, 1]);
        done.send(()).unwrap();
    });
    receive.recv_timeout(Duration::from_secs(10)).unwrap();
}
