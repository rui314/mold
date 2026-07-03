.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

======================
JoinNodeFunctionObject
======================
**[req.join_node_function_object]**

A type `Func` satisfies `JoinNodeFunctionObject` if it meets the following requirements:

----------------------------------------------------------------------

**JoinNodeFunctionObject Requirements: Pseudo-Signature, Semantics**

.. cpp:function:: Func::Func( const Func& )

    Copy constructor.

.. cpp:function:: Func::~Func()

    Destructor.

.. cpp:function:: Key Func::operator()( const Input& v )

    **Requirements:** The ``Key`` and ``Input`` types must be the same as the ``K`` and the corresponding
    element of the ``OutputTuple`` template arguments of the ``join_node`` instance to which the ``Func`` object is passed
    during construction.

    Returns key to be used for hashing input messages.
