.. _Mutex_Flavors:

Mutex Flavors
=============


Connoisseurs of mutexes distinguish various attributes of mutexes. It
helps to know some of these, because they involve tradeoffs of
generality and efficiency. Picking the right one often helps
performance. Mutexes can be described by the following qualities, also
summarized in the table below.


-  **Scalable**. Some mutexes are called *scalable*. In a strict sense,
   this is not an accurate name, because a mutex limits execution to one
   thread at a time. A *scalable mutex* is one that does not do *worse*
   than this. A mutex can do worse than serialize execution if the
   waiting threads consume excessive processor cycles and memory
   bandwidth, reducing the speed of threads trying to do real work.
   Scalable mutexes are often slower than non-scalable mutexes under
   light contention, so a non-scalable mutex may be better. When in
   doubt, use a scalable mutex.


-  **Fair**. Mutexes can be *fair* or *unfair*. A fair mutex lets
   threads through in the order they arrived. Fair mutexes avoid
   starving threads. Each thread gets its turn. However, unfair mutexes
   can be faster, because they let threads that are running go through
   first, instead of the thread that is next in line which may be
   sleeping on account of an interrupt.


-  **Yield or Block**. This is an implementation detail that impacts
   performance. On long waits, an |full_name|
   mutex either *yields* or *blocks*. Here *yields* means to
   repeatedly poll whether progress can be made, and if not, temporarily
   yield [#]_ the processor. To *block* means to yield the
   processor until the mutex permits progress. Use the yielding mutexes
   if waits are typically short and blocking mutexes if waits are
   typically long.


The following is a summary of mutex behaviors:


-  ``spin_mutex`` is non-scalable, unfair, non-recursive, and spins in
   user space. It would seem to be the worst of all possible worlds,
   except that it is *very fast* in *lightly contended* situations. If
   you can design your program so that contention is somehow spread out
   among many ``spin_mutex`` objects, you can improve performance over
   using other kinds of mutexes. If a mutex is heavily contended, your
   algorithm will not scale anyway. Consider redesigning the algorithm
   instead of looking for a more efficient lock.


-  ``mutex`` has behavior similar to the ``spin_mutex``. However,
   the ``mutex`` *blocks* on long waits that makes it
   resistant to high contention.


-  ``queuing_mutex`` is scalable, fair, non-recursive, and spins in user
   space. Use it when scalability and fairness are important.


-  ``spin_rw_mutex`` and ``queuing_rw_mutex`` are similar to
   ``spin_mutex`` and ``queuing_mutex``, but additionally support
   *reader* locks.


-  ``rw_mutex`` is similar to ``mutex``, but additionally support
   *reader* locks.


-  ``speculative_spin_mutex`` and ``speculative_spin_rw_mutex`` are
   similar to ``spin_mutex`` and ``spin_rw_mutex``, but additionally
   provide *speculative locking* on processors that support hardware
   transaction memory. Speculative locking allows multiple threads
   acquire the same lock, as long as there are no "conflicts" that may
   generate different results than non-speculative locking. These
   mutexes are *scalable* when work with low conflict rate, i.e. mostly
   in speculative locking mode.


-  ``null_mutex`` and ``null_rw_mutex`` do nothing. They can be useful
   as template arguments. For example, suppose you are defining a
   container template and know that some instantiations will be shared
   by multiple threads and need internal locking, but others will be
   private to a thread and not need locking. You can define the template
   to take a Mutex type parameter. The parameter can be one of the real
   mutex types when locking is necessary, and ``null_mutex`` when
   locking is unnecessary.


.. container:: tablenoborder


   .. list-table:: 
      :header-rows: 1

      * -     Mutex     
        -     Scalable     
        -     Fair     
        -     Recursive     
        -     Long Wait     
        -     Size     
      * -     \ ``spin_mutex``     
        -     no     
        -     no     
        -     no     
        -     yields     
        -     1 byte     
      * -     \ ``mutex``     
        -     ✓     
        -     no     
        -     no     
        -     blocks     
        -     1 byte     
      * -     \ ``speculative_spin_mutex``     
        -     HW dependent     
        -     no     
        -     no     
        -     yields     
        -     2 cache lines     
      * -     \ ``queuing_mutex``     
        -     ✓     
        -     ✓     
        -     no     
        -     yields     
        -     1 word     
      * -     \ ``spin_rw_mutex``     
        -     no     
        -     no     
        -     no     
        -     yields     
        -     1 word     
      * -     \ ``spin_rw_mutex``     
        -     ✓     
        -     no     
        -     no     
        -     blocks     
        -     1 word     
      * -     \ ``speculative_spin_rw_mutex``     
        -     HW dependent     
        -     no     
        -     no     
        -     yields     
        -     3 cache lines     
      * -     \ ``queuing_rw_mutex``     
        -     ✓     
        -     ✓     
        -     no     
        -     yields     
        -     1 word     
      * -     \ ``null_mutex`` [#]_   
        -     moot     
        -     ✓     
        -     ✓     
        -     never     
        -     empty     
      * -     \ ``null_rw_mutex``     
        -     moot     
        -     ✓     
        -     ✓     
        -     never     
        -     empty     




.. [#] The yielding is implemented via ``SwitchToThread()`` on Microsoft
       Windows\* operating systems and by ``sched_yield()`` on other systems.


.. [#] Null mutexes are considered fair by oneTBB because they cannot cause
       starvation. They lack any non-static data members.

