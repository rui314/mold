.. _use_graph_reset:

Use graph::reset() to Reset a Canceled Graph
============================================


When a graph execution is canceled either because of an unhandled
exception or because its task_group_context is canceled explicitly, the
graph and its nodes may be left in an indeterminate state. For example,
in the code samples shown in :ref:`cancel_a_graph` the input 2 may be
left in a buffer. But even beyond remnants in the buffers, there are
other optimizations performed during the execution of a flow graph that
can leave its nodes and edges in an indeterminate state. If you want to
re-execute or restart a graph, you first need to reset the graph:


::


     try {
         g.wait_for_all();
     } catch ( int j ) {
         cout << "Caught " << j << "\n";
         // do something to fix the problem
         g.reset();
         f1.try_put(1);
         f1.try_put(2);
         g.wait_for_all();
     }

