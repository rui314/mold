.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============================
Dependency Flow Graph Example
=============================

In the following example, five computations A-E are set up with the
partial ordering shown below in "A simple dependency graph.". For each edge in
the flow graph, the node at the tail of the edge must complete its execution
before the node at the head may begin.

.. figure:: ../Resources/dep_graph.jpg
    :width: 249
    :height: 409
    :align: center

    A simple dependency graph.

.. include:: ./examples/dependency_flow_graph.cpp
    :code: cpp


In this example, nodes A-E print out their names. All of these nodes are
therefore able to use ``struct body`` to construct their body objects.

In function ``main``, the flow graph is set up once and then run three times. All of the nodes in
this example pass around ``continue_msg`` objects. This type is used to communicate that a node has
completed execution.

The first line in function ``main`` instantiates a ``graph`` object ``g``. On the next line, a
``broadcast_node`` named ``start`` is created. Anything passed to this node will be
broadcast to all of its successors. The node ``start`` is used in the ``for`` loop at the bottom of
``main`` to launch the execution of the rest of the flow graph.

In the example, five ``continue_node`` objects are created, named a - e. Each node is constructed
with a reference to ``graph`` ``g`` and the function object to invoke when it runs. The
successor / predecessor relationships are set up by the ``make_edge`` calls that follow the
declaration of the nodes.

After the nodes and edges are set up, the ``try_put`` in each iteration of the ``for`` loop results
in a broadcast of a ``continue_msg`` to both ``a`` and ``b``. Both ``a`` and ``b`` are waiting for a
single ``continue_msg``, since they both have only a single predecessor, ``start``.

When they receive the message from ``start``, they execute their body objects. When complete,
each of them forwards a message to a successor, and so on. The graph
uses tasks to execute the node bodies as well as to forward messages between
the nodes, allowing computation to execute concurrently when possible.

See also:

* :doc:`continue_msg class <continue_msg_cls>`
* :doc:`continue_node class <continue_node_cls>`
