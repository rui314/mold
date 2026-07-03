.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==================
task_group_context
==================
**[scheduler.task_group_context]**

``task_group_context`` represents a set of properties used by task scheduler for execution of
the associated tasks. Each task is associated with only one ``task_group_context`` object.

The ``task_group_context`` objects form a forest of trees. Each tree's root is a ``task_group_context`` constructed as ``isolated``.

``task_group_context`` is cancelled explicitly by the user request, or implicitly when an
exception is thrown out of an associated task. Canceling ``task_group_context`` causes the entire subtree rooted at it to be cancelled.

The ``task_group_context`` carries floating point settings inherited from the parent ``task_group_context`` object or captured with a dedicated interface.

.. code:: cpp

    // Defined in header <oneapi/tbb/task_group.h>

    namespace oneapi {
    namespace tbb {

        class task_group_context {
        public:
            enum kind_t {
                isolated = /* implementation-defined */,
                bound = /* implementation-defined */
            };
            enum traits_type {
                fp_settings = /* implementation-defined */,
                default_traits = 0
            };

            task_group_context( kind_t relation_with_parent = bound,
                                uintptr_t traits = default_traits );
            ~task_group_context();

            void reset();
            bool cancel_group_execution();
            bool is_group_execution_cancelled() const;
            void capture_fp_settings();
            uintptr_t traits() const;
        };

    } // namespace tbb;
    } // namespace oneapi


Member types and constants
--------------------------

.. cpp:enum:: kind_t::isolated

    When passed to the specific constructor, the created ``task_group_context`` object has no parent.

.. cpp:enum:: kind_t::bound

    When passed to the specific constructor, the created ``task_group_context`` object becomes a child of the
    innermost running task's group when the first task associated to the ``task_group_context`` is passed to the task scheduler.
    If there is no innermost running task on the current thread, the ``task_group_context`` becomes ``isolated``.

.. cpp:enum:: traits_type::fp_settings

    When passed to the specific constructor, the flag forces the context to capture floating-point settings from the current thread.

Member functions
----------------

.. cpp:function:: task_group_context( kind_t relation_to_parent=bound, uintptr_t traits=default_traits )

    Constructs an empty ``task_group_context``.

.. cpp:function:: ~task_group_context()

    Destroys an empty task_group_context.
    The behavior is undefined if there are still extant tasks associated with this ``task_group_context``.

.. cpp:function:: bool cancel_group_execution()

    Requests that tasks associated with this ``task_group_context`` are not executed.

    Returns ``false`` if this ``task_group_context`` is already cancelled; ``true``, otherwise.
    If concurrently called by multiple threads, exactly one call returns ``true`` and the rest return ``false``.

.. cpp:function:: bool is_group_execution_cancelled() const

    Returns ``true`` if this ``task_group_context`` has received the cancellation request.

.. cpp:function:: void reset()

    Reinitializes this ``task_group_context`` to the uncancelled state.

    .. caution::

        This method is only safe to call once all tasks associated
        with the group's subordinate groups have completed. This method must not be
        invoked concurrently by multiple threads.

.. cpp:function:: void capture_fp_settings()

    Captures floating-point settings from the current thread.

    .. caution::

        This method is only safe to call once all tasks associated
        with the group's subordinate groups have completed. This method must not be
        invoked concurrently by multiple threads.

.. cpp:function:: uintptr_t traits() const

    Returns traits of this ``task_group_context``.

