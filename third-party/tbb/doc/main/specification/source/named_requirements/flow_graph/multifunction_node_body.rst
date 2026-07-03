.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=====================
MultifunctionNodeBody
=====================
**[req.multifunction_node_body]**

A type `Body` satisfies `MultifunctionNodeBody` if it meets the following requirements:

----------------------------------------------------------------------

**MultifunctionNodeBody Requirements: Pseudo-Signature, Semantics**

.. namespace:: MultifunctionNodeBody
   
.. cpp:function:: Body::Body( const Body& )

    Copy constructor.

.. cpp:function:: Body::~Body()

    Destructor.

.. cpp:function:: void Body::operator()(const Input &v, OutputPortsType &p)

    **Requirements:** 

    * The ``Input`` type must be the same as the ``Input`` template type argument
      of the ``multifunction_node`` instance in which the ``Body`` object is passed during construction.
    * The ``OutputPortsType`` type must be the same as the ``output_ports_type`` member type
      of the ``multifunction_node`` instance in which the ``Body`` object is passed during construction.

    Performs operation on ``v``. May call ``try_put()`` on zero or more of
    the output ports. May call ``try_put()`` on any output port multiple
    times.
