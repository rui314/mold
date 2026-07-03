.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============
AsyncNodeBody
=============
**[req.async_node_body]**

A type `Body` satisfies `AsyncNodeBody` if it meets the following requirements:

----------------------------------------------------------------------

**AsyncNodeBody Requirements: Pseudo-Signature, Semantics**

.. namespace:: AsyncNodeBody
	       
.. cpp:function:: Body::Body( const Body& )

    Copy constructor.

.. cpp:function:: Body::~Body()

    Destructor.

.. cpp:function:: void Body::operator()( const Input &v, GatewayType &gateway )

    **Requirements:** 

    * The ``Input`` type must be the same as the ``Input`` template type argument
      of the ``async_node`` instance in which the ``Body`` object is passed during construction.
    * The ``GatewayType`` type must be the same as the ``gateway_type`` member type
      of the ``async_node`` instance in which the ``Body`` object is passed during construction.

    The input value ``v`` is submitted by the flow graph to an external activity.
    The :doc:`gateway interface <gateway_type>` allows the external activity to communicate
    with the enclosing flow graph.
