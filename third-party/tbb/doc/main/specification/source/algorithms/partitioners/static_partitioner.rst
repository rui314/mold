.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==================
static_partitioner
==================
**[algorithms.static_partitioner]**

Specifies that a parallel algorithm should distribute the work uniformly across threads and
should not do additional load balancing.

An algorithm with a ``static_partitioner`` distributes the range across threads in subranges
of approximately equal size.  The number of subranges is equal to the number of
threads that can possibly participate in task execution, as specified by
:doc:`global_contol <../../task_scheduler/scheduling_controls/global_control_cls>`
or :doc:`task_arena <../../task_scheduler/task_arena/task_arena_cls>` classes.
These subranges are not further split.

.. caution::

   The regularity of subrange sizes is not guaranteed if the range type does not support
   proportional splitting, or if the grain size is set larger than the
   size of the range divided by the number of threads participating in task execution.

In addition, ``static_partitioner`` uses a deterministic task affinity pattern to hint the task scheduler
how the subranges should be assigned to threads.

The ``static_partitioner`` class satisfies the *CopyConstructibe* requirement from the ISO C++ [utility.arg.requirements] section.

.. tip::

   Use ``static_partitioner`` to:

   * Parallelize small well-balanced workloads where enabling additional load balancing
     opportunities brings more overhead than performance benefits.
   * Port OpenMP* parallel loops with ``schedule(static)`` if deterministic
     work partitioning across threads is important.

.. code:: cpp

    // Defined in header <oneapi/tbb/partitioner.h>

    namespace oneapi {
        namespace tbb {

            class static_partitioner {
            public:
               static_partitioner() = default;
               ~static_partitioner() = default;
            };

        } // namespace tbb
    } // namespace oneapi

See also:

* :doc:`Range named requirement <../../named_requirements/algorithms/range>`

