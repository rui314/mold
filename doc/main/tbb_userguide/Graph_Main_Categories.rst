.. _Graph_Main_Categories:

Graph Application Categories
============================


Most flow graphs fall into one of two categories:


-  **Data flow graphs.** In this type of graph, data is passed along the
   graph's edges. The nodes receive, transform and then pass along the
   data messages.
-  **Dependence graphs.** In this type of graph, the data operated on by
   the nodes is obtained through shared memory directly and is not
   passed along the edges.

.. toctree::
   :maxdepth: 4

   ../tbb_userguide/Data_Flow_Graph
   ../tbb_userguide/Dependence_Graph