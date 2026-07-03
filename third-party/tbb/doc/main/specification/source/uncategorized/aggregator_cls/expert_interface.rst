.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=================================
aggregator Class Expert Interface
=================================


Syntax
------

.. code:: cpp

   template<typename handler_type>
   class aggregator_ext;


Header
------

.. code:: cpp

   #define TBB_PREVIEW_AGGREGATOR 1
   #include "oneapi/tbb/aggregator.h"


Description
-----------

The extended ``aggregator`` interface is provided for expert-level use of the ``aggregator``.  
It gives the user more control over the operations that are passed to the ``aggregator`` and how those operations 
are handled by the ``aggregator``.  Specifically, instead of an ``execute`` method to pass in a 
particular function, there is a ``process`` method to which any sort of data (derived from ``aggregator_operation``, 
see below) can be passed.  In addition, the user must specify a custom function object that performs the operations specified 
by the data passed in through the ``process`` method.

Members
-------

.. code:: cpp

   namespace oneapi {
   namespace tbb {
     class aggregator_operation {
      public:
       enum aggregator_operation_status {agg_waiting=0,agg_finished};
       aggregator_operation();
       void start();
       void finish();
       aggregator_operation* next();
       void set_next(aggregator_operation* n);
     };
   
     template<typename handler_type>
     class aggregator_ext {
      public:
       aggregator_ext(const handler_type& h);
       void process(aggregator_operation *op);
     };
   } // namespace tbb
   } // namespace oneapi

The following table provides additional information on the
members of this class.

= ========================================================================================
\ Member, Description
==========================================================================================
\ ``aggregator_ext(const handler_type& h)``
  \
  Constructs an ``aggregator_ext`` object that uses 
  handler ``h`` to handle operations.
------------------------------------------------------------------------------------------
\ ``void process(aggregator_operation* op)``
  \
  Submits data about an operation in ``op`` to ``aggregator_ext`` 
  to be executed in a mutually exclusive fashion.  Returns after ``op`` has been handled.
------------------------------------------------------------------------------------------
\ ``aggregator_operation::aggregator_operation()``
  \
  Constructs a base ``aggregator_operation`` object.
------------------------------------------------------------------------------------------
\ ``void aggregator_operation::start()``
  \
  Prepares the ``aggregator_operation`` object to be handled.
------------------------------------------------------------------------------------------
\ ``void aggregator_operation::finish()``
  \
  Prepares the ``aggregator_operation`` object to be released to its originating thread.
------------------------------------------------------------------------------------------
\ ``aggregator_operation* aggregator_operation::next()``
  \
  The next ``aggregator_operation`` following ``this``.
------------------------------------------------------------------------------------------
\ ``void aggregator_operation::set_next(aggregator_operation* n)``
  \
  Makes ``n`` the next ``aggregator_operation`` following ``this``.
------------------------------------------------------------------------------------------
= ========================================================================================


Example
-------

The following example uses an ``aggregator_ext`` to safely operate on a non-concurrent ``std::priority_queue`` container.

.. code:: cpp

   typedef priority_queue<value_type, vector<value_type>, compare_type> pq_t;
   pq_t my_pq;
   value_type elem = 42;
   
   // The operation data, derived from aggregator_node
   class op_data : public aggregator_node
    public:
     value_type* elem;
     bool success, is_push;
     op_data(value_type* e, bool push=false) : 
       elem(e), success(false), is_push(push) {}
   };
   
   // A handler to pass in the aggregator_ext template
   class my_handler_t {
     pq_t *pq;
    public:
     my_handler_t() {}
     my_handler_t(pq_t *pq_) : pq(pq_) {}
     void operator()(aggregator_node* op_list) {
       op_data* tmp;
       while (op_list) {
         tmp = (op_data*)op_list;
         op_list = op_list->next();
         tmp->start();
         if (tmp->is_push) pq->push(*(tmp->elem));
         else {
           if (!pq->empty()) {
             tmp->success = true;
             *(tmp->elem) = pq->top();
             pq->pop();
           }
         }
         tmp->finish();
       }
     }
   };
   
   // create the aggregator_ext and initialize with handler instance
   aggregator_ext<my_handler_t> my_aggregator(my_handler_t(my_pq));
   
   // push elem onto the priority queue
   op_data my_push_op(&elem, true);
   my_aggregator.process(&my_push_op);
   
   // pop an elem from the priority queue
   bool result;
   op_data my_pop_op(&elem);
   my_aggregator.process(&my_pop_op);
   result = my_pop_op.success;

There are several important things to note in this example.  Most importantly is the handler 
algorithm, which must conform to what is shown above.  Specifically, the handler must receive a 
linked list of ``aggregator_nodes``, and it must process all the nodes in the list.  The ordering of 
this processing is up to the user, but all the nodes must be processed before the handler returns.  
The ``next`` and ``set_next`` methods on ``aggregator_node`` should be used 
for all manipulations of nodes in the list.

Further, to process an ``aggregator_node``, the user must first call the method ``start`` on the node.  
Then, the user can handle the operation associated with the node in whatever way necessary.  When 
this is complete, call the method ``finish`` on the node.  The ``finish`` method releases the 
node to its originating thread, causing that thread's invocation of the ``process`` method to return.

The ``handler`` function in the example above illustrates this process in the simplest fashion: 
loops over the list handling each operation in turn, calls ``start`` before working with the information 
contained in the node, calls ``finish`` when done with the node.

See also:

* :doc:`aggregator Class Basic Interface <basic_interface>`
