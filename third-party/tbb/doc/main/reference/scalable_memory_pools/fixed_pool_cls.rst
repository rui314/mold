.. _fixed_pool_cls:

fixed_pool
==========

.. note::
   To enable this feature, set the ``TBB_PREVIEW_MEMORY_POOL`` macro to 1.

A class for scalable memory allocation from a buffer of fixed size.

.. contents::
    :local:
    :depth: 1

Description
***********

``fixed_pool`` allocates and frees memory in a way that scales with the number of processors.
All the memory available for the allocation is initially passed through arguments of the constructor.
``fixed_pool`` meet the :doc:`Memory Pool named requirement<../scalable_memory_pools>`.

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
            class fixed_pool {
            public:
                fixed_pool(void *buffer, size_t size);
                fixed_pool(const fixed_pool& other) = delete;
                fixed_pool& operator=(const fixed_pool& other) = delete;
                ~fixed_pool();

                void recycle();
                void* malloc(size_t size);
                void free(void* ptr);
                void* realloc(void* ptr, size_t size);
            };
        } // namespace tbb
    } // namespace oneapi

Member Functions
----------------

.. cpp:function:: fixed_pool(void *buffer, size_t size)

    **Effects**: Constructs a memory pool to manage the memory of size ``size`` pointed to by ``buffer``.
    Throws the ``bad_alloc`` exception if the library fails to construct an instance of the class.

Examples
********

The code below provides a simple example of allocation from a fixed pool.

.. literalinclude:: ../examples/fixed_pool_example.cpp
    :language: c++
    :start-after: /*begin_fixed_pool_example*/
    :end-before: /*end_fixed_pool_example*/
