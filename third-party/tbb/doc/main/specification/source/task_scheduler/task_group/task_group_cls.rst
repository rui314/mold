.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==========
task_group
==========
**[scheduler.task_group]**

A ``task_group`` represents the concurrent execution of a group of tasks.
You can dynamically add tasks to the group while it is executing. 
The thread executing ``task_group::wait()`` might participate in other tasks that
are not related to the particular ``task_group``.


.. code:: cpp

    // Defined in header <oneapi/tbb/task_group.h>

    namespace oneapi {
    namespace tbb {

        class task_group {
        public:
            task_group();
            task_group(task_group_context& context);
            
            ~task_group();

            template<typename Func>
            void run(Func&& f);
            
            template<typename Func>
            task_handle defer(Func&& f);
            
            void run(task_handle&& h);

            template<typename Func>
            task_group_status run_and_wait(const Func& f);
            
            task_group_status run_and_wait(task_handle&& h);

            task_group_status wait();
            void cancel();
        };

        bool is_current_task_group_canceling();

    } // namespace tbb
    } // namespace oneapi

Member functions
----------------

.. namespace:: oneapi::tbb::task_group

.. cpp:function:: task_group()

    Constructs an empty ``task_group``.

.. cpp:function:: task_group(task_group_context& context)

    Constructs an empty ``task_group``. All tasks added into the ``task_group`` are associated with the ``context``.

.. cpp:function:: ~task_group()

    Destroys the ``task_group``.

    **Requires**: Method ``wait`` must be called before destroying a ``task_group``,
    otherwise, the destructor throws an exception.

.. cpp:function:: template<typename F> task_handle  defer(F&& f)

    Creates a deferred task to compute ``f()`` and returns ``task_handle`` pointing to it.
   
    The task is not scheduled for the execution until it is explicitly requested, for example, with the ``task_group::run`` method.
    However, the task is still added into the ``task_group``, thus the ``task_group::wait`` method waits until the ``task_handle`` 
    is either scheduled or destroyed.
    
    The ``F`` type must meet the `Function Objects` requirements described in the [function.objects] section of the ISO C++ standard.
   
    **Returns:** ``task_handle`` object pointing to a task to compute ``f()``.

.. cpp:function:: template<typename Func> void run(Func&& f)

    Adds a task to compute ``f()`` and returns immediately.
    The ``Func`` type must meet the `Function Objects` requirements described in the [function.objects] section of the ISO C++ standard.
    
.. cpp:function:: void run(task_handle&& h)
   
    Schedules the task object pointed by the ``h`` for the execution.

    .. note::
       The failure to satisfy the following conditions leads to undefined behavior:
          * ``h`` is not empty.
          * ``*this`` is the same ``task_group`` that ``h`` is created with.    

.. cpp:function:: template<typename Func> task_group_status run_and_wait(const Func& f)

    Equivalent to ``{run(f); return wait();}``.
    The ``Func`` type must meet the `Function Objects` requirements described in the [function.objects] section of the ISO C++ standard.

    **Returns**: The status of ``task_group``. See :doc:`task_group_status <task_group_status_enum>`.

.. cpp:function::task_group_status run_and_wait(task_handle&& h)

    Equivalent to ``{run(h); return wait();}``.

    .. note::
       The failure to satisfy the following conditions leads to undefined behavior:
          * ``h`` is not empty.
          * ``*this`` is the same ``task_group`` that ``h`` is created with.    

    **Returns**: The status of ``task_group``. See :doc:`task_group_status <task_group_status_enum>`.

.. cpp:function:: task_group_status wait()

    Waits for all tasks in the group to complete or be cancelled.

    **Returns**: The status of ``task_group``. See :doc:`task_group_status <task_group_status_enum>`.

.. cpp:function:: void cancel()

    Cancels all tasks in this ``task_group``.

Non-member functions
--------------------

.. cpp:function:: bool is_current_task_group_canceling()

    Returns true if an innermost ``task_group`` executing on this thread is cancelling its tasks.

