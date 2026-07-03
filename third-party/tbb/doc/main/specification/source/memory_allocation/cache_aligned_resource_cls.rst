.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

======================
cache_aligned_resource
======================
**[memory_allocation.cache_aligned_resource]**

A ``cache_aligned_resource`` is a general-purpose memory resource class, which acts as a wrapper to another memory resource
to ensure that all allocations are aligned on cache line boundaries to avoid false sharing.

See the :doc:`cache_aligned_allocator template class <cache_aligned_allocator_cls>` section for more information about false sharing avoidance.

.. code:: cpp

    // Defined in header <oneapi/tbb/cache_aligned_allocator.h>

    namespace oneapi {    
    namespace tbb {
        class cache_aligned_resource {
        public:
            cache_aligned_resource();
            explicit cache_aligned_resource( std::pmr::memory_resource* );

            std::pmr::memory_resource* upstream_resource() const;

        private:
            void* do_allocate(size_t n, size_t alignment) override;
            void do_deallocate(void* p, size_t n, size_t alignment) override;
            bool do_is_equal(const std::pmr::memory_resource& other) const noexcept override;
        };
    } // namespace tbb
    } // namespace oneapi

Member Functions
----------------

.. cpp:function:: cache_aligned_resource()

    Constructs a ``cache_aligned_resource`` over ``std::pmr::get_default_resource()``.

.. cpp:function:: explicit cache_aligned_resource(std::pmr::memory_resource* r)

    Constructs a ``cache_aligned_resource`` over the memory resource ``r``.

.. cpp:function:: std::pmr::memory_resource* upstream_resource() const

    Returns the pointer to the underlying memory resource.

.. cpp:function:: void* do_allocate(size_t n, size_t alignment) override

    Allocates ``n`` bytes of memory on a cache-line boundary, with alignment not less than requested.
    The allocation may include extra memory for padding. Returns pointer to the allocated memory.

.. cpp:function:: void do_deallocate(void* p, size_t n, size_t alignment) override

    Deallocates memory pointed to by p and any extra padding. Pointer ``p`` must be obtained with ``do_allocate(n, alignment)``.
    The memory must not be deallocated beforehand.

.. cpp:function:: bool do_is_equal(const std::pmr::memory_resource& other) const noexcept override

    Compares upstream memory resources of ``*this`` and ``other``. If ``other`` is not a ``cache_aligned_resource``, returns false.


