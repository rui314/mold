.. _Task_Scheduler_Bypass:

Task Scheduler Bypass
=====================

Scheduler bypass is an optimization where you directly specify the next task to run. 
According to the rules of execution described in :doc:`How Task Scheduler Works <How_Task_Scheduler_Works>`, 
the spawning of the new task to be executed by the current thread involves the next steps:

 -  Push a new task onto the thread's deque.
 -  Continue to execute the current task until it is completed.
 -  Take a task from the thread's deque, unless it is stolen by another thread.

Steps 1 and 3 introduce unnecessary deque operations or, even worse, allow stealing that can hurt 
locality without adding significant parallelism. These problems can be avoided by using "Task Scheduler Bypass" technique to directly point the preferable task to be executed next 
instead of spawning it. When, as described in :doc:`How Task Scheduler Works <How_Task_Scheduler_Works>`,
the returned task becomes the first candidate for the next task to be executed by the thread. Furthermore, this approach almost guarantees that 
the task is executed by the current thread and not by any other thread.

Please note that at the moment the only way to use this optimization is to use `preview feature of ``onepai::tbb::task_group`` 