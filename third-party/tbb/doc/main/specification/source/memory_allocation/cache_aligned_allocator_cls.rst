.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=======================
cache_aligned_allocator
=======================
**[memory_allocation.cache_aligned_allocator]**

A ``cache_aligned_allocator`` is a class template that models the allocator requirements from the [allocator.requirements] ISO C++ section.

The ``cache_aligned_allocator`` allocates memory on cache line boundaries, in order to avoid false sharing and potentially improve performance.
False sharing is a situation when logically distinct items occupy the same cache line,
which can hurt performance if multiple threads attempt to access the different items simultaneously.
Even though the items are logically separate, the processor hardware may have to transfer the cache line between the processors
as if they were sharing a location. The net result can be much more memory traffic than if the logically distinct items were on different cache lines.

However, this class is sometimes an inappropriate replacement for default allocator, because the benefit of allocating on a cache line comes at the price
that ``cache_aligned_allocator`` implicitly adds pad memory. Therefore allocating many small objects with ``cache_aligned_allocator`` may increase memory usage.

.. code:: cpp

    // Defined in header <oneapi/tbb/cache_aligned_allocator.h>

    namespace oneapi {
    namespace tbb {
        template<typename T> class cache_aligned_allocator {
        public:
            using value_type = T;
            using size_type = std::size_t;
            using propagate_on_container_move_assignment = std::true_type;
            using is_always_equal = std::true_type;

            cache_aligned_allocator() = default;
            template<typename U>
            cache_aligned_allocator(const cache_aligned_allocator<U>&) noexcept;

            T* allocate(size_type);
            void deallocate(T*, size_type);
            size_type max_size() const noexcept;
        };
    } // namespace tbb
    } // namespace oneapi

Member Functions
----------------

.. cpp:function:: T* allocate(size_type n)

    Returns a pointer to the allocated ``n * sizeof(T)`` bytes of memory, aligned on a cache-line boundary.
    The allocation may include extra hidden padding.

.. cpp:function:: void deallocate(T* p, size_type n)

    Deallocates memory pointed to by ``p``. Deallocation also deallocates any extra hidden padding.
    The behavior is undefined if the pointer ``p`` is not the result of the ``allocate(n)`` method.
    The behavior is undefined if the memory has been already deallocated.

.. cpp:function:: size_type max_size() const noexcept

    Returns the largest value ``n`` for which the call ``allocate(n)`` might succeed with cache alignment constraints.

Non-member Functions
--------------------

These functions provide comparison operations between two ``cache_aligned_allocator`` instances.

.. code:: cpp

    template<typename T, typename U>
    bool operator==(const cache_aligned_allocator<T>&,
                    const cache_aligned_allocator<U>&) noexcept;

    template<typename T, typename U>
    bool operator!=(const cache_aligned_allocator<T>&,
                    const cache_aligned_allocator<U>&) noexcept;

The namespace where these functions are defined is unspecified, as long as they may be used in respective binary operation expressions on ``cache_aligned_allocator`` objects.
For example, an implementation may define the classes and functions in the same unspecified internal namespace,
and define ``oneapi::tbb::cache_aligned_allocator`` as a type alias for which the non-member functions are reachable only via argument-dependent lookup.

.. cpp:function:: template<typename T, typename U> \
    bool operator==(const cache_aligned_allocator<T>&, const cache_aligned_allocator<U>&) noexcept

    Returns **true**.

.. cpp:function:: template<typename T, typename U> \
    bool operator!=(const cache_aligned_allocator<T>&, const cache_aligned_allocator<U>&) noexcept

    Returns **false**.

