.. _make_node_set:

``make_node_set`` function template
===================================

.. note::
   To enable this feature, define the ``TBB_PREVIEW_FLOW_GRAPH_FEATURES`` macro to 1.

.. contents::
    :local:
    :depth: 1

Description
***********

The ``make_node_set`` function template creates a set of nodes that
can be passed as arguments to ``make_edges``, ``follows`` and ``precedes`` functions.

API
***

Header
------

.. code:: cpp

    #include <oneapi/tbb/flow_graph.h>

Syntax
------

.. code:: cpp

    template <typename Node, typename... Nodes>
    /*unspecified*/ make_node_set( Node& node, Nodes&... nodes );

See Also
********

:ref:`make_edges`

:ref:`follows_precedes`