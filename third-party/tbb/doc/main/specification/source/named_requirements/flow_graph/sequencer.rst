.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=========
Sequencer
=========
**[req.sequencer]**

A type `S` satisfies `Sequencer` if it meets the following requirements:

----------------------------------------------------------------------

**Sequencer Requirements: Pseudo-Signature, Semantics**

.. cpp:function:: S::S( const S& )

    Copy constructor.

.. cpp:function:: S::~S()

    Destructor.

.. cpp:function:: size_t S::operator()( const T &v )

    **Requirements:** The type ``T`` must be the same as the template type argument ``T`` of the
    ``sequencer_node`` instance in which the ``S`` object is passed during construction.

    Returns the sequence number for the provided message ``v``.

See also:

* :doc:`sequencer_node class <../../flow_graph/sequencer_node_cls>`
