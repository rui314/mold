.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=======================
Function Nodes Policies
=======================
**[flow_graph.function_node_policies]**

``function_node``, ``multifunction_node``, ``async_node`` and ``continue_node`` can be specified
by the ``Policy`` parameter, which is represented as a set of tag classes. This parameter affects behavior
of node execution.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        class queueing { /*unspecified*/ };
        class rejecting { /*unspecified*/ };
        class lightweight { /*unspecified*/ };
        class queueing_lightweight { /*unspecified*/ };
        class rejecting_lightweight { /*unspecified*/ };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

Each policy class satisfies the `CopyConstructible` requirements from [copyconstructible]
ISO C++ Standard sections.

Queueing
--------

This policy defines behavior for input messages acceptance. The ``queueing`` policy means that input
messages that cannot be processed right away are kept to be processed when possible.

Rejecting
---------

This policy defines behavior for input messages acceptance. The ``rejecting`` policy means that input
messages that cannot be processed right away are not accepted by the node and it is responsibility
of a predecessor to handle this.

Lightweight
-----------

This policy allows to specify that the node body takes little time to process, as a non-binding hint
for an implementation to reduce overheads associated with the node execution. Any optimization applied
by an implementation must have no observable side effects on the node and graph execution.

When combined with another policy, the ``lightweight`` policy results in extending the behavior
of that other policy with the optimization hint. This rule automatically applies to functional nodes
that have a default value for the ``Policy`` template parameter. For example, if the default value of
``Policy`` is ``queueing``, specifying ``lightweight`` as the ``Policy`` value is equivalent to
specifying ``queueing_lightweight``.

The function call ``operator()`` of a node body must be ``noexcept`` for lightweight policies to have effect.

Example
~~~~~~~

The example below shows the application of the ``lightweight`` policy to a
graph with a pipeline topology. It is reasonable to apply the ``lightweight``
policy to the second and third nodes because the bodies of these nodes are small. This
allows the second and third nodes to execute without task scheduling overhead. The
``lightweight`` policy is not specified for the first node in order to permit
concurrent invocations of the graph.

.. include:: ./examples/lightweight_policy.cpp
    :code: cpp
