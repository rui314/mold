.. _appendix_A:

Appendix A Costs of Time Slicing
================================


Time slicing enables there to be more logical threads than physical
threads. Each logical thread is serviced for a *time slice* by a
physical thread. If a thread runs longer than a time slice, as most do,
it relinquishes the physical thread until it gets another turn. This
appendix details the costs incurred by time slicing.


The most obvious is the time for *context switching* between logical
threads. Each context switch requires that the processor save all its
registers for the previous logical thread that it was executing, and
load its registers for the next logical thread that it runs.


A more subtle cost is *cache cooling*. Processors keep recently accessed
data in cache memory, which is very fast, but also relatively small
compared to main memory. When the processor runs out of cache memory, it
has to evict items from cache and put them back into main memory.
Typically, it chooses the least recently used items in the cache. (The
reality of set-associative caches is a bit more complicated, but this is
not a cache primer.) When a logical thread gets its time slice, as it
references a piece of data for the first time, this data will be pulled
into cache, taking hundreds of cycles. If it is referenced frequently
enough to not be evicted, each subsequent reference will find it in
cache, and only take a few cycles. Such data is called "hot in cache".
Time slicing undoes this, because if a thread A finishes its time slice,
and subsequently thread B runs on the same physical thread, B will tend
to evict data that was hot in cache for A, unless both threads need the
data. When thread A gets its next time slice, it will need to reload
evicted data, at the cost of hundreds of cycles for each cache miss. Or
worse yet, the next time slice for thread A may be on a different
physical thread that has a different cache altogether.


Another cost is *lock preemption.* This happens if a thread acquires a
lock on a resource, and its time slice runs out before it releases the
lock. No matter how short a time the thread intended to hold the lock,
it is now going to hold it for at least as long as it takes for its next
turn at a time slice to come up. Any other threads waiting on the lock
either pointlessly busy-wait, or lose the rest of their time slice. The
effect is called *convoying*, because the threads end up "bumper to
bumper" waiting for the preempted thread in front to resume driving.

