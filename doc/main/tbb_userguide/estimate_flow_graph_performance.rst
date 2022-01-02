.. _estimate_flow_graph_performance:

Estimating Flow Graph Performance
=================================


The performance or scalability of a flow graph is not easy to predict.
However there are a few key points that can guide you in estimating the
limits on performance and speedup of some graphs.


.. container:: section


   .. rubric:: The Critical Path Limits the Scalability in a Dependence
      Graph
      :class: sectiontitle

   A critical path is the most time consuming path from a node with no
   predecessors to a node with no successors. In a dependence graph, the
   execution of the nodes along a path cannot be overlapped since they
   have a strict ordering. Therefore, for a dependence graph, the
   critical path limits scalability.


   More formally, let T be the total time consumed by all of the nodes
   in your graph if executed sequentially. Then let C be the time
   consumed along the path that takes the most time. The nodes along
   this path cannot be overlapped even in a parallel execution.
   Therefore, even if all other paths are executed in parallel with C,
   the wall clock time for the parallel execution is at least C, and the
   maximum possible speedup (ignoring microarchitectural and memory
   effects) is T/C.


.. container:: section


   .. rubric:: There is Overhead in Spawning a Node's Body as a Task
      :class: sectiontitle

   The bodies of ``input_nodes``, ``function_nodes``, ``continue_nodes`` and
   ``multifunction_nodes`` execute within spawned tasks by default. This
   means that you need to take into account the overhead of task
   scheduling when estimating the time it takes for a node to execute
   its body. All of the rules of thumb for determining the appropriate
   granularity of tasks therefore also apply to node bodies as well. If
   you have many fine-grained nodes in your flow graph, the impact of
   these overheads can noticeably impact your performance. However,
   depending on the graph structure, you can reduce such overheads by
   using lightweight policy with these nodes.

