.. _follows_precedes:

``follows`` and ``precedes`` function templates
===============================================

.. note::
   To enable this feature, define the ``TBB_PREVIEW_FLOW_GRAPH_FEATURES`` macro to 1.

The ``follows`` and ``precedes`` helper functions aid in expressing
dependencies between nodes when building oneTBB flow graphs. These helper functions can
only be used while constructing the node.

.. contents::
    :local:
    :depth: 1

Description
***********

The ``follows`` helper function specifies that the node being constructed is
the successor of the set of nodes passed as an argument.

The ``precedes`` helper function specifies that the node being constructed is
the predecessor of the set of nodes passed as an argument.

Functions ``follows`` and ``precedes`` are meant to replace the graph argument, which is
passed as the first argument to the constructor of the node. The graph argument for the
node being constructed is obtained either from the specified node set or the sequence of nodes passed
to ``follows`` or ``precedes``.

If the nodes passed to ``follows`` or ``precedes`` belong to
different graphs, the behavior is undefined.

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

    template <typename NodeType, typename... NodeTypes>
    /*unspecified*/ follows( node_set<NodeType, NodeTypes...>& set );

    template <typename NodeType, typename... NodeTypes>
    /*unspecified*/ follows( NodeType& node, NodeTypes&... nodes );

    template <typename NodeType, typename... NodeTypes>
    /*unspecified*/ precedes( node_set<NodeType, NodeTypes...>& set );

    template <typename NodeType, typename... NodeTypes>
    /*unspecified*/ precedes( NodeType& node, NodeTypes&... nodes );

Input Parameters
----------------

Either a set or a sequence of nodes can be used as arguments for ``follows`` and
``precedes``. The following expressions are equivalent:

.. code-block:: cpp
    :caption: A set of nodes as an input

    auto handlers = make_node_set(n1, n2, n3);
    broadcast_node<int> input(precedes(handlers));

.. code-block:: cpp
    :caption: A sequence of nodes as an input
 
    broadcast_node<int> input(precedes(n1, n2, n3));
