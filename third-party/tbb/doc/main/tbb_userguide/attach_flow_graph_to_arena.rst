.. _attach_flow_graph_to_arena:

Attach Flow Graph to an Arbitrary Task Arena
=============================================


|short_name| ``task_arena`` interface provides mechanisms to guide tasks
execution within the arena by setting the preferred computation units,
restricting part of computation units, or limiting arena concurrency. In some
cases, you may want to apply such mechanisms when a flow graph executes.

During its construction, a ``graph`` object attaches to the arena, in which the constructing
thread occupies a slot.

This example shows how to set the most performant core type as the preferred one
for a graph execution:


.. literalinclude:: ./snippets/flow_graph_examples.cpp
   :language: c++
   :start-after: /*begin_attach_to_arena_1*/
   :end-before: /*end_attach_to_arena_1*/


A ``graph`` object can be reattached to a different ``task_arena`` by calling
the ``graph::reset()`` function. It reinitializes and reattaches the ``graph`` to
the task arena instance, inside which the ``graph::reset()`` method is executed.

This example shows how to reattach existing graph to an arena with the most performant
core type as the preferred one for a work execution. Whenever a task is spawned on behalf
of the graph, it is spawned in the arena of a graph it is attached to, disregarding
the arena of the thread that the task is spawned from:


.. literalinclude:: ./snippets/flow_graph_examples.cpp
   :language: c++
   :start-after: /*begin_attach_to_arena_2*/
   :end-before: /*end_attach_to_arena_2*/

See the following topics to learn more:

.. toctree::
   :maxdepth: 4

   ../tbb_userguide/Guiding_Task_Scheduler_Execution
   ../tbb_userguide/work_isolation

