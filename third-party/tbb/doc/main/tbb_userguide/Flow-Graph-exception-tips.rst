.. _Flow_Graph_exception_tips:

Flow Graph Tips for Exception Handling and Cancellation
=======================================================


The execution of a flow graph can be canceled directly or as a result of
an exception that propagates beyond a node's body. You can then
optionally reset the graph so that it can be re-executed.

.. toctree::
   :maxdepth: 4

   ../tbb_userguide/catching_exceptions
   ../tbb_userguide/cancel_a_graph
   ../tbb_userguide/use_graph_reset
   ../tbb_userguide/cancelling_nested_parallelism
