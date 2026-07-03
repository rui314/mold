.. _wait_single_task:

Waiting for Individual Tasks in ``task_group``
==============================================

.. note::
    To enable this extension, define the ``TBB_PREVIEW_TASK_GROUP_EXTENSIONS`` macro with a value of ``1``.
    When available and enabled, the feature-test macro ``TBB_HAS_TASK_GROUP_WAIT_FOR_SINGLE_TASK`` is defined.

.. contents::
    :local:
    :depth: 2

Description
***********

This feature extends the ``task_group`` and ``task_arena`` classes with functions
that wait for the completion of a single task, identified by a ``task_handle`` or a
:ref:`task_completion_handle <task_completion_handle_cls>`, complementing the existing
functions that wait for the completion of *all* tasks in a group. 

``task_group::wait_for_task`` blocks until the task represented by the given ``task_completion_handle``
has completed. Other tasks in the group may still be in progress when the function returns.

``task_group::run_and_wait_for_task`` combines submitting a task for execution and waiting for its completion.

``task_group::get_status_of`` returns the current status of the task without blocking.

If the completion of the awaited task was transferred to another task using
``task_group::transfer_this_task_completion_to``, all waiting functions track the task that received the completion
instead.

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

            enum task_group_status {
                not_complete,
                complete,
                canceled,
                task_complete
            };

            class task_group {
            public:
                task_group_status wait_for_task(task_completion_handle& comp_handle);
                task_group_status run_and_wait_for_task(task_handle&& handle);

                task_group_status get_status_of(task_completion_handle& comp_handle);
            }; // class task_group

        } // namespace tbb
    } // namespace oneapi

.. code:: cpp

    // <oneapi/tbb/task_arena.h> synopsis
    namespace oneapi {
        namespace tbb {
            
            class task_arena {
            public:
                task_group_status wait_for(task_completion_handle& comp_handle);
            }; // class task_arena

        } // namespace tbb
    } // namespace oneapi

``task_group_status`` Enumeration
---------------------------------

.. code:: cpp

    not_complete

The work is not yet finished. The meaning depends on the context:

* When returned by ``get_status_of``: the individual task has not yet completed.
* When returned by ``wait`` or ``run_and_wait``: not all tasks in the group have completed.

-------------------------------------------------------

.. code:: cpp

    complete

The group was not canceled and all tasks in the group have completed.

-------------------------------------------------------

.. code:: cpp

    canceled

The cancellation request has been received. The meaning depends on the context:

* When returned by ``get_status_of`` or by a single-task waiting function: the individual task was not executed.
* When returned by ``wait`` or ``run_and_wait``: group execution was canceled, the completion status of individual tasks is unknown.

-------------------------------------------------------

.. code:: cpp

    task_complete

The individual task has completed execution. The cancellation status of the ``task_group`` is unknown - the individual task may be executed
even if the group received a cancellation request.

Member Functions of ``task_group`` Class
----------------------------------------

.. code:: cpp

    task_group_status wait_for_task(task_completion_handle& comp_handle);

Waits for the completion of the task represented by ``comp_handle``. 

If completion was transferred to another task using ``task_group::transfer_this_task_completion_to``,
the function waits for the task that received the completion.

**Returns**: ``task_group_status::task_complete`` if the task was executed, ``task_group_status::canceled`` otherwise.

-------------------------------------------------------

.. code:: cpp

    task_group_status run_and_wait_for_task(task_handle&& handle);

Schedules the task represented by ``handle`` for execution (if it has no unresolved dependencies),
and waits for its completion.

If completion was transferred to another task using ``task_group::transfer_this_task_completion_to``,
the function waits for the task that received the completion.

Semantically equivalent to ``task_completion_handle ch = handle; run(std::move(handle)); wait_for_task(ch);``.

**Returns**: ``task_group_status::task_complete`` if the task was executed, ``task_group_status::canceled`` otherwise.

-------------------------------------------------------

.. code:: cpp

    task_group_status get_status_of(task_completion_handle& comp_handle);

**Returns**: the status of the task represented by ``comp_handle``:

* ``task_group_status::not_complete`` - if the task has not been submitted for execution, or has not finished execution.
* ``task_group_status::task_complete`` - if the task has finished execution.
* ``task_group_status::canceled`` - if the task was not executed due to ``task_group`` cancellation.

If the completion was transferred to another task using ``task_group::transfer_this_task_completion_to``,
the function returns the status of the task that received the completion.

Member Functions of ``task_arena`` Class
----------------------------------------

.. code:: cpp

    task_group_status wait_for(task_completion_handle& comp_handle);

Waits for completion of the task represented by ``comp_handle`` in the current arena.

If completion was transferred to another task using ``task_group::transfer_this_task_completion_to``,
the function waits for the task that received the completion.

Semantically equivalent to: ``execute([&] { tg.wait_for_task(comp_handle); });``, where
``tg`` is a ``task_group`` where the task referred to by ``comp_handle`` is registered.

**Returns**: ``task_group_status::task_complete`` if the task was executed, ``task_group_status::canceled`` otherwise.

Example
*******

The example below demonstrates a cache-backed computation. The ``calculate_one_result``
function checks a cache for a previously computed result. On a cache miss, it uses
``run_and_wait_for_task`` to compute the result and return it to the caller immediately,
while the cache update runs asynchronously in the same ``task_group``.

After all results are computed, ``tg.wait()`` ensures that every background cache store
has completed before the cache is cleared.

.. literalinclude:: ../examples/task_group_extensions_wait_for_one.cpp
    :language: c++
    :start-after: /*begin_task_group_extensions_wait_for_one_example*/
    :end-before: /*end_task_group_extensions_wait_for_one_example*/
