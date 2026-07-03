.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

======
sender
======
**[flow_graph.sender]**

A base class for all nodes that may send messages.

.. code:: cpp

    namespace oneapi {
    namespace tbb {
    namespace flow {

        template< typename T >
        class sender { /*unspecified*/ };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

The ``T`` type is a message type.
