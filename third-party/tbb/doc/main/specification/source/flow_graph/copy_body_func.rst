.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=========
copy_body
=========
**[flow_graph.copy_body]**

``copy_body`` is a function template that returns a copy of the body function object from the following
nodes:

* :doc:`continue_node <continue_node_cls>`
* :doc:`function_node <func_node_cls>`
* :doc:`multifunction_node <multifunc_node_cls>`
* :doc:`input_node <input_node_cls>`
* :doc:`async_node <async_node_cls>`

.. code:: cpp

    namespace oneapi {
    namespace tbb {
    namespace flow {

        // Defined in header <oneapi/tbb/flow_graph.h>

        template< typename Body, typename Node >
        Body copy_body( Node &n );

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi
