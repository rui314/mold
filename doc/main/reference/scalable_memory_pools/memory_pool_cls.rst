.. _memory_pool_cls:

memory_pool
===========

.. note::
   To enable this feature, set the ``TBB_PREVIEW_MEMORY_POOL`` macro to 1.

A class template for scalable memory allocation from memory blocks provided by an underlying allocator.

.. contents::
    :local:
    :depth: 1

Description
***********

A ``memory_pool`` allocates and frees memory in a way that scales with the number of processors.
The memory is obtained as big chunks from an underlying allocator specified by the template
argument. The latter must satisfy the subset of the allocator requirements from the [allocator.requirements]
ISO C++ Standard section. A ``memory_pool`` meet the :doc:`Memory Pool named requirement<../scalable_memory_pools>`.

.. caution::

    If the underlying allocator refers to another scalable memory pool, the inner pool (or pools)
    must be destroyed before the outer pool is destroyed or recycled.

API
***

Header
------

.. code:: cpp

    #include "oneapi/tbb/memory_pool.h"

Synopsis
--------

.. code:: cpp

    namespace oneapi {
        namespace tbb {
            template <typename Alloc>
            class memory_pool {
            public:
                explicit memory_pool(const Alloc &src = Alloc());
                memory_pool(const memory_pool& other) = delete;
                memory_pool& operator=(const memory_pool& other) = delete;
                ~memory_pool();
                void recycle();
                void *malloc(size_t size);
                void free(void* ptr);
               void *realloc(void* ptr, size_t size);
            };
        }
    }

Member Functions
----------------

.. cpp:function:: explicit memory_pool(const Alloc &src = Alloc())

    **Effects**: Constructs a memory pool with an instance of underlying memory allocator of type ``Alloc`` copied from ``src``.
    Throws the ``bad_alloc`` exception if runtime fails to construct an instance of the class.

Examples
********

The code below provides a simple example of allocation from an extensible memory pool.

.. code:: cpp

    #define TBB_PREVIEW_MEMORY_POOL 1
    #include "oneapi/tbb/memory_pool.h"
    ...
    oneapi::tbb::memory_pool<std::allocator<char> > my_pool;
    void* my_ptr = my_pool.malloc(10);
    my_pool.free(my_ptr);
