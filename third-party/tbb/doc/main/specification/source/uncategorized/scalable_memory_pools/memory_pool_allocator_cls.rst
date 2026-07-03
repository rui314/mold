.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

====================================
memory_pool_allocator Template Class
====================================


Summary
-------

Template class that provides the C++ allocator
interface for memory pools.

Syntax
------

.. code:: cpp

   template<typename T> class memory_pool_allocator;


Header
------

.. code:: cpp

   #define TBB_PREVIEW_MEMORY_POOL 1
   #include "oneapi/tbb/memory_pool.h"


Description
-----------

A 
``memory_pool_allocator``
models the allocator requirements described in
:doc:`Memory Pool Concept table <../scalable_memory_pools>` except for default
constructor which is excluded from the class. Instead, it provides a
constructor, which links with an instance of 
``memory_pool`` or 
``fixed_pool`` classes,
that actually allocates and deallocates memory. The class is mainly intended to
enable memory pools within STL containers.

Example
-------

.. code:: cpp

   #define TBB_PREVIEW_MEMORY_POOL 1
   #include "oneapi/tbb/memory_pool.h"
   ...
   typedef oneapi::tbb::memory_pool_allocator<int>
   pool_allocator_t;
   std::list<int, pool_allocator_t>
   my_list(pool_allocator_t( my_pool ));

The code above provides a simple example of
construction of a container that uses a memory pool.

Members
-------

.. code:: cpp

   namespace oneapi {
   namespace tbb {
   template<typename T>
   class memory_pool_allocator {
   public:
       typedef T value_type;
       typedef value_type* pointer;
       typedef const value_type* const_pointer;
       typedef value_type& reference;
       typedef const value_type& const_reference;
       typedef size_t size_type;
       typedef ptrdiff_t difference_type;
       template<typename U> struct rebind {
           typedef memory_pool_allocator<U> other;
       };
       explicit memory_pool_allocator(memory_pool &pool) throw();
       explicit memory_pool_allocator(fixed_pool &pool) throw();
       memory_pool_allocator(const memory_pool_allocator& src) throw();
       template<typename U>
       memory_pool_allocator(const memory_pool_allocator<U,P>& src) throw();
       pointer address(reference x) const;
       const_pointer address(const_reference x) const;
       pointer allocate( size_type n, const void* hint=0);
       void deallocate( pointer p, size_type );
       size_type max_size() const throw();
       void construct( pointer p, const T& value );
       void destroy( pointer p );
   };
   template<>
   class memory_pool_allocator<void> {
   public:
       typedef void* pointer;
       typedef const void* const_pointer;
       typedef void value_type;
       template<typename U> struct rebind {
           typedef memory_pool_allocator<U> other;
       };
       memory_pool_allocator(memory_pool &pool) throw();
       memory_pool_allocator(fixed_pool &pool) throw();
       memory_pool_allocator(const memory_pool_allocator& src) throw();
       template<typename U>
       memory_pool_allocator(const memory_pool_allocator<U>& src) throw();
   };
   template<typename T, typename U>
   inline bool operator==( const memory_pool_allocator<T>& a,
                const memory_pool_allocator<U>& b);
   template<typename T, typename U>
   inline bool operator!=( const memory_pool_allocator<T>& a,
                const memory_pool_allocator<U>& b);
   } // namespace tbb
   } // namespace oneapi

The following table provides additional information on the
members of this template class.

= ========================================================================================
\ Member, Description
==========================================================================================
\ ``explicit memory_pool_allocator(memory_pool &pool)``
  \
  Constructs a memory pool allocator serviced
  by the 
  ``memory_pool``
  instance pool.
------------------------------------------------------------------------------------------
\ ``explicit memory_pool_allocator(fixed_pool &pool)``
  \
  Constructs a memory pool allocator serviced
  by the 
  ``fixed_pool``
  instance pool.
------------------------------------------------------------------------------------------
= ========================================================================================
