.. _When_Task-Based_Programming_Is_Inappropriate:

When Task-Based Programming Is Inappropriate
============================================


Using the task scheduler is usually the best approach to threading for
performance, however there are cases when the task scheduler is not
appropriate. The task scheduler is intended for high-performance
algorithms composed from non-blocking tasks. It still works if the tasks
rarely block. However, if threads block frequently, there is a
performance loss when using the task scheduler because while the thread
is blocked, it is not working on any tasks. Blocking typically occurs
while waiting for I/O or mutexes for long periods. If threads hold
mutexes for long periods, your code is not likely to perform well
anyway, no matter how many threads it has. If you have blocking tasks,
it is best to use full-blown threads for those. The task scheduler is
designed so that you can safely mix your own threads with |full_name| tasks.

