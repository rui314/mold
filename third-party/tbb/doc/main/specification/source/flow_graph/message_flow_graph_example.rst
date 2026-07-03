.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==========================
Message Flow Graph Example
==========================

This example calculates the sum ``x*x + x*x*x`` for all ``x = 1 to 10``. The layout of this example is shown in
the figure below.

.. figure:: ../Resources/message_flow_graph.jpg
   :width: 454
   :height: 252
   :align: center

   A simple message flow graph.

Each value enters through the ``broadcast_node<int>`` ``input``. This node broadcasts the value to both
``squarer`` and ``cuber``, which calculate ``x*x`` and ``x*x*x``, respectively. The output of each
of these nodes is put to one of ``join``'s ports. A tuple containing both values is
created by ``join_node<std::tuple<int,int>> join`` and forwarded to ``summer``, which adds both
values to the running total. Both ``squarer`` and ``cuber`` allow unlimited concurrency, that is they each
may process multiple values simultaneously. The final ``summer``, which updates a shared total, is only allowed
to process a single incoming tuple at a time, eliminating the need for a lock around the shared value.

.. code:: cpp

    #include <cstdio>
    #include "oneapi/tbb/flow_graph.h"

    using namespace oneapi::tbb::flow;

    struct square {
      int operator()(int v) { return v*v; }
    };

    struct cube {
      int operator()(int v) { return v*v*v; }
    };

    class sum {
      int &my_sum;
    public:
      sum( int &s ) : my_sum(s) {}
      int operator()( std::tuple<int, int> v ) {
        my_sum += get<0>(v) + get<1>(v);
        return my_sum;
      }
    };

    int main() {
      int result = 0;

      graph g;
      broadcast_node<int> input(g);
      function_node<int,int> squarer( g, unlimited, square() );
      function_node<int,int> cuber( g, unlimited, cube() );
      join_node<std::tuple<int,int>, queueing> join( g );
      function_node<std::tuple<int,int>,int>
          summer( g, serial, sum(result) );

      make_edge( input, squarer );
      make_edge( input, cuber );
      make_edge( squarer, get<0>( join.input_ports() ) );
      make_edge( cuber, get<1>( join.input_ports() ) );
      make_edge( join, summer );

      for (int i = 1; i <= 10; ++i)
          input.try_put(i);
      g.wait_for_all();

      printf("Final result is %d\n", result);
      return 0;
    }

In the example code above, the classes ``square``, ``cube``, and ``sum`` define the three
user-defined operations. Each class is used to create a ``function_node``.

In function ``main``, the flow graph is set up and then the values 1-10 are put into the node
``input``. All the nodes in this example pass around values of type ``int``. The nodes used in
this example are all class templates and therefore can be used with any type that supports copy
construction, including pointers and objects.
