.. _task_scheduler_handle_reference:

task_scheduler_handle Class
===========================

.. note::
    To enable this feature, set the ``TBB_PREVIEW_WAITING_FOR_WORKERS`` macro to 1.

.. contents::
    :local:
    :depth: 1

Description
***********

The ``oneapi::tbb::task_scheduler_handle`` class and the ``oneapi::tbb::finalize`` function allow to wait for completion of worker threads.

When the ``oneapi::tbb::finalize`` function is called with an ``oneapi::tbb::task_scheduler_handle`` instance, it blocks the calling
thread until the completion of all worker threads that were implicitly created by the library.

API
***

Header
------

.. code:: cpp

    #include <oneapi/tbb/global_control.h>

Synopsis
--------

.. code:: cpp

    namespace oneapi {
        namespace tbb {

            class task_scheduler_handle {
            public:
                task_scheduler_handle() = default;
                ~task_scheduler_handle();

                task_scheduler_handle(const task_scheduler_handle& other) = delete;
                task_scheduler_handle(task_scheduler_handle&& other) noexcept;
                task_scheduler_handle& operator=(const task_scheduler_handle& other) = delete;
                task_scheduler_handle& operator=(task_scheduler_handle&& other) noexcept;

                explicit operator bool() const noexcept;

                static task_scheduler_handle get();

                static void release(task_scheduler_handle& handle);
            };

            void finalize(task_scheduler_handle& handle);
            bool finalize(task_scheduler_handle& handle, const std::nothrow_t&) noexcept;

        } // namespace tbb
    } // namespace oneapi

Member Functions
----------------

.. cpp:function:: task_scheduler_handle()

    **Effects**: Creates an instance of the ``task_scheduler_handle`` class that does not contain any reference to the task scheduler.

-------------------------------------------------------

.. cpp:function:: ~task_scheduler_handle()

    **Effects**: Destroys an instance of the ``task_scheduler_handle`` class.
    Releases a reference to the task scheduler and deactivates an instance of the ``task_scheduler_handle`` class.

-------------------------------------------------------

.. cpp:function:: task_scheduler_handle(task_scheduler_handle&& other) noexcept

    **Effects**: Creates an instance of the ``task_scheduler_handle`` class that references the task scheduler referenced by ``other``. In turn, ``other`` releases its reference to the task scheduler.

-------------------------------------------------------

.. cpp:function:: task_scheduler_handle& operator=(task_scheduler_handle&& other) noexcept

    **Effects**: Releases a reference to the task scheduler referenced by ``this``. Adds a reference to the task scheduler referenced by ``other``.
In turn, ``other`` releases its reference to the task scheduler.

-------------------------------------------------------

.. cpp:function:: explicit operator bool() const noexcept

    **Returns**: ``true`` if ``this`` references any task scheduler; ``false`` otherwise.

-------------------------------------------------------

.. cpp:function:: task_scheduler_handle get()

    **Returns**: An instance of the ``task_scheduler_handle`` class that holds a reference to the task scheduler preventing
    its premature destruction.

-------------------------------------------------------

.. cpp:function:: void release(task_scheduler_handle& handle)

    **Effects**: Releases a reference to the task scheduler and deactivates an instance of the ``task_scheduler_handle``
    class. Non-blocking method.

Non-member Functions
--------------------

.. cpp:function:: void finalize(task_scheduler_handle& handle)

    **Effects**: Blocks the program execution until all worker threads have been completed. Throws the ``oneapi::tbb::unsafe_wait``
    exception if it is not safe to wait for the completion of the worker threads.

The following conditions should be met for finalization to succeed:

- No active (not yet terminated) instances of class ``task_arena`` exist in the whole program;
- ``task_scheduler_handle::release`` is called for each other active instance of class ``task_scheduler_handle``, possibly by different application threads.

Under these conditions, it is guaranteed that at least one ``finalize`` call succeeds,
at which point all worker threads have been completed.
If calls are performed simultaneously, more than one call might succeed.

.. note::

    If you know how many active ``task_scheduler_handle`` instances exist in the program,
    it is necessary to ``release`` all but the last one, then call ``finalize`` for
    the last instance.

.. caution::

  The method always fails if called within a task, a parallel algorithm, or a flow graph node.

-------------------------------------------------------

.. cpp:function:: bool finalize(task_scheduler_handle& handle, const std::nothrow_t&) noexcept

    **Effects**: Blocks the program execution until all worker threads have been completed. Same as above, but returns ``true`` if all worker
    threads have been completed successfully, or ``false`` if it is not safe to wait for the completion of the worker threads.

Examples
********

.. code:: cpp

    #define TBB_PREVIEW_WAITING_FOR_WORKERS 1
    #include <oneapi/tbb/global_control.h>
    #include <oneapi/tbb/parallel_for.h>

    #include <iostream>

    int main() {
        oneapi::tbb::task_scheduler_handle handle = oneapi::tbb::task_scheduler_handle::get();
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
