.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

================
fixed_pool Class
================


Summary
-------

Template class for scalable memory allocation from
a buffer of fixed size.

Syntax
------

.. code:: cpp

   class fixed_pool;


Header
------

.. code:: cpp

   #define TBB_PREVIEW_MEMORY_POOL 1
    #include "oneapi/tbb/memory_pool.h"


Description
-----------

A 
``fixed_pool`` allocates
and frees memory in a way that scales with the number of processors. All the
memory available for the allocation is initially passed through arguments of
the constructor. A 
``fixed_pool`` models the
Memory Pool concept described in Table 52.

Example
-------

.. code:: cpp

   #define TBB_PREVIEW_MEMORY_POOL 1
    #include "oneapi/tbb/memory_pool.h"
    ...
    char buf[1024*1024];
    oneapi::tbb::fixed_pool my_pool(buf, 1024*1024);
    void* my_ptr = my_pool.malloc(10);
    my_pool.free(my_ptr);}

The code above provides a simple example of
allocation from a fixed pool.

Members
-------

.. code:: cpp

   namespace oneapi {
   namespace tbb {
    class fixed_pool : no_copy {
    public:
    fixed_pool(void *buffer, size_t size) throw(std::bad_alloc);
    ~fixed_pool();
    void recycle();
    void *malloc(size_t size);
    void free(void* ptr);
    void *realloc(void* ptr, size_t size);
    };
   } // namespace tbb
   } // namespace oneapi
    

The following table provides additional information on the member
of this class.

= ========================================================================================
\ Member, Description
==========================================================================================
\ ``fixed_pool(void *buffer, size_t size)``
  \
  Constructs a memory pool to manage the
  memory pointed by buffer and of size.
  
  Throws 
  ``bad_alloc``
  exception if runtime fails to construct an instance of the class.
------------------------------------------------------------------------------------------
= ========================================================================================
