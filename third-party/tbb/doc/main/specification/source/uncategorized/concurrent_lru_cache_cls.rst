.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===================================
concurrent_lru_cache Template Class
===================================


Summary
-------

Template class for Least Recently Used cache with concurrent operations.

Syntax
------

.. code:: cpp

   template <typename key_type, typename value_type, typename value_functor_type = value_type (*)(key_type)>
   class concurrent_lru_cache;


Header
------

.. code:: cpp

   #define TBB_PREVIEW_CONCURRENT_LRU_CACHE 1
   #include "oneapi/tbb/concurrent_lru_cache.h"


Description
-----------

A
``concurrent_lru_cache`` container maps keys to values
with the ability to limit the number of stored unused values. There is at most
one item stored in the container for each key.

The container permits multiple threads to
concurrently retrieve items from it.

The container tracks which items are in use
by returning a proxy ``concurrent_lru_cache::handle`` object
that refers to an item, instead of its real value. Once there are no
``handle`` objects holding reference to an item, it is considered unused.

The container stores all the items that are
currently in use plus a limited number of unused items. Excessive unused items are erased
according to the least recently used policy.

When no item is found for a given key, the container
calls the user-specified ``value_functor_type`` object to construct
a value for the key, and stores that value. The function object must be thread safe.

Members
-------

.. code:: cpp

   namespace oneapi {
   namespace tbb {
       template <typename key_type,
                 typename value_type,
                 typename value_functor_type = value_type (*)(key_type) >
       class concurrent_lru_cache{
       public:
           class handle;
   
           concurrent_lru_cache(value_functor_type f, std::size_t number_of_lru_history_items);
   
           handle operator[](key_type k);
   
       private:
           struct handle_move_t; // until C++11
       };
   } // namespace tbb 
   } // namespace oneapi

The following table provides additional information on the
members of this template class.

= ========================================================================================
\ Member, Description
==========================================================================================
\ ``concurrent_lru_cache(value_function_type f, std::size_t number_of_lru_history_items)``
  \
  Constructs an empty cache that can keep up to
  ``number_of_lru_history_items`` unused values, with
  function object ``f`` for constructing new values.
------------------------------------------------------------------------------------------
\ ``handle operator[](key_type k)``
  \
  Searches the container for an item corresponding to the
  given key. If such item is not found, the user-specified function object is
  called to construct a value which is inserted into the container.
  
  **Returns**:
  ``handle`` object holding reference to the matching value.
------------------------------------------------------------------------------------------
\ ``~concurrent_lru_cache()``
  \
  Destroys all items in the container, and
  the container itself, so that it can no longer be used.
------------------------------------------------------------------------------------------
\ ``class handle``
  \
  A class to hold references and provide access
  to the items stored in concurrent_lru_cache.
  :doc:`More information <concurrent_lru_cache_cls/handle_object_cls>`
------------------------------------------------------------------------------------------
\ ``struct handle_move_t``
  \
  An auxiliary private type to transfer references between
  instances of class ``handle``; may not be used directly.
  Move semantics in C++11 make this class obsolete.
------------------------------------------------------------------------------------------
= ========================================================================================

.. toctree::

   concurrent_lru_cache_cls/handle_object_cls.rst
