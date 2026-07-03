.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

========================
scalable_memory_resource
========================
**[memory_allocation.scalable_memory_resource]**

A ``oneapi::tbb::scalable_memory_resource()`` is a function that returns a memory resource for scalable memory allocation.

The ``scalable_memory_resource()`` function returns the pointer to the memory resource managed by the oneTBB scalable memory allocator.
In particular, its ``allocate`` method uses ``scalable_aligned_malloc()``, and ``deallocate`` uses ``scalable_free()``.
The memory resources returned by this function compare equal.

``std::pmr::polymorphic_allocator`` instantiated with ``oneapi::tbb::scalable_memory_resource()`` behaves like ``oneapi::tbb::scalable_allocator``.

.. code:: cpp

    // Defined in header <oneapi/tbb/scalable_allocator.h>

    std::pmr::memory_resource* scalable_memory_resource();

