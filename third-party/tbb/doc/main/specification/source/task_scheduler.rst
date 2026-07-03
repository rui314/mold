.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==============
Task Scheduler
==============
**[scheduler]**

oneAPI Threading Building Blocks (oneTBB) provides a task scheduler, which is the engine that drives the algorithm
templates and task groups. The exact tasking API depends on the implementation.

The tasks are quanta of computation. The scheduler implements worker thread pool and maps tasks onto these threads.
The mapping is non-preemptive. Once a thread starts running a task, the task is bound to that thread until completion.
During that time, the thread services other tasks only when it waits for completion of
nested parallel constructs, as described below. While waiting, either user or worker thread may run any available
task, including unrelated tasks created by this or other threads.

The task scheduler is intended for parallelizing computationally intensive work.
Because task objects are not scheduled preemptively, they should generally avoid making
calls that might block a thread for long periods during which the thread cannot service other tasks.

.. caution::

   There is no guarantee that *potentially* parallel tasks *actually* execute in parallel,
   because the scheduler adjusts actual parallelism to fit available worker threads.
   For example, given a single worker thread, the scheduler creates no actual parallelism.
   For example, it is generally unsafe to use tasks in a producer consumer relationship, because
   there is no guarantee that the consumer runs at all while the producer is running.

Scheduling controls
-------------------

.. toctree::
   :titlesonly:

   task_scheduler/scheduling_controls/task_group_context_cls.rst
   task_scheduler/scheduling_controls/global_control_cls.rst
   task_scheduler/scheduling_controls/resumable_tasks.rst
   task_scheduler/scheduling_controls/task_scheduler_handle_cls.rst

Task Group
----------

.. toctree::
   :titlesonly:

   task_scheduler/task_group/task_group_cls.rst
   task_scheduler/task_group/task_group_status_enum.rst
   task_scheduler/task_group/task_handle.rst

Task Arena
----------

.. toctree::
   :titlesonly:

   task_scheduler/task_arena/task_arena_cls.rst
   task_scheduler/task_arena/this_task_arena_ns.rst
   task_scheduler/task_arena/task_scheduler_observer_cls.rst

Helper types
------------

.. toctree::
   :titlesonly:

   task_scheduler/attach_tag_type.rst

