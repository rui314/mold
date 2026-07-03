.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==============
info Namespace
==============
**[info_namespace]**

Interfaces to query information about execution environment.

.. code:: cpp

   // Declared in header <oneapi/tbb/info.h>

    namespace oneapi {
    namespace tbb {
        using numa_node_id = /*implementation-defined*/;
        using core_type_id = /*implementation-defined*/;

        namespace info {
            std::vector<numa_node_id> numa_nodes();
            std::vector<core_type_id> core_types();

            int default_concurrency(task_arena::constraints c);
            int default_concurrency(numa_node_id id = oneapi::tbb::task_arena::automatic);
        }
    } // namespace tbb
    } // namespace oneapi

Types
-----

``numa_node_id`` - Represents NUMA node identifier.

Functions
---------

.. cpp:function:: std::vector<numa_node_id> numa_nodes()

    Returns the vector of integral indexes that indicate available NUMA nodes.

    .. note::
        If error occurs during system topology parsing, returns vector containing single element
        that equals to ``task_arena::automatic``.

.. cpp:function:: std::vector<core_type_id> core_types()

    Returns the vector of integral indexes that indicate available core types.
    The indexes are sorted from the least performant to the most performant core type.

    .. note::
        If error occurs during system topology parsing, returns vector containing single element
        that equals to ``task_arena::automatic``.

.. cpp:function:: int default_concurrency(task_arena::constraints c)

    Returns concurrency level for the given constraints.

.. cpp:function:: int default_concurrency(numa_node_id id = oneapi::tbb::task_arena::automatic)

    Returns concurrency level of the given NUMA node. If argument is not specified, returns default
    concurrency level for current library configuration.
