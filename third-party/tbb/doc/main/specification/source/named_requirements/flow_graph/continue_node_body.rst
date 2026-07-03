.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

================
ContinueNodeBody
================
**[req.continue_node_body]**

A type `Body` satisfies `ContinueNodeBody` if it meets the following requirements:

----------------------------------------------------------------------

**ContinueNodeBody Requirements: Pseudo-Signature, Semantics**

.. namespace:: ContinueNodeBody
	       
.. cpp:function:: Body::Body( const Body& )

    Copy constructor.

.. cpp:function:: Body::~Body()

    Destructor.

.. cpp:function:: Output Body::operator()( const continue_msg &v )

    **Requirements:** The type ``Output`` must be the same as the template type argument ``Output`` of the
    ``continue_node`` instance in which the ``Body`` object is passed during construction.

    Performs operation and returns a value of type Output.

See also:

* :doc:`continue_node class <../../flow_graph/continue_node_cls>`
* :doc:`continue_msg class <../../flow_graph/continue_msg_cls>`
