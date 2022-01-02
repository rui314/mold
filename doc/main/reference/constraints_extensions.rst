.. _constraints_extensions:

task_arena::constraints extensions
======================================

.. note::
    To enable this feature, set the ``TBB_PREVIEW_TASK_ARENA_CONSTRAINTS_EXTENSION`` macro to 1.

.. contents::
    :local:
    :depth: 1

Description
***********

These extensions allow to customize ``tbb::task_arena::constraints`` with the following properties:

* On machines with Intel® Hybrid Technology set the preferred core type for threads working within the task arena.
* Limit the maximum number of threads that can be scheduled to one core simultaneously.

API
***

Header
------

.. code:: cpp

    #include <oneapi/tbb/task_arena.h>

Synopsis
--------

.. code:: cpp

    namespace oneapi {
        namespace tbb {

            class task_arena {
            public:
                struct constraints {
                    constraints& set_numa_id(numa_node_id id);
                    constraints& set_max_concurrency(int maximal_concurrency);
                    constraints& set_core_type(core_type_id id);
                    constraints& set_max_threads_per_core(int threads_number);

                    numa_node_id numa_id = task_arena::automatic;
                    int max_concurrency = task_arena::automatic;
                    core_type_id core_type = task_arena::automatic;
                    int max_threads_per_core = task_arena::automatic;
                }; // struct constraints
            }; // class task_arena

        } // namespace tbb
    } // namespace oneapi

Member Functions
----------------

.. cpp:function:: constraints& set_numa_id(numa_node_id id)

    Sets the ``numa_id`` field to the ``id``.

    **Returns:** Reference to ``*this``.

.. cpp:function:: constraints& set_max_concurrency(int maximal_concurrency)

    Sets the ``max_concurrency`` field to the ``maximal_concurrency``.

    **Returns:** Reference to ``*this``.

.. cpp:function:: constraints& set_core_type(core_type_id id)

    Sets the ``core_type`` field to the ``id``.

    **Returns:** Reference to ``*this``.

.. cpp:function:: constraints& set_max_threads_per_core(int threads_number)

    Sets the ``max_threads_per_core`` field to the ``threads_number``.

    **Returns:** Reference to ``*this``.

Member Objects
--------------

.. cpp:member:: numa_node_id numa_id

    An integral logical index uniquely identifying a NUMA node. All threads joining the
    ``task_arena`` are bound to this NUMA node.

    .. note::

        To obtain a valid NUMA node ID, call ``oneapi::tbb::info::numa_nodes()``.

.. cpp:member:: int max_concurrency

    The maximum number of threads that can participate in work processing within the
    ``task_arena`` at the same time.


.. cpp:member:: core_type_id core_type

    An integral logical index uniquely identifying a core type. All threads joining the
    ``task_arena`` are bound to this core type.

    .. note::

        To obtain a valid core type node ID, call ``oneapi::tbb::info::core_types()``.

.. cpp:member:: int max_threads_per_core

    The maximum number of threads that can be scheduled to one core simultaneously.

See also:

* :doc:`oneapi::tbb::info namespace preview extensions <info_namespace_extensions>`
* `oneapi::tbb::task_arena specification <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/task_scheduler/task_arena/task_arena_cls.html>`_
