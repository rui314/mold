.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==========================
memory_pool Template Class
==========================


Summary
-------

Template class for scalable memory allocation from
memory blocks provided by an underlying allocator.

.. caution::

   If the underlying allocator refers to another
   scalable memory pool, the inner pool (or pools) must be destroyed before the
   outer pool is destroyed or recycled.


Syntax
------

.. code:: cpp

   template <typename Alloc> class memory_pool;


Header
------

.. code:: cpp

   #define TBB_PREVIEW_MEMORY_POOL 1
   #include "oneapi/tbb/memory_pool.h"


Description
-----------

A 
``memory_pool`` allocates
and frees memory in a way that scales with the number of processors. The memory
is obtained as big chunks from an underlying allocator specified by the
template argument. The latter must satisfy the subset of requirements described
in Table 29 with 
``allocate``, 
``deallocate``, and 
``value_type`` valid for 
``sizeof(value_type)>0``. A 
``memory_pool`` models the
Memory Pool concept described in Table 52.

Example
-------

.. code:: cpp

   #define TBB_PREVIEW_MEMORY_POOL 1
   #include "oneapi/tbb/memory_pool.h"
   ...
   oneapi::tbb::memory_pool<std::allocator<char> > my_pool;
   void* my_ptr = my_pool.malloc(10);
   my_pool.free(my_ptr);

The code above provides a simple example of
allocation from an extensible memory pool.

Members
-------

.. code:: cpp

   namespace oneapi {
   namespace tbb {
   template <typename Alloc>
   class memory_pool : no_copy {
   public:
       explicit memory_pool(const Alloc &src = Alloc()) throw(std::bad_alloc);
       ~memory_pool();
       void recycle();
       void *malloc(size_t size);
       void free(void* ptr);
       void *realloc(void* ptr, size_t size);
   };
   } // namespace tbb
   } // namespace oneapi

The following table provides additional information on the member
of this template class.

= ========================================================================================
\ Member, Description
==========================================================================================
\ ``explicit memory_pool(const Alloc &src = Alloc())``
  \
  Constructs a memory pool with an instance
  of underlying memory allocator of type 
  ``Alloc`` copied
  from 
  ``src``. Throws the
  ``bad_alloc``
  exception if runtime fails to construct an instance of the class.
------------------------------------------------------------------------------------------
= ========================================================================================
