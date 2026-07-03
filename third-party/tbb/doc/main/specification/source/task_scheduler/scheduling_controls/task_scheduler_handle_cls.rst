.. SPDX-FileCopyrightText: 2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=====================
task_scheduler_handle
=====================
**[scheduler.task_scheduler_handle]**

The ``oneapi::tbb::task_scheduler_handle`` class and the ``oneapi::tbb::finalize`` function allow user to wait for completion of worker threads.

When the ``oneapi::tbb::finalize`` function is called with an ``oneapi::tbb::task_scheduler_handle`` instance, it blocks the calling
thread until the completion of all worker threads that were implicitly created by the library.


.. code:: cpp

    // Defined in header <oneapi/tbb/global_control.h>

    namespace oneapi {
        namespace tbb {

            class task_scheduler_handle {
            public:
                task_scheduler_handle() = default;
                task_scheduler_handle(oneapi::tbb::attach);
                ~task_scheduler_handle();

                task_scheduler_handle(const task_scheduler_handle& other) = delete;
                task_scheduler_handle(task_scheduler_handle&& other) noexcept;
                task_scheduler_handle& operator=(const task_scheduler_handle& other) = delete;
                task_scheduler_handle& operator=(task_scheduler_handle&& other) noexcept;

                explicit operator bool() const noexcept;

                void release();
            };

            void finalize(task_scheduler_handle& handle);
            bool finalize(task_scheduler_handle& handle, const std::nothrow_t&) noexcept;

        } // namespace tbb
    } // namespace oneapi

Member Functions
----------------

.. cpp:function:: task_scheduler_handle()

    **Effects**: Creates an empty instance of the ``task_scheduler_handle`` class that does not contain any references to the task scheduler.
    
-------------------------------------------------------

.. cpp:function:: task_scheduler_handle(oneapi::tbb::attach)

    **Effects**: Creates an instance of the ``task_scheduler_handle`` class that holds a reference to the task scheduler preventing
    its premature destruction.

-------------------------------------------------------

.. cpp:function:: ~task_scheduler_handle()

    **Effects**: Destroys an instance of the ``task_scheduler_handle`` class.
    If not empty, releases a reference to the task scheduler and deactivates an instance of the ``task_scheduler_handle`` class.

-------------------------------------------------------

.. cpp:function:: task_scheduler_handle(task_scheduler_handle&& other) noexcept

    **Effects**: Creates an instance of the ``task_scheduler_handle`` class that references the task scheduler referenced by ``other``. In turn, ``other`` releases its reference to the task scheduler.

-------------------------------------------------------

.. cpp:function:: task_scheduler_handle& operator=(task_scheduler_handle&& other) noexcept

    **Effects**: If not empty, releases a reference to the task scheduler referenced by ``this``. Adds a reference to the task scheduler referenced by ``other``.
    In turn, ``other`` releases its reference to the task scheduler.
    **Returns**: A reference to ``*this``.

-------------------------------------------------------

.. cpp:function:: explicit operator bool() const noexcept

    **Returns**: ``true`` if ``this`` is not empty and refers to some task scheduler; ``false`` otherwise.

-------------------------------------------------------

.. cpp:function:: void release()

    **Effects**: If not empty, releases a reference to the task scheduler and deactivates an instance of the ``task_scheduler_handle``
    class; otherwise, does nothing. Non-blocking method.

Non-member Functions
--------------------

.. cpp:function:: void finalize(task_scheduler_handle& handle)

    **Effects**: If ``handle`` is not empty, blocks the program execution until all worker threads have been completed; otherwise, does nothing.
    Throws the ``oneapi::tbb::unsafe_wait`` exception if it is not safe to wait for the completion of the worker threads.

The following conditions should be met for finalization to succeed:

- No active, not yet terminated, instances of ``task_arena`` class exist in the whole program.
- ``task_scheduler_handle::release`` is called for each other active instance of ``task_scheduler_handle`` class, possibly by different application threads.

Under these conditions, it is guaranteed that at least one ``finalize`` call succeeds,
at which point all worker threads have been completed.
If calls are performed simultaneously, more than one call might succeed.

.. note::

    If user knows how many active ``task_scheduler_handle`` instances exist in the program,
    it is necessary to ``release`` all but the last one, then call ``finalize`` for
    the last instance.

.. caution::

  The method always fails if called within a task, a parallel algorithm, or a flow graph node.

-------------------------------------------------------

.. cpp:function:: bool finalize(task_scheduler_handle& handle, const std::nothrow_t&) noexcept

    **Effects**: If ``handle`` is not empty, blocks the program execution until all worker threads have been completed; otherwise, does nothing.
    The behavior is the same as ``finalize(handle)`` however, ``false`` is returned instead of exception  or ``true`` if no exception.

Examples
--------

.. code:: cpp

    #include <oneapi/tbb/global_control.h>
    #include <oneapi/tbb/parallel_for.h>

    #include <iostream>

    int main() {
        oneapi::tbb::task_scheduler_handle handle;

        handle = oneapi::tbb::task_scheduler_handle{oneapi::tbb::attach{}};
        
        // Do some parallel work here, e.g.
        oneapi::tbb::parallel_for(0, 10000, [](int){});
        try {
            oneapi::tbb::finalize(handle);
            // oneTBB worker threads are terminated at this point.
        } catch (const oneapi::tbb::unsafe_wait&) {
            std::cerr << "Failed to terminate the worker threads." << std::endl;
        }
        return 0;
    }

See also:

* :doc:`attach <../attach_tag_type>`
