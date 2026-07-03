.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==============
composite_node
==============
**[flow_graph.composite_node]**

A node that encapsulates a collection of other nodes as a first class graph node.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        template< typename InputTuple, typename OutputTuple > class composite_node;

        // composite_node with both input ports and output ports
        template< typename... InputTypes, typename... OutputTypes>
        class composite_node <std::tuple<InputTypes...>, std::tuple<OutputTypes...> > : public graph_node {
        public:
            typedef std::tuple< receiver<InputTypes>&... > input_ports_type;
            typedef std::tuple< sender<OutputTypes>&... > output_ports_type;

            composite_node( graph &g );
            virtual ~composite_node();

            void set_external_ports(input_ports_type&& input_ports_tuple, output_ports_type&& output_ports_tuple);
            input_ports_type& input_ports();
            output_ports_type& output_ports();
        };

        // composite_node with only input ports
        template< typename... InputTypes>
        class composite_node <std::tuple<InputTypes...>, std::tuple<> > : public graph_node{
        public:
            typedef std::tuple< receiver<InputTypes>&... > input_ports_type;

            composite_node( graph &g );
            virtual ~composite_node();

            void set_external_ports(input_ports_type&& input_ports_tuple);
            input_ports_type& input_ports();
        };

        // composite_nodes with only output_ports
        template<typename... OutputTypes>
        class composite_node <std::tuple<>, std::tuple<OutputTypes...> > : public graph_node{
        public:
            typedef std::tuple< sender<OutputTypes>&... > output_ports_type;

            composite_node( graph &g );
            virtual ~composite_node();

            void set_external_ports(output_ports_type&& output_ports_tuple);
            output_ports_type& output_ports();
        };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

* The ``InputTuple`` and ``OutputTuple`` must be instantiations of ``std::tuple``.

``composite_node`` is a ``graph_node``, ``receiver<T>``, and ``sender<T>``.

The ``composite_node`` can package any number of other nodes. It maintains input and output port
references to nodes in the package that border the ``composite_node``. This allows the references to
be used to make edges to other nodes outside of the ``composite_node``. The ``InputTuple`` is a
tuple of input types. The ``composite_node`` has an input port for each type in ``InputTuple``.
Likewise, the ``OutputTuple`` is a tuple of output types. The ``composite_node`` has an output port
for each type in ``OutputTuple``.

The composite_node is a multi-port node with three specializations.

* **A multi-port node with multi-input ports and multi-output ports:** This specialization has a tuple of
  input ports, each of which is a ``receiver`` of a type in
  ``InputTuple``. Each input port is a reference to a port of a
  node that the ``composite_node`` encapsulates. Similarly, this specialization also has a tuple
  of output ports, each of which is a ``sender`` of a type in
  ``OutputTuple``. Each output port is a reference to a port of a
  node that the ``composite_node`` encapsulates.
* **A multi-port node with only input ports and no output ports:** This specialization only has a tuple of
  input ports.
* **A multi-port node with only output ports and no input_ports:** This specialization only has a tuple of
  output ports.

The function template :doc:`input_port <input_port_func>` can be used to get a reference to
a specific input port and the function template :doc:`output_port <output_port_func>` can be
used to get a reference to a specific output port.

Construction of a ``composite_node`` is done in two stages:

* Defining the ``composite_node`` with specification of ``InputTuple`` and ``OutputTuple``.
* Making aliases from the encapsulated nodes that border the ``composite_node`` to the input and
  output ports of the ``composite_node``. This step is mandatory as without it the ``composite_node``
  input and output ports are not bound to any actual nodes. Making the aliases is achieved
  by calling the method ``set_external_ports``.

The composite_node does not meet the `CopyConstructible` requirements from [copyconstructible]
ISO C++ Standard section.

Member functions
----------------

.. cpp:function:: composite_node( graph &g )

    Constructs a ``composite_node`` that belongs to the graph ``g``.

.. cpp:function:: void set_external_ports(input_ports_type&& input_ports_tuple, output_ports_type&& output_ports_tuple)

    Creates input and output ports of the ``composite_node`` as
    aliases to the ports referenced by ``input_ports_tuple`` and
    ``output_ports_tuple``, respectively. That is, a port referenced at
    position ``N`` in ``input_ports_tuple`` is mapped as the ``Nth``
    input port of the ``composite_node``, similarly for output ports.

.. cpp:function:: input_ports_type& input_ports()

    **Returns**: A ``std::tuple`` of ``receivers``. Each element is a
    reference to the actual node or input port that was aliased to
    that position in ``set_external_ports()``.

  .. caution::

    Calling ``input_ports()`` without a prior call to ``set_external_ports()``
    results in undefined behavior.

.. cpp:function:: output_ports_type& output_ports()

    **Returns**: A ``std::tuple`` of ``senders``. Each element is a
    reference to the actual node or output port that was aliased to
    that position in ``set_external_ports()``.

    .. caution::

        Calling ``output_ports()`` without a prior call to ``set_external_ports()`` results in undefined behavior.

See also:

* :doc:`input_port function template <input_port_func>`
* :doc:`output_port function template <output_port_func>`
