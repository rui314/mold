.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=================
parallel_pipeline
=================
**[algorithms.parallel_pipeline]**

Strongly-typed interface for pipelined execution.

.. code:: cpp

    // Defined in header <oneapi/tbb/parallel_pipeline.h>

    namespace oneapi {
        namespace tbb {

            void parallel_pipeline( size_t max_number_of_live_tokens, const filter<void,void>& filter_chain );
            void parallel_pipeline( size_t max_number_of_live_tokens, const filter<void,void>& filter_chain, task_group_context& context );

        } // namespace tbb
    } // namespace oneapi

A ``parallel_pipeline`` algorithm represents pipelined application of a series of filters to a stream of items.
Each filter operates in a particular mode: parallel, serial in-order, or serial out-of-order.

To build and run a pipeline from functors *g*\ :sub:`0`, *g*\ :sub:`1`, *g*\ :sub:`2`, ..., *g*\ :sub:`n`, write:

.. code:: cpp

   parallel_pipeline( max_number_of_live_tokens,
                      make_filter<void,I1>(mode0,g0) &
                      make_filter<I1,I2>(mode1,g1) &
                      make_filter<I2,I3>(mode2,g2) &
                      ...
                      make_filter<In,void>(moden,gn) );

In general, the *g*\ :sub:`i` functor should define its ``operator()`` to map objects of type
*I*\ :sub:`i` to objects of type *I*\ :sub:`i+1`. Functor *g*\ :sub:`0` is a special case, because
it notifies the pipeline when the end of an input stream is reached. Functor *g*\ :sub:`0` must
be defined such that for a *flow_control* object *fc*, the expression *g*\ :sub:`0` (*fc* ) either
returns the next value in the input stream, or invokes *fc.stop()* if the end of the input stream is reached
and returns a dummy value.

Each `filter` should be specified by two template arguments. These arguments define filters input
and output types. The first and last filters are special cases. Input type of the first filter must be `void`,
output type of the last filter must be `void` too.

Before passing to `parallel_pipeline`, concatenate all filters to one(``filter<void, void>``)
with `filter::operator&()`. The operator requires that the second template argument of its left operand matches
the first template argument of its second operand.

The number of items processed in parallel depends on the structure of the pipeline and number of available threads.
*max_number_of_live_tokens* sets the threshold for concurrently processed items.

If the *context* argument is specified, pipeline's tasks are executed in this context. By default, the algorithm
is executed in a bound context of its own.

Example
-------

The following example uses ``parallel_pipeline`` to compute the root-mean-square of a sequence defined
by [ *first* , *last* ).

.. code:: cpp

    float RootMeanSquare( float* first, float* last ) {
        float sum=0;
        parallel_pipeline( /*max_number_of_live_token=*/16,
            make_filter<void,float*>(
                filter_mode::serial_in_order,
                [&](flow_control& fc)-> float*{
                    if( first<last ) {
                        return first++;
                    } else {
                        fc.stop();
                        return nullptr;
                    }
                }
            ) &
            make_filter<float*,float>(
                filter_mode::parallel,
                [](float* p){return (*p)*(*p);} 
            ) &
            make_filter<float,void>(
                filter_mode::serial_in_order,
                [&](float x) {sum+=x;}
            )
        );
        return sqrt(sum);
    }

filter Class Template
---------------------

.. toctree::
    :titlesonly:

    parallel_pipeline_func/filter_cls.rst

flow_control Class
------------------

.. toctree::
    :titlesonly:

    parallel_pipeline_func/flow_control_cls.rst

See also:

* :doc:`task_group_context <../../task_scheduler/scheduling_controls/task_group_context_cls>`
