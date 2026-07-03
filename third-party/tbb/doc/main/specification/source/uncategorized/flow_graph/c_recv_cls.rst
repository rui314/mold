.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=======================
continue_receiver Class
=======================


Summary
-------

.. caution::

   This feature is deprecated and will be reworked or removed in the future.

An abstract base class for nodes that act as receivers of 
``continue_msg`` objects. These nodes call a method 
``execute`` when the number of 
``try_put`` calls reaches a threshold that represents
the number of known predecessors.

Syntax
------

.. code:: cpp

   class continue_receiver;


Header
------

.. code:: cpp

   #include "oneapi/tbb/flow_graph.h"


Description
-----------

This type of node is triggered when its method 
``try_put`` has been called a number of times that is
equal to the number of known predecessors. When triggered, the node calls the
method 
``execute,`` then resets and fires again when it
receives the correct number of 
``try_put`` calls. This node type is useful for
dependency graphs, where each node must wait for its predecessors to complete
before executing, but no explicit data is passed across the edge.

Members
-------

.. code:: cpp

   namespace oneapi {
   namespace tbb {
   namespace flow {
    
   class continue_receiver : public receiver< continue_msg > {
   public:
       typedef continue_msg input_type;
       typedef sender< input_type > predecessor_type;
       explicit continue_receiver( int num_predecessors = 0 );
       continue_receiver( const continue_receiver &src );
       virtual ~continue_receiver();
       virtual bool try_put( const input_type &v );
       virtual bool register_predecessor( predecessor_type &p );
       virtual bool remove_predecessor( predecessor_type &p );
    
   protected:
       virtual void execute() = 0;
   };
    
   } // namespace flow
   } // namespace tbb
   } // namespace oneapi

The following table provides additional information on the
members of this class.

= ========================================================================================
\ Member, Description
==========================================================================================
\ ``explicit continue_receiver( int num_predecessors = 0 )``
  \
  Constructs a 
  ``continue_receiver`` that is initialized to
  trigger after receiving 
  ``num_predecessors`` calls to 
  ``try_put``.
------------------------------------------------------------------------------------------
\ ``continue_receiver( const continue_receiver &src )``
  \
  Constructs a 
  ``continue_receiver`` that has the same
  initial state that 
  ``src`` had after its construction. It does
  not copy the current count of 
  ``try_puts`` received, or the current known
  number of predecessors. The 
  ``continue_receiver`` that is constructed will
  only have a non-zero threshold if 
  ``src`` was constructed with a non-zero
  threshold.
------------------------------------------------------------------------------------------
\ ``~continue_receiver( )``
  \
  The destructor
------------------------------------------------------------------------------------------
\ ``bool try_put( const input_type &v )``
  \
  Increments the count of 
  ``try_put`` calls received. If the incremented
  count is equal to the number of known predecessors, a call is made to 
  ``execute`` and the internal count of 
  ``try_put`` calls is reset to zero. This
  method performs as if the call to 
  ``execute`` and the updates to the internal
  count occur atomically.
  
  **Returns**: 
  ``true``
------------------------------------------------------------------------------------------
\ ``bool register_predecessor( predecessor_type &p )``
  \
  Increments the number of known predecessors.
  
  **Returns**: 
  ``true``
------------------------------------------------------------------------------------------
\ ``bool remove_predecessor( predecessor_type &p )``
  \
  Decrements the number of known predecessors.
  
  .. caution::

     The method 
     ``execute`` is not called if the count of 
     ``try_put`` calls received becomes equal to
     the number of known predecessors as a result of this call. That is, a call to 
     ``remove_predecessor`` never calls 
     ``execute``.
  
------------------------------------------------------------------------------------------
\ ``void execute() = 0``
  \
  A pure virtual method that is called when the number of 
  ``try_put`` calls is equal to the number of
  known predecessors. Must be overridden by the child class.
  
  .. caution::

     This method should be very fast or else spawn a task to
     offload its work, since this method is called while the sender is blocked on 
     ``try_put``.
  
------------------------------------------------------------------------------------------
= ========================================================================================
