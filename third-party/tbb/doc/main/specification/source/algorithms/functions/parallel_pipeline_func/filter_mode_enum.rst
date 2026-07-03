.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===========
filter_mode
===========
**[algorithms.parallel_pipeline.filter_mode]**

A ``filter_mode`` enumeration represents an execution mode of a ``filter`` in a ``parallel_pipeline`` algorithm.

Its enumerated values and their meanings are as follows:

* A ``parallel`` filter can process multiple items in parallel and without a particular order.
* A ``serial_out_of_order`` filter processes items one at a time and without a particular order.
* A ``serial_in_order`` filter processes items one at a time. The order in which items are processed
  is implicitly set by the first `serial_in_order` filter and respected by all other such filters in the pipeline.

.. code:: cpp

    // Defined in header <oneapi/tbb/parallel_pipeline.h>
    
    namespace oneapi {
        namespace tbb {

            enum class filter_mode {
                parallel = /*implementation-defined*/,
                serial_in_order = /*implementation-defined*/,
                serial_out_of_order = /*implementation-defined*/
            };

        } // namespace tbb
    } // namespace oneapi
