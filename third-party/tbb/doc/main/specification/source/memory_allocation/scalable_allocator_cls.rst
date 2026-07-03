.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==================
scalable_allocator
==================
**[memory_allocation.scalable_allocator]**

A ``scalable_allocator`` is a class template that models the allocator requirements from the [allocator.requirements] ISO C++ section.

The ``scalable_allocator`` allocates and frees memory in a way that scales with the number of processors.
Memory allocated by a ``scalable_allocator`` should be freed by a ``scalable_allocator``, not by a ``std::allocator``.

.. code:: cpp

    // Defined in header <oneapi/tbb/scalable_allocator.h>

    namespace oneapi {
    namespace tbb {
        template<typename T> class scalable_allocator {
        public:
            using value_type = T;
            using size_type = std::size_t;
            using propagate_on_container_move_assignment = std::true_type;
            using is_always_equal = std::true_type;

            scalable_allocator() = default;
            template<typename U>
            scalable_allocator(const scalable_allocator<U>&) noexcept;

            T* allocate(size_type);
            void deallocate(T*, size_type);
        };
    } // namespace tbb
    } // namespace oneapi

.. caution::

   The ``scalable_allocator`` requires the memory allocator library. If the library is missing, calls to the scalable allocator fail. In
   contrast to ``scalable_allocator``, if the memory allocator library is not available, ``tbb_allocator`` falls back on ``std::malloc`` and ``std::free``.

Member Functions
----------------

.. namespace:: oneapi::tbb::scalable_allocator

.. cpp:function:: value_type* allocate(size_type n)

    Allocates ``n * sizeof(T)`` bytes of memory. Returns a pointer to the allocated memory.

.. cpp:function:: void deallocate(value_type* p, size_type n)

    Deallocates memory pointed to by ``p``.
    The behavior is undefined if the pointer ``p`` is not the result of the ``allocate(n)`` method.
    The behavior is undefined if the memory has been already deallocated.

Non-member Functions
--------------------

These functions provide comparison operations between two ``scalable_allocator`` instances.

.. code:: cpp

    namespace oneapi {
    namespace tbb {
        template<typename T, typename U>
        bool operator==(const scalable_allocator<T>&,
                        const scalable_allocator<U>&) noexcept;

        template<typename T, typename U>
        bool operator!=(const scalable_allocator<T>&,
                        const scalable_allocator<U>&) noexcept;
    } // namespace tbb
    } // namespace oneapi

The namespace where these functions are defined is unspecified, as long as they may be used in respective binary operation expressions on ``scalable_allocator`` objects.
For example, an implementation may define the classes and functions in the same unspecified internal namespace,
and define ``oneapi::tbb::scalable_allocator`` as a type alias for which the non-member functions are reachable only via argument-dependent lookup.

.. cpp:function:: template<typename T, typename U> \
    bool operator==(const scalable_allocator<T>&, const scalable_allocator<U>&) noexcept

    Returns **true**.

.. cpp:function:: template<typename T, typename U> \
    bool operator!=(const scalable_allocator<T>&, const scalable_allocator<U>&) noexcept

    Returns **false**.

