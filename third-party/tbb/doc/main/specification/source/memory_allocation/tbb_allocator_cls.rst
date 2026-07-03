.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============
tbb_allocator
=============
**[memory_allocation.tbb_allocator]**

A ``tbb_allocator`` is a class template that models the allocator requirements from the [allocator.requirements] ISO C++ section.

The ``tbb_allocator`` allocates and frees memory via the oneTBB malloc library if it is available,
otherwise, it reverts to using ``std::malloc`` and ``std::free``.

.. code:: cpp

    // Defined in header <oneapi/tbb/tbb_allocator.h>

    namespace oneapi {
    namespace tbb {
        template<typename T> class tbb_allocator {
        public:
            using value_type = T;
            using size_type = std::size_t;
            using propagate_on_container_move_assignment = std::true_type;
            using is_always_equal = std::true_type;

            enum malloc_type {
                scalable,
                standard
            };

            tbb_allocator() = default;
            template<typename U>
            tbb_allocator(const tbb_allocator<U>&) noexcept;

            T* allocate(size_type);
            void deallocate(T*, size_type);

            static malloc_type allocator_type();
        };
    } // namespace tbb
    } // namespace oneapi

Member Functions
----------------

.. namespace:: oneapi::tbb::tbb_allocator
	       
.. cpp:function:: T* allocate(size_type n)

    Allocates ``n * sizeof(T)`` bytes. Returns a pointer to the allocated memory.

.. cpp:function:: void deallocate(T* p, size_type n)

    Deallocates memory pointed to by ``p``.
    The behavior is undefined if the pointer ``p`` is not the result of the ``allocate(n)`` method.
    The behavior is undefined if the memory has been already deallocated.

.. cpp:function:: static malloc_type allocator_type()

    Returns the enumeration type ``malloc_type::scalable`` if the oneTBB malloc library is available, and ``malloc_type::standard``, otherwise.

Non-member Functions
--------------------

These functions provide comparison operations between two ``tbb_allocator`` instances.

.. code:: cpp

    template<typename T, typename U>
    bool operator==(const tbb_allocator<T>&, const tbb_allocator<U>&) noexcept;

    template<typename T, typename U>
    bool operator!=(const tbb_allocator<T>&, const tbb_allocator<U>&) noexcept;

The namespace where these functions are defined is unspecified, as long as they may be used in respective binary operation expressions on ``tbb_allocator`` objects.
For example, an implementation may define the classes and functions in the same unspecified internal namespace
and define ``oneapi::tbb::tbb_allocator`` as a type alias for which the non-member functions are reachable only via argument-dependent lookup.

.. cpp:function:: template<typename T, typename U> \
    bool operator==(const tbb_allocator<T>&, const tbb_allocator<U>&) noexcept

    Returns **true**.

.. cpp:function:: template<typename T, typename U> \
    bool operator!=(const tbb_allocator<T>&, const tbb_allocator<U>&) noexcept

    Returns **false**.

