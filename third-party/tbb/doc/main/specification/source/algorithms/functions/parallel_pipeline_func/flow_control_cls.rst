.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

============
flow_control
============
**[algorithms.parallel_pipeline.flow_control]**

Enables the first filter in a composite filter to indicate when the end of input stream is reached.

Template function ``parallel_pipeline`` passes a ``flow_control`` object to the functor
of the first filter. When the functor reaches the end of its input stream, it should invoke ``fc.stop()``
and return a dummy value that will not be passed to the next filter.

.. code:: cpp

    // Defined in header <oneapi/tbb/parallel_pipeline.h>
    
    namespace oneapi {
        namespace tbb {

            class flow_control {
            public:
                void stop();
            };

        } // namespace tbb
    } namespace oneapi

Member functions
----------------

.. cpp:function:: void stop()

    Indicates that first filter of the pipeline reaches the end of its output.

See also:

* :doc:`FilterBody requiremnts <../../../named_requirements/algorithms/filter_body>`
* :doc:`filter class <filter_cls>`
