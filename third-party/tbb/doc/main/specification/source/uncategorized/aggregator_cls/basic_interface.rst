.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

================================
aggregator Class Basic Interface
================================


Syntax
------

.. code:: cpp

   class aggregator;


Header
------

.. code:: cpp

   #define TBB_PREVIEW_AGGREGATOR 1
   #include "oneapi/tbb/aggregator.h"


Description
-----------

An ``aggregator`` is similar to a ``mutex`` in that it allows mutually 
exclusive execution of operations, however the interface is quite different.  In particular, operations 
(in the form of function bodies or lambda functions) are passed to an ``aggregator`` for 
execution via an ``execute`` method on the ``aggregator`` object.  Operations 
passed to the same ``aggregator`` object are executed mutually exclusively.  
The ``execute`` method returns after the function passed to it has completed execution.

Members
-------

.. code:: cpp

   namespace oneapi {
   namespace tbb {
     class aggregator {
     public:
       aggregator();
       template<typename Body> 
       void execute(const Body& b);
     };
   } // namespace tbb
   } // namespace oneapi

The following table provides additional information on the
members of this class.

= ========================================================================================
\ Member, Description
==========================================================================================
\ ``aggregator()``
  \
  Constructs an ``aggregator`` object.
------------------------------------------------------------------------------------------
\ ``template<typename Body> void execute(const Body& b)``
  \
  Submits ``b`` to the ``aggregator`` to be executed in a mutually 
  exclusive fashion.  Returns after ``b`` has been executed.
------------------------------------------------------------------------------------------
= ========================================================================================


Example
-------

The following example uses an ``aggregator`` to safely operate on a non-concurrent ``std::priority_queue`` container.

.. code:: cpp

   typedef priority_queue<value_type, vector<value_type>, compare_type> pq_t;
   pq_t my_pq;
   aggregator my_aggregator;
   value_type elem = 42;
   
   // push elem onto the priority queue
   my_aggregator.execute( [&my_pq, &elem](){ my_pq.push(elem); } );
   
   // pop an elem from the priority queue
   bool result = false;
   my_aggregator.execute( [&my_pq, &elem, &result](){
     if (!my_pq.empty()) {
       result = true;
       elem = my_pq.top();
       my_pq.pop();
     }
   } );


See also:

* :doc:`aggregator Class Expert Interface <expert_interface>`
