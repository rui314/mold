.. _How_Task_Scheduler_Works.rst:

How Task Scheduler Works
========================


While the task scheduler is not bound to any particular type of parallelism, 
it was designed to work efficiently for fork-join parallelism with lots of forks.
This type of parallelism is typical for parallel algorithms such as `oneapi::tbb::parallel_for
<https://spec.oneapi.io/versions/latest/elements/oneTBB/source/algorithms/functions/parallel_for_func.html>`_.

Let's consider the mapping of fork-join parallelism on the task scheduler in more detail. 

The scheduler runs tasks in a way that tries to achieve several targets simultaneously: 
 - Enable as many threads as possible, by creating enough job, to achieve actual parallelism
 - Preserve data locality to make a single thread execution more efficient  
 - Minimize both memory demands and cross-thread communication to reduce an overhead 

To achieve this, a balance between depth-first and breadth-first execution strategies 
must be reached. Assuming that the task graph is finite, depth-first is better for 
a sequential execution because:

- **Strike when the cache is hot**. The deepest tasks are the most recently created tasks and therefore are the hottest in the cache.
  Also, if they can be completed, tasks that depend on it can continue executing, and though not the hottest in a cache, 
  they are still warmer than the older tasks deeper in the dequeue.
 
- **Minimize space**. Execution of the shallowest task leads to the breadth-first unfolding of a graph. It creates an exponential
  number of nodes that co-exist simultaneously. In contrast, depth-first execution creates the same number 
  of nodes, but only a linear number can exists at the same time, since it creates a stack of other ready 
  tasks.
  
Each thread has its deque of tasks that are ready to run. When a 
thread spawns a task, it pushes it onto the bottom of its deque.

When a thread participates in the evaluation of tasks, it constantly executes 
a task obtained by the first rule that applies from the roughly equivalent ruleset:

- Get the task returned by the previous one, if any.

- Take a task from the bottom of its deque, if any.

- Steal a task from the top of another randomly chosen deque. If the 
  selected deque is empty, the thread tries again to execute this rule until it succeeds.

Rule 1 is described in :doc:`Task Scheduler Bypass <Task_Scheduler_Bypass>`. 
The overall effect of rule 2 is to execute the *youngest* task spawned by the thread, 
which causes the depth-first execution until the thread runs out of work. 
Then rule 3 applies. It steals the *oldest* task spawned by another thread, 
which causes temporary breadth-first execution that converts potential parallelism 
into actual parallelism.
