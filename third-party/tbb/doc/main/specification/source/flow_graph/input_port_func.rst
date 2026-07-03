.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==========
input_port
==========
**[flow_graph.input_port]**

A template function that returns a reference to a specific input port of a given
``join_node``, ``indexer_node`` or ``composite_node``.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        template<size_t N, typename NodeType>
        /*implementation-defined*/& input_port(NodeType &n);

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

See also:

* :doc:`join_node template class <join_node_cls>`
* :doc:`indexer_node template class <indexer_node_cls>`
* :doc:`composite_node template class <composite_node_cls>`
