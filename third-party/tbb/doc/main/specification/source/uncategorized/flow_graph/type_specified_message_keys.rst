.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=========================================
Type-specified message keys for join_node
=========================================


Summary
-------

The extension allows a key matching join_node to obtain keys via functions associated with
its input types. The extension simplifies the existing approach by removing the need to
provide a function object for each input port of join_node.

Header
------


.. code:: cpp

   #define TBB_PREVIEW_FLOW_GRAPH_FEATURES 1
   #include "oneapi/tbb/flow_graph.h"


Description
-----------

The extension adds a special constructor to the join_node interface when the
``key_matching<typename K, class KHash=tbb_hash_compare>`` policy is
used. The constructor has the following signature:
``join_node( graph &g )``.
When constructed this way, a ``join_node`` calls the
``key_from_message`` function for each incoming message to obtain an associated
key. The default implementation of ``key_from_message`` is

.. code:: cpp

   namespace oneapi {
   namespace tbb {
       namespace flow {
           template <typename K, typename T>
           K key_from_message( const T &t ) {
                 return t.key();
           }
       }
   } // namespace tbb
   } // namespace oneapi

Where ``T`` is one of the user-provided types in ``OutputTuple``
used for the ``join_node`` construction and ``K`` is the key type
for the node.
By default, the ``key()`` method defined in the message class is called.
Alternatively, users can define their own ``key_from_message`` function in the
same namespace with the message type. This function is found via C++ argument-dependent
lookup and used instead of the default implementation.

.. code:: cpp

   struct MyMessageType {
       int my_key;
       int my_value;
       // The following method will not be called because ADL will find
       // the custom implementation of the key_from_message function below.
       int key() const {
           // This method should not be called.
           assert(false);
           return 0;
       }
   };
   
   // The custom implementation of the key_from_message function.
   template <typename K>
   int key_from_message(const MyMessageType &m) {
       return m.my_key;
   }


Example
-------


.. code:: cpp

   #define TBB_PREVIEW_FLOW_GRAPH_FEATURES 1
   #include "oneapi/tbb/flow_graph.h"
   #include <cstdio>
   #include <cassert>
   
   class MyMessage {
       int my_key;
       float my_value;
   public:
       MyMessage( int k = 0, float v = 0 ) : my_key( k ), my_value( v ) {}
       int key() const {
           return my_key;
       }
       float value() const {
           return my_value;
       }
   };
   
   int main() {
       using namespace oneapi::tbb::flow;
   
       graph g;
       function_node<int, MyMessage>
           f1( g, unlimited, []( int i ) { return MyMessage( i, (float)i ); } );
       function_node<int, MyMessage>
           f2( g, unlimited, []( int i ) { return MyMessage( i, (float)2 * i ); } );
   
       function_node< tuple<MyMessage, MyMessage> >
           f3( g, unlimited,
           []( const tuple<MyMessage, MyMessage> &t ) {
           assert( get<0>( t ).key() == get<1>( t ).key() );
           std::printf( "The result is %f for key %d\n", get<0>( t ).value() + get<1>( t ).value(), get<0>( t ).key() );
       } );
   
       join_node< tuple<MyMessage, MyMessage>, key_matching<int> > jn( g );
   
       make_edge( f1, input_port<0>( jn ) );
       make_edge( f2, input_port<1>( jn ) );
       make_edge( jn, f3 );
   
       f1.try_put( 1 );
       f1.try_put( 2 );
       f2.try_put( 2 );
       f2.try_put( 1 );
   
       g.wait_for_all();
   }

In the example, a key matching ``join_node`` is used to pair messages with the
same key. The ``join_node`` uses the type-specified message keys extension and
calls the ``MyMessage::key`` method to obtain the keys.

See also:

* :doc:`join_node Template Class <../../../../flow_graph/join_node_cls>`
