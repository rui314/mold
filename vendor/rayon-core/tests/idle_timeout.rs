use rayon_core::{broadcast, join, scope, ThreadPoolBuilder};
use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;

#[test]
fn bursts_nested_work_broadcast_and_shutdown() {
    for threads in [1, 2, 4] {
        for timeout in [Duration::ZERO, Duration::from_millis(2)] {
            let (finished, receive) = mpsc::channel();
            std::thread::spawn(move || {
                let (exited, exits) = mpsc::channel();
                let pool = ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .idle_timeout(timeout)
                    .exit_handler(move |_| {
                        exited.send(()).unwrap();
                    })
                    .build()
                    .unwrap();
                for _ in 0..8 {
                    let barrier = Arc::new(Barrier::new(threads));
                    let values = pool.install(|| {
                        broadcast(|ctx| {
                            barrier.wait();
                            {
                                let index = ctx.index();
                                join(move || index + 1, || 7)
                            }
                        })
                    });
                    assert_eq!(values.len(), threads);
                    assert_eq!(
                        values.iter().map(|x| x.0).sum::<usize>(),
                        threads * (threads + 1) / 2
                    );
                    pool.install(|| {
                        scope(|s| {
                            let values = &values;
                            s.spawn(move |_| assert!(values.iter().all(|x| x.1 == 7)));
                        })
                    });
                    // Exercise both the polling period and wake-up after parking.
                    std::thread::sleep(Duration::from_millis(8));
                }
                drop(pool);
                for _ in 0..threads {
                    exits.recv_timeout(Duration::from_secs(5)).unwrap();
                }
                finished.send(()).unwrap();
            });
            receive.recv_timeout(Duration::from_secs(10)).unwrap();
        }
    }
}
