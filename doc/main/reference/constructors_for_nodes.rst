.. _constructors_for_fg_nodes:

Constructors for Flow Graph nodes
=================================

.. note::
   To enable this feature, define the ``TBB_PREVIEW_FLOW_GRAPH_FEATURES`` macro to 1.

.. contents::
    :local:
    :depth: 1

Description
***********

The "Helper Functions for Expressing Graphs" feature adds a set of new constructors
that can be used to construct a node that ``follows`` or ``precedes`` a set of nodes.

Where possible, the constructors support Class Template Argument Deduction (since C++17).

API
***

Header
------

.. code:: cpp

    #include <oneapi/tbb/flow_graph.h>

Syntax
------

.. code:: cpp

    // continue_node
    continue_node(follows(...), Body body, Policy = Policy());
    continue_node(precedes(...), Body body, Policy = Policy());

    continue_node(follows(...), int number_of_predecessors, Body body, Policy = Policy());
    continue_node(precedes(...), int number_of_predecessors, Body body, Policy = Policy());

    // function_node
    function_node(follows(...), std::size_t concurrency, Policy = Policy());
    function_node(precedes(...), std::size_t concurrency, Policy = Policy());

    // input_node
    input_node(precedes(...), body);

    // multifunction_node
    multifunction_node(follows(...), std::size_t concurrency, Body body);
    multifunction_node(precedes(...), std::size_t concurrency, Body body);

    // async_node
    async_node(follows(...), std::size_t concurrency, Body body);
    async_node(precedes(...), std::size_t concurrency, Body body);

    // overwrite_node
    explicit overwrite_node(follows(...));
    explicit overwrite_node(precedes(...));

    // write_once_node
    explicit write_once_node(follows(...));
    explicit write_once_node(precedes(...));

    // buffer_node
    explicit buffer_node(follows(...));
    explicit buffer_node(precedes(...));

    // queue_node
    explicit queue_node(follows(...));
    explicit queue_node(precedes(...));

    // priority_queue_node
    explicit priority_queue_node(follows(...), const Compare& comp = Compare());
    explicit priority_queue_node(precedes(...), const Compare& compare = Compare());

    // sequencer_node
    sequencer_node(follows(...), const Sequencer& s);
    sequencer_node(precedes(...), const Sequencer& s);

    // limiter_node
    limiter_node(follows(...), std::size_t threshold);
    limiter_node(precedes(...), std::size_t threshold);

    // broadcast_node
    explicit broadcast_node(follows(...));
    explicit broadcast_node(precedes(...));

    // join_node
    explicit join_node(follows(...), Policy = Policy());
    explicit join_node(precedes(...), Policy = Policy());

    // split_node
    explicit split_node(follows(...));
    explicit split_node(precedes(...));

    // indexer_node
    indexer_node(follows(...));
    indexer_node(precedes(...));

See Also
********
:ref:`follows_precedes`
