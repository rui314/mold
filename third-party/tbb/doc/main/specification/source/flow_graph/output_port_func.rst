.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===========
output_port
===========
**[flow_graph.output_port]**

A template function that returns a reference to a specific output port of a given
``split_node``, ``indexer_node``, or ``composite_node``.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        template<size_t N, typename NodeType>
        /*implementation-defined*/& output_port(NodeType &n);

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi
 
See also:

* :doc:`split_node Template Class <split_node_cls>`
* :doc:`multifunction_node Template Class <multifunc_node_cls>`
* :doc:`composite_node Template Class <composite_node_cls>`
