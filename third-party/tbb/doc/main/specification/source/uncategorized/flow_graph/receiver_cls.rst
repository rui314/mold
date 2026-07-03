.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=======================
receiver Template Class
=======================


Summary
-------

.. caution::

   This feature is deprecated and will be reworked or removed in the future.

An abstract base class for nodes that act as message receivers.

Syntax
------

.. code:: cpp

   template< typename T > class receiver;


Header
------

.. code:: cpp

   #include "oneapi/tbb/flow_graph.h"


Description
-----------

The 
``receiver`` template class is an abstract base class
that defines the interface for nodes that can act as receivers. Default
implementations for several functions are provided.

.. caution::

   The use of ``register_predecessor`` to build
   graphs has been deprecated for ``flow::graph``.  
   Graphs should be constructed with ``make_edge`` and 
   ``remove_edge``.
   Replace ``n1.register_predecessor(n2)`` with ``make_edge(n2,n1)``.


Members
-------


.. code:: cpp

   namespace oneapi {
   namespace tbb {
   namespace flow {
    
   template< typename T >
   class receiver {
   public:
       typedef T input_type;
       typedef sender<input_type> predecessor_type;
       virtual ~receiver();
       virtual bool try_put( const input_type &v ) = 0;
       virtual bool register_predecessor( predecessor_type &p ) {
           return false; }
       virtual bool remove_predecessor( predecessor_type &p ) {
           return false; }
   };
    
   } // namespace flow
   } // namespace tbb
   } // namespace oneapi


The following table provides additional information on the
members of this template class.

= ========================================================================================
\ Member, Description
==========================================================================================
\ ``~receiver()``
  \
  The destructor.
------------------------------------------------------------------------------------------
\ ``bool try_put( const input_type &v )``
  \
  A method that represents the interface for
  putting an item to a receiver.
------------------------------------------------------------------------------------------
\ ``bool register_predecessor( predecessor_type &p )``
  \
  Adds a predecessor to the node's set of predecessors.
  
  **Returns**: 
  ``true`` if the predecessor is added. 
  ``false``, otherwise. The default
  implementation returns ``false``.
------------------------------------------------------------------------------------------
\ ``bool remove_predecessor( predecessor_type &p )``
  \
  Removes a predecessor from the node's set of predecessors.
  
  **Returns**: 
  ``true`` if the predecessor is removed. 
  ``false``, otherwise. The default
  implementation returns ``false``.
------------------------------------------------------------------------------------------
= ========================================================================================
