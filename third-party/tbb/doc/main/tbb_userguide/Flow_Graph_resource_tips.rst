.. _Flow_Graph_resource_tips:

Flow Graph Tips for Limiting Resource Consumption
=================================================


You may want to control the number of messages allowed to enter parts of
your graph, or control the maximum number of tasks in the work pool.
There are several mechanisms available for limiting resource consumption
in a flow graph.

.. toctree::
   :maxdepth: 4

   ../tbb_userguide/use_limiter_node
   ../tbb_userguide/use_concurrency_limits
   ../tbb_userguide/create_token_based_system
   ../tbb_userguide/attach_flow_graph_to_arena