.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===============
Resumable tasks
===============
**[scheduler.resumable_tasks]**

Functions to suspend task execution at a specific point and signal to resume it later.

.. code:: cpp

    // Defined in header <oneapi/tbb/task.h>

    using oneapi::tbb::task::suspend_point = /* implementation-defined */;
    template < typename Func > void oneapi::tbb::task::suspend( Func );
    void oneapi::tbb::task::resume( oneapi::tbb::task::suspend_point );

Requirements:

* The ``Func`` type must meet the :doc:`SuspendFunc requirements <../../named_requirements/task_scheduler/suspend_func>`.

The ``oneapi::tbb::task::suspend`` function called within a running task suspends execution of the task and switches the thread to participate in other oneTBB parallel work.
This function accepts a user callable object with the current execution context ``oneapi::tbb::task::suspend_point`` as an argument.
The user-specified callable object is executed by the calling thread.

The ``oneapi::tbb::task::suspend_point`` context tag must be passed to the ``oneapi::tbb::task::resume`` function to trigger a program execution at the suspended point.
The ``oneapi::tbb::task::resume`` function can be called at any point of an application, even on a separate thread.
In this regard, this function acts as a signal for the task scheduler.

.. note::

    There are no guarantees that the same thread that called ``oneapi::tbb::task::suspend`` continues execution after the suspended point.
    However, these guarantees are provided for the outermost blocking oneTBB calls
    (such as ``oneapi::tbb::parallel_for`` and ``oneapi::tbb::flow::graph::wait_for_all``) and ``oneapi::tbb::task_arena::execute`` calls.

Example
-------

.. code:: cpp

    // Parallel computation region
    oneapi::tbb::parallel_for(0, N, [&](int) {
        // Suspend the current task execution and capture the context
        oneapi::tbb::task::suspend([&] (oneapi::tbb::task::suspend_point tag) {
            // Dedicated user-managed activity that processes async requests.
            async_activity.submit(tag); // could be OpenCL/IO/Database/Network etc.
        }); // execution will be resumed after this function
    });

.. code:: cpp

    // Dedicated user-managed activity:

    // Signal to resume execution of the task referenced by the oneapi::tbb::task::suspend_point
    // from a dedicated user-managed activity
    oneapi::tbb::task::resume(tag);

