.. SPDX-FileCopyrightText: 2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===============
attach tag type
===============
**[scheduler.attach]**

An ``attach`` tag type is specifically used with ``task_arena`` and 
``task_scheduler_handle`` interfaces. It is guaranteed to be constructible by default.

.. code:: cpp

    namespace oneapi {
        namespace tbb {
            using attach = /* unspecified */
        }
    }

See also:

* :doc:`task_arena <task_arena/task_arena_cls>`
* :doc:`task_scheduler_handle <scheduling_controls/task_scheduler_handle_cls>`