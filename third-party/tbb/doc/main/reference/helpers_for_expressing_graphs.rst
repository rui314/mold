.. _helpers_for_expressing_graphs:

Helper Functions for Expressing Graphs
======================================

.. note::
   To enable this feature, define the ``TBB_PREVIEW_FLOW_GRAPH_FEATURES`` macro to 1.

Helper functions are intended to make creation of the flow graphs less verbose.

.. contents::
    :local:
    :depth: 1

Description
***********

This feature adds ``make_edges``, ``make_node_set``,
``follows`` and ``precedes`` functions to ``oneapi::tbb::flow`` namespace.
These functions simplify the process of building flow graphs by allowing to gather nodes
into sets and connect them to other nodes in the graph.

API
***

.. toctree::
    :titlesonly:

    constructors_for_nodes
    follows_and_precedes_functions
    make_node_set_function
    make_edges_function

Example
*******

Consider the graph depicted below.

.. figure:: ./Resources/fg_api_graph_structure.png
    :align: center

In the examples below, C++17 Class Template Argument Deduction is used
to avoid template parameter specification where possible.

**Regular API**

.. literalinclude:: ./examples/helpers_for_expressing_graphs_regular_api_example.cpp
    :language: c++
    :start-after: /*begin_helpers_for_expressing_graphs_regular_api_example*/
    :end-before: /*end_helpers_for_expressing_graphs_regular_api_example*/

**Preview API**

.. literalinclude:: ./examples/helpers_for_expressing_graphs_preview_api_example.cpp
    :language: c++
    :start-after: /*begin_helpers_for_expressing_graphs_preview_api_example*/
    :end-before: /*end_helpers_for_expressing_graphs_preview_api_example*/
