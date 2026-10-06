use rayon_core::{
    broadcast, current_num_threads, current_thread_index, join, spawn, ThreadPoolBuilder,
};
use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;

#[test]
fn returns_before_workers_start_and_publishes_the_global_registry() {
    let (done, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let release = Arc::new(Barrier::new(4));
        let worker_release = release.clone();
        ThreadPoolBuilder::new()
            .num_threads(4)
            .use_current_thread()
            .thread_name(|i| format!("async-worker-{i}"))
            .stack_size(2 * 1024 * 1024)
            .start_handler(move |i| {
                assert!(i > 0);
                assert_eq!(
                    std::thread::current().name(),
                    Some(format!("async-worker-{i}").as_str())
                );
                worker_release.wait();
            })
            .build_global_async()
            .unwrap();
        assert_eq!(current_thread_index(), Some(0));
        assert_eq!(current_num_threads(), 4);
        // All other workers are still blocked. The caller can do parallel work.
        assert_eq!(join(|| 17, || 25), (17, 25));
        let (sent, received) = mpsc::channel();
        std::thread::spawn(move || {
            assert_eq!(current_thread_index(), None);
            assert_eq!(current_num_threads(), 4);
            spawn(move || sent.send(current_thread_index().unwrap()).unwrap());
        })
        .join()
        .unwrap();
        release.wait();
        assert!(received.recv_timeout(Duration::from_secs(5)).unwrap() < 4);
        assert_eq!(broadcast(|ctx| ctx.index()), vec![0, 1, 2, 3]);
        assert!(ThreadPoolBuilder::new().build_global_async().is_err());
        done.send(()).unwrap();
    });
    receive.recv_timeout(Duration::from_secs(10)).unwrap();
}
