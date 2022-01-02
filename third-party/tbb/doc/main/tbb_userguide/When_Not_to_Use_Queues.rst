.. _When_Not_to_Use_Queues:

When Not to Use Queues
======================


Queues are widely used in parallel programs to buffer consumers from
producers. Before using an explicit queue, however, consider using
``parallel_for_each`` ``parallel_pipeline`` instead. These is often more
efficient than queues for the following reasons:


-  A queue is inherently a bottle neck, because it must maintain
   first-in first-out order.


-  A thread that is popping a value may have to wait idly until the
   value is pushed.


-  A queue is a passive data structure. If a thread pushes a value, it
   could take time until it pops the value, and in the meantime the
   value (and whatever it references) becomes "cold" in cache. Or worse
   yet, another thread pops the value, and the value (and whatever it
   references) must be moved to the other processor.


In contrast, ``parallel_pipeline`` avoids these bottlenecks. Because its
threading is implicit, it optimizes use of worker threads so that they
do other work until a value shows up. It also tries to keep items hot in
cache.

