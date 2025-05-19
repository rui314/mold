.. _make_edges:

``make_edges`` function template
================================

.. note::
   To enable this feature, define the ``TBB_PREVIEW_FLOW_GRAPH_FEATURES`` macro to 1.

.. contents::
    :local:
    :depth: 1

Description
***********

The ``make_edges`` function template creates edges between a single node
and each node in a set of nodes.

There are two ways to connect nodes in a set and a single node using
``make_edges``:

.. figure:: ./Resources/make_edges_usage.png
   :align: center

API
***

Header
------

.. code:: cpp

    #include <oneapi/tbb/flow_graph.h>

Syntax
------

.. code:: cpp

    // node_set is an exposition-only name for the type returned from make_node_set function

    template <typename NodeType, typename Node, typename... Nodes>
    void make_edges(node_set<Node, Nodes...>& set, NodeType& node);

    template <typename NodeType, typename Node, typename... Nodes>
    void make_edges(NodeType& node, node_set<Node, Nodes...>& set);

Example
-------

The example implements the graph structure in the picture below.

.. figure:: ./Resources/make_edges_example.png
    :align: center

.. literalinclude:: ./examples/make_edges_function_example.cpp
    :language: c++
    :start-after: /*begin_make_edges_function_example*/
    :end-before: /*end_make_edges_function_example*/
