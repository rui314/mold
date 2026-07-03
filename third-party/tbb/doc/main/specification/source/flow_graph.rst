.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==========
Flow Graph
==========
**[flow_graph]**

In addition to loop parallelism, the oneAPI Threading Building Blocks (oneTBB) library also supports graph
parallelism. With this feature, highly scalable and completely sequential graphs can be created.

There are three types of components used to implement a graph:

* A ``graph`` class instance
* Nodes
* Ports and edges

Graph Class
-----------

The ``graph`` class instance owns all the tasks created on behalf of the flow graph. Users can wait
on the ``graph`` if they need to wait for the completion of all of the tasks related to the flow
graph execution. Users can also register external interactions with the ``graph`` and run tasks under
the ownership of the flow graph.

.. toctree::
    :titlesonly:

    flow_graph/graph_cls.rst

Nodes
-----

Abstract Interfaces
~~~~~~~~~~~~~~~~~~~

To be used as a graph node type, a class needs to inherit certain abstract types and implement the
corresponding interfaces. ``graph_node`` is the base class for any other node type; its interfaces
always have to be implemented. If a node sends messages to other nodes, it has to implement the ``sender``
interface, while with the ``receiver`` interface the node may accept messages. For nodes that have multiple
inputs and/or outputs, each input port is a ``receiver`` and each output port is a ``sender``.

.. toctree::
    :titlesonly:

    flow_graph/graph_node_cls.rst
    flow_graph/sender.rst
    flow_graph/receiver.rst

Properties
~~~~~~~~~~

Every node in a flow graph has its own properties.

.. toctree::
    :titlesonly:

    flow_graph/forwarding_and_buffering.rst


Functional Nodes
~~~~~~~~~~~~~~~~

Functional nodes do computations in response to input messages (if any), and send the result or a
signal to their successors.

.. toctree::
    :titlesonly:

    flow_graph/continue_node_cls.rst
    flow_graph/func_node_cls.rst
    flow_graph/input_node_cls.rst
    flow_graph/multifunc_node_cls.rst
    flow_graph/async_node_cls.rst

**Auxiliary**

.. toctree::
    :titlesonly:

    flow_graph/functional_node_policies.rst
    flow_graph/node_priorities.rst
    flow_graph/predefined_concurrency_limits.rst
    flow_graph/copy_body_func.rst

Buffering Nodes
~~~~~~~~~~~~~~~

Buffering nodes are designed to accumulate input messages and pass them to successors in a predefined order, depending on the node type.

.. toctree::
    :titlesonly:

    flow_graph/overwrite_node_cls.rst
    flow_graph/write_once_node_cls.rst
    flow_graph/buffer_node_cls.rst
    flow_graph/queue_node_cls.rst
    flow_graph/priority_queue_node_cls.rst
    flow_graph/sequencer_node_cls.rst

Service Nodes
~~~~~~~~~~~~~

These nodes are designed for advanced control of the message flow, such as combining
messages from different paths in a graph or limiting the number of simultaneously processed
messages, as well as for creating reusable custom nodes.

.. toctree::
    :titlesonly:

    flow_graph/limiter_node_cls.rst
    flow_graph/broadcast_node_cls.rst
    flow_graph/join_node_cls.rst
    flow_graph/split_node_cls.rst
    flow_graph/indexer_node_cls.rst
    flow_graph/composite_node_cls.rst

Ports and Edges
---------------

Flow Graph provides an API to manage connections between the nodes.
For nodes that have more than one input or output ports (for example, ``join_node``),
making a connection requires to specify a certain port by using special helper functions.

.. toctree::
    :titlesonly:

    flow_graph/input_port_func.rst
    flow_graph/output_port_func.rst
    flow_graph/make_edge_func.rst
    flow_graph/remove_edge_func.rst

Special Messages Types
----------------------

Flow Graph supports a set of specific message types.

.. toctree::
    :titlesonly:

    flow_graph/continue_msg_cls.rst
    flow_graph/tagged_msg_cls.rst


Examples
--------

.. toctree::
    :titlesonly:

    flow_graph/dependency_flow_graph_example.rst
    flow_graph/message_flow_graph_example.rst
