.. _fg_ctad:

Class Template Argument Deduction for Flow Graph Nodes
======================================================

Starting from C++17, many Flow Graph nodes support Class Template Argument Deduction (CTAD),
which allows the compiler to deduce class template parameters from constructor arguments.
This eliminates the need to explicitly specify input and output types for Flow Graph nodes.

.. contents::
    :local:
    :depth: 1

Deduction from Body Type
************************

Flow Graph functional nodes can deduce template parameters from the signature of any
callable object supported by ``std::invoke`` passed as the body. 

For example, a ``function_node`` body that takes ``int`` and returns ``double`` results in deducing the node
type as ``function_node<int, double>``:

.. code:: cpp

    function_node f(g, unlimited, [](int input) -> double { return ...; });
    // f is deduced as function_node<int, double>

For nodes supporting different node policies, the object of the policy can be passed as
an additional constructor argument to allow the deduction:

.. code:: cpp

    function_node f(g, unlimited, [](int input) -> double { return ...; }, rejecting{});
    // f is deduced as function_node<int, double, rejecting>

The following nodes support CTAD from the body type:

* ``function_node`` - deduces node's input and output types from the body
* ``continue_node`` - deduces output type from the body's return type (``void`` maps to ``continue_msg``)
* ``input_node`` - deduces output type from the body's return type
* ``sequencer_node`` - deduces message type from the sequencer body's input type
* ``join_node`` with ``key_matching`` policy - deduces output tuple and key-matching policy from the provided bodies

``multifunction_node`` and ``async_node`` do not support CTAD because their body arguments depend
on the complete node type through the ``output_ports_type`` parameter.

Example
-------

Without CTAD, the template parameters must be specified explicitly:

.. literalinclude:: ./examples/fg_ctad.cpp
    :language: c++
    :start-after: /*begin_fg_ctad_no_ctad_example*/
    :end-before: /*end_fg_ctad_no_ctad_example*/

With CTAD, the compiler deduces the types from the constructor arguments:

.. literalinclude:: ./examples/fg_ctad.cpp
    :language: c++
    :start-after: /*begin_fg_ctad_with_ctad_example*/
    :end-before: /*end_fg_ctad_with_ctad_example*/

Deduction from Predecessors and Successors
******************************************

.. note::
    To enable this feature, define the ``TBB_PREVIEW_FLOW_GRAPH_FEATURES`` macro to 1.

When using the ``follows`` and ``precedes`` helper functions to construct nodes, non-functional
nodes can deduce their input and output types from their predecessors and successors.

The following additional nodes support CTAD through ``follows``/``precedes``:

* ``broadcast_node``
* ``buffer_node``, ``queue_node``, ``priority_queue_node``
* ``overwrite_node``, ``write_once_node``
* ``limiter_node``
* ``join_node`` with ``queueing`` or ``reserving`` policy
* ``indexer_node``
* ``split_node``

CTAD for functional nodes also works with ``follows`` and ``precedes``, but deduces the node's input
and output types from the provided body, as described in the section above.

Example
-------

.. literalinclude:: ./examples/fg_ctad.cpp
    :language: c++
    :start-after: /*begin_fg_ctad_preview_example*/
    :end-before: /*end_fg_ctad_preview_example*/
