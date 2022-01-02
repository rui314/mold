.. _Task-Based_Programming:

Task-Based Programming
======================


When striving for performance, programming in terms of threads can be a
poor way to do multithreaded programming. It is much better to formulate
your program in terms of *logical tasks*, not threads, for several
reasons.


-  Matching parallelism to available resources


-  Faster task startup and shutdown


-  More efficient evaluation order


-  Improved load balancing


-  Higher–level thinking


The following paragraphs explain these points in detail.


The threads you create with a threading package are *logical* threads,
which map onto the *physical threads* of the hardware. For computations
that do not wait on external devices, highest efficiency usually occurs
when there is exactly one running logical thread per physical thread.
Otherwise, there can be inefficiencies from the mismatch\ *.
Undersubscription* occurs when there are not enough running logical
threads to keep the physical threads working. *Oversubscription* occurs
when there are more running logical threads than physical threads.
Oversubscription usually leads to *time sliced* execution of logical
threads, which incurs overheads as discussed in Appendix A, *Costs of
Time Slicing*. The scheduler tries to avoid oversubscription, by having
one logical thread per physical thread, and mapping tasks to logical
threads, in a way that tolerates interference by other threads from the
same or other processes.


The key advantage of tasks versus logical threads is that tasks are much
*lighter weight* than logical threads. On Linux systems, starting and
terminating a task is about 18 times faster than starting and
terminating a thread. On Windows systems, the ratio is more than 100.
This is because a thread has its own copy of a lot of resources, such as
register state and a stack. On Linux, a thread even has its own process
id. A task in |full_name|, in contrast, is
typically a small routine, and also, cannot be preempted at the task
level (though its logical thread can be preempted).


Tasks in oneTBB are efficient too because *the scheduler is unfair*. Thread schedulers typically
distribute time slices in a round-robin fashion. This distribution is
called "fair", because each logical thread gets its fair share of time.
Thread schedulers are typically fair because it is the safest strategy
to undertake without understanding the higher-level organization of a
program. In task-based programming, the task scheduler does have some
higher-level information, and so can sacrifice fairness for efficiency.
Indeed, it often delays starting a task until it can make useful
progress.


The scheduler does *load balancing*. In addition to using the right
number of threads, it is important to distribute work evenly across
those threads. As long as you break your program into enough small
tasks, the scheduler usually does a good job of assigning tasks to
threads to balance load. With thread-based programming, you are often
stuck dealing with load-balancing yourself, which can be tricky to get
right.


.. tip:: 
   Design your programs to try to create many more tasks than there are
   threads, and let the task scheduler choose the mapping from tasks to
   threads.


Finally, the main advantage of using tasks instead of threads is that
they let you think at a higher, task-based, level. With thread-based
programming, you are forced to think at the low level of physical
threads to get good efficiency, because you have one logical thread per
physical thread to avoid undersubscription or oversubscription. You also
have to deal with the relatively coarse grain of threads. With tasks,
you can concentrate on the logical dependences between tasks, and leave
the efficient scheduling to the scheduler.

