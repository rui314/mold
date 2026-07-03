.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=====================
sender Template Class
=====================


Summary
-------

.. caution::

   This feature is deprecated and will be reworked or removed in the future.

An abstract base class for nodes that act as message senders.

Syntax
------

.. code:: cpp

   template< typename T > class sender;


Header
------

.. code:: cpp

   #include "oneapi/tbb/flow_graph.h"


Description
-----------

The 
``sender`` template class is an abstract base class that
defines the interface for nodes that can act as senders. Default
implementations for several functions are provided.

.. caution::

   The use of ``register_successor`` to build
   graphs has been deprecated for ``flow::graph``.  
   Graphs should be constructed with ``make_edge`` and 
   ``remove_edge``.
   Replace ``n1.register_successor(n2)`` with ``make_edge(n1,n2)``.


Members
-------


.. code:: cpp

   namespace oneapi {
   namespace tbb {
   namespace flow {
    
   template< typename T >
   class sender {
   public:
       typedef T output_type;
       typedef receiver<output_type> successor_type;
       virtual ~sender();
       virtual bool register_successor( successor_type &r ) = 0;
       virtual bool remove_successor( successor_type &r ) = 0;
       virtual bool try_get( output_type &v ) { return false; }
       virtual bool try_reserve( output_type &v ) { return false; }
       virtual bool try_release( ) { return false; }
       virtual bool try_consume( ) { return false; }
   };
    
   } // namespace flow
   } // namespace tbb
   } // namespace oneapi


The following table provides additional information on the
members of this template class.

= ========================================================================================
\ Member, Description
==========================================================================================
\ ``~sender()``
  \
  The destructor.
------------------------------------------------------------------------------------------
\ ``bool register_successor( successor_type &r ) = 0``
  \
  A pure virtual method that describes the interface for
  adding a successor node to the set of successors for the sender.
  
  **Returns**: 
  ``true`` if the successor is added; 
  ``false`` otherwise.
------------------------------------------------------------------------------------------
\ ``bool remove_successor( successor_type &r ) = 0``
  \
  A pure virtual method that describes the interface for
  removing a successor node from the set of successors for a sender.
  
  **Returns**: 
  ``true`` if the successor is removed. 
  ``false``, otherwise.
------------------------------------------------------------------------------------------
\ ``bool try_get( output_type &v )``
  \
  Requests an item from a sender.
  
  **Returns**: The default implementation returns 
  ``false``.
------------------------------------------------------------------------------------------
\ ``bool try_reserve( output_type &v )``
  \
  Reserves an item at the sender.
  
  **Returns**: The default implementation returns 
  ``false``.
------------------------------------------------------------------------------------------
\ ``bool try_release( )``
  \
  Releases the reservation held at the sender.
  
  **Returns**: The default implementation returns 
  ``false``.
------------------------------------------------------------------------------------------
\ ``bool try_consume( )``
  \
  Consumes the reservation held at the sender.
  
  **Returns**: The default implementation returns 
  ``false``.
------------------------------------------------------------------------------------------
= ========================================================================================
