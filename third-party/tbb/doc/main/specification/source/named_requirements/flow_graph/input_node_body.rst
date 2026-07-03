.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============
InputNodeBody
=============
**[req.input_node_body]**

A type `Body` satisfies `InputNodeBody` if it meets the following requirements:

----------------------------------------------------------------------

**InputNodeBody Requirements: Pseudo-Signature, Semantics**

.. namespace:: InputNodeBody
	       
.. cpp:function:: Body::Body( const Body& )

    Copy constructor.

.. cpp:function:: Body::~Body()

    Destructor.

.. cpp:function:: Output Body::operator()( oneapi::tbb::flow_control& fc )

    **Requirements:** The type ``Output`` must be the same as the template type argument ``Output`` of the
    ``input_node`` instance in which the ``Body`` object is passed during construction.

    Applies body to generate the next item. Call ``fc.stop()`` when new element cannot be generated.
    Because ``Output`` needs to be returned, ``Body`` may return any valid value of ``Output``, to be
    immediately discarded.

