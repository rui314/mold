.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=================
task_group_status
=================
**[scheduler.task_group_status]**

A ``task_group_status`` type represents the status of a ``task_group``.

.. code:: cpp

    namespace oneapi {
    namespace tbb {
        enum task_group_status {
            not_complete,
            complete,
            canceled
        };
    } // namespace tbb
    } // namespace oneapi

Member constants
----------------

.. c:macro:: not_complete

    Not cancelled and not all tasks in a group have completed.

.. c:macro:: complete

    Not cancelled and all tasks in a group have completed.

.. c:macro:: canceled

    Task group received cancellation request.

