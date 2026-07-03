.. _dynamic_dependencies:

``task_group`` Dynamic Dependencies
===================================

.. note::
    To enable this extension, define the ``TBB_PREVIEW_TASK_GROUP_EXTENSIONS`` macro with a value of ``1``.
    When available and enabled, the feature-test macro ``TBB_HAS_TASK_GROUP_DEPENDENCIES`` is defined.

.. contents::
    :local:
    :depth: 2

Description
***********

The |full_name| implementation extends the
:onetbb-spec:`tbb::task_group specification <task_scheduler/task_group/task_group_cls>`
with an API for defining predecessor-successor relationships between tasks,
such that a successor task can begin execution only after all of its predecessors are completed.

An **unsubmitted** task is one that has not been submitted for execution.

A **submitted** task is one that has been submitted for execution, such as by passing a ``task_handle`` to ``task_group::run``.

A non-empty ``task_handle`` object represents an unsubmitted task, while a ``task_completion_handle`` can represent any task.

Both submitted and unsubmitted tasks can serve as predecessors. However, only unsubmitted tasks may be used as successors.

The ``tbb::task_group::set_task_order(pred, succ)`` function establishes a dependency such that ``succ`` cannot begin execution until ``pred`` has completed.

.. code:: cpp

    tbb::task_handle predecessor = tg.defer(pred_body);
    tbb::task_handle successor = tg.defer(succ_body);

    tbb::task_group::set_task_order(predecessor, successor);

The ``tbb::task_group::transfer_this_task_completion_to`` function allows transferring the completion of the currently executing task to another task.
This function must be invoked from within the task body. All successors of the currently executing task will execute only after the task receiving the completion has finished.

.. code:: cpp

    tbb::task_handle t = tg.defer([&tg] {
        tbb::task_handle comp_receiver = tg.defer(receiver_body);
        tbb::task_group::transfer_this_task_completion_to(comp_receiver);
        tg.run(std::move(comp_receiver));
    });

    tbb::task_handle succ = tg.defer(succ_body);

    tbb::task_group::set_task_order(t, succ);
    // Since t transfers its completion to comp_receiver,
    // succ_body will execute after receiver_body

API
***

Header
------

.. code:: cpp

    #define TBB_PREVIEW_TASK_GROUP_EXTENSIONS 1
    #include <oneapi/tbb/task_group.h>
    #include <oneapi/tbb/task_arena.h>

Synopsis
--------

.. code:: cpp

    // <oneapi/tbb/task_group.h> synopsis
    namespace oneapi {
        namespace tbb {
            class task_handle {
            public:
                // Only the requirements for destroyed object are changed
                ~task_handle();
            };

            class task_group {
                // Only the behavior in case of dependent tasks is changed
                void run(task_handle&& handle);

                static void set_task_order(task_handle& pred, task_handle& succ);
                static void set_task_order(task_completion_handle& pred, task_handle& succ);

                static void transfer_this_task_completion_to(task_handle& handle);
            };
        } // namespace tbb
    } // namespace oneapi

.. code:: cpp

    // // <oneapi/tbb/task_arena.h> synopsis
    namespace oneapi {
        namespace tbb {
            class task_arena {
                // Only the behavior in case of dependent tasks is changed
                void enqueue(task_handle&& handle);
            }; // class task_arena

            namespace this_task_arena {
                // Only the behavior in case of dependent tasks is changed
                void enqueue(task_handle&& handle);
            } // namespace this_task_arena
        } // namespace tbb
    } // namespace oneapi

Member Functions of ``task_handle`` Class
-----------------------------------------

.. code:: cpp

    ~task_handle();

Destroys the ``task_handle`` object and associated task if it exists.

.. admonition:: Extension

    If the associated task is involved in a predecessor-successor relationship, the behavior is undefined.

Member Functions of ``task_group`` Class
----------------------------------------

.. code:: cpp

    void run(task_handle&& h);

Schedules the task object pointed by ``h`` for the execution.

.. admonition:: Extension

    If the task associated with ``h`` has predecessors, scheduling the task execution is postponed until all
    of the predecessors have completed, while the function returns immediately.

.. note::

    The failure to satisfy the following conditions leads to undefined behavior:
    
    * ``h`` is not empty.
    * ``*this`` is the same ``task_group`` that ``h`` is created with.

.. code:: cpp

    static void set_task_order(task_handle& pred, task_handle& succ);
    static void set_task_order(task_completion_handle& pred, task_handle& succ);

Registers the task associated with ``pred`` as a predecessor that must complete before the task associated with ``succ`` can begin execution.

It is thread-safe to concurrently add multiple predecessors to a single successor and to register the same predecessor with multiple successors.

It is thread-safe to concurrently add successors to both the task transferring its completion and the task receiving the completion.

It is thread-safe to concurrently add a successor to a ``task_completion_handle`` while the ``task_handle`` associated with the same task is being run.

The behavior is undefined in the following cases:

* Either ``pred`` or ``succ`` is empty.
* The tasks referred by ``pred`` and ``succ`` belong to different ``task_group`` instances.
* The task referred by ``task_completion_handle`` was destroyed without being submitted for execution.

.. code:: cpp

    static void transfer_this_task_completion_to(task_handle& handle);

Transfers the completion of the currently executing task to the task associated with ``handle``.

After the transfer, the successors of the currently executing task will be reassigned to the task associated with ``handle``.

It is thread-safe to transfer successors to the task while concurrently adding successors to it or to the currently executing task.

The behavior is undefined in the following cases:

* ``handle`` is empty.
* The function is called outside the body of a ``task_group`` task.
* The function is called for the task whose completion has already been transferred.
* The currently executing task and the task associated with ``handle`` belong to different ``task_group`` instances.

Member Functions of ``task_arena`` Class
----------------------------------------

.. code:: cpp

    void enqueue(task_handle&& h);

Enqueues a task owned by ``h`` into the ``task_arena`` for processing.

.. admonition:: Extension

    If the task associated with ``h`` has predecessors, scheduling the task execution is postponed until all
    of the predecessors have completed, while the function returns immediately.

The behavior of this function is identical to the generic version (``template<typename F> void task_arena::enqueue(F&& f)``),
except parameter type.

.. note::

    ``h`` should not be empty to avoid an undefined behavior.

``this_task_arena`` Namespace
-----------------------------

.. code:: cpp

    void enqueue(task_handle&& h);

Enqueues a task owned by ``h`` into the ``task_arena`` that is currently used by the calling thread.

.. admonition:: Extension

    If the task associated with ``h`` has predecessors, scheduling the task execution is postponed until all
    of the predecessors have completed, while the function returns immediately.

The behavior of this function is identical to the generic version (``template<typename F> void task_arena::enqueue(F&& f)``),
except parameter type.

.. note::

    ``h`` should not be empty to avoid an undefined behavior.

Example
-------

The following example demonstrates how to perform parallel reduction over a range using the described API.

.. literalinclude:: ../examples/task_group_extensions_reduction.cpp
    :language: c++
    :start-after: /*begin_task_group_extensions_reduction_example*/
    :end-before: /*end_task_group_extensions_reduction_example*/
