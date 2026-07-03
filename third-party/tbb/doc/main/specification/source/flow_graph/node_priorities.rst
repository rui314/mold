.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

================
Nodes Priorities
================
**[flow_graph.node_priorities]**

Flow graph provides interface for setting relative priorities at construction of flow graph functional
nodes, guiding threads that execute the graph to prefer nodes with higher priority.

.. code:: cpp

    namespace oneapi {
    namespace tbb {
    namespace flow {

        typedef unsigned int node_priority_t;

        const node_priority_t no_priority = node_priority_t(0);

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

``function_node``, ``multifunction_node``, ``async_node`` and ``continue_node`` has a constructor
with parameter of ``node_priority_t`` type, which sets the node priority in the graph: the larger
the specified value for the parameter, the higher the priority. The special constant value
``no_priority``, which is also the default value of the parameter, switches priority off for a particular node.

For a particular graph, tasks to execute the nodes whose priority is specified have
precedence over tasks for the nodes with lower or no priority value set. When looking for a
task to execute, a thread chooses the one with the highest priority from those in the
graph that are available for execution.

Example
-------

The following basic example demonstrates prioritization of one path in the graph over the
other, which may help to improve overall performance of the graph.

.. figure:: ../Resources/critical_path_in_graph.png
   :width: 463
   :height: 336
   :align: center

   Dependency flow graph with a critical path.

Consider executing the graph from the picture above using two threads. Assume that the nodes ``f1`` and
``f3`` take equal time to execute, while the node ``f2`` takes longer. That makes the nodes
``bs``, ``f2``, and ``fe`` constitute the critical path in this graph. Due to the
non-deterministic behavior in selection of the tasks, oneTBB might execute nodes ``f1`` and ``f3``
in parallel first, which would make the whole graph execution time last longer than the case when
one of the threads chooses the node ``f2`` just after the broadcast node. By setting a higher
priority on node ``f2``, threads are guided to take the critical path task earlier, thus reducing
overall execution time.

.. include:: examples/node_priorities.cpp
   :code: cpp
