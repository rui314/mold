.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

================
auto_partitioner
================
**[algorithms.auto_partitioner]**

Specifies that a parallel loop should optimize its range subdivision based on work-stealing events.

A loop template with an ``auto_partitioner`` attempts to minimize range splitting while providing
ample opportunities for work stealing.

The range subdivision is initially limited to S subranges, where S is proportional to the number of
threads specified by the :doc:`global_contol <../../task_scheduler/scheduling_controls/global_control_cls>`
or :doc:`task_arena <../../task_scheduler/task_arena/task_arena_cls>`.
Each of these subranges is not divided further unless it is stolen by an idle thread.
If stolen, it is further subdivided to create additional subranges. Thus a loop template with an
``auto_partitioner`` creates additional subranges only when it is necessary to balance a load.

An ``auto_partitioner`` performs sufficient splitting to balance load, not necessarily splitting as finely as ``Range::is_divisible`` permits.
When used with classes such as ``blocked_range``, the selection of an appropriate
grain size is less important, and often acceptable performance can be achieved with the default grain size of 1.

The ``auto_partitioner`` class satisfies the *CopyConstructibe* requirement from the ISO C++ [utility.arg.requirements] section.

.. tip::

   When using ``auto_partitioner`` and a ``blocked_range`` for a parallel loop, the body may receive a subrange larger than the grain size of the ``blocked_range``.
   Therefore, do not assume that the grain size is an upper bound of the subrange size.
   Use ``simple_partitioner`` if an upper bound is required.

.. code:: cpp

    // Defined in header <oneapi/tbb/partitioner.h>

    namespace oneapi {
        namespace tbb {

            class auto_partitioner {
            public:
                auto_partitioner() = default;
                ~auto_partitioner() = default;
            };

        } // namespace tbb
    } // namespace oneapi
