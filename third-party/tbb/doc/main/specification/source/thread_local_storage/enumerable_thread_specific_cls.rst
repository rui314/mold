.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==========================
enumerable_thread_specific
==========================
**[tls.enumerable_thread_specific]**

A class template for thread local storage (TLS).

.. code:: cpp

    // Defined in header <oneapi/tbb/enumerable_thread_specific.h>

    namespace oneapi {
    namespace tbb {

        enum ets_key_usage_type {
            ets_key_per_instance,
            ets_no_key,
            ets_suspend_aware
        };

        template <typename T,
            typename Allocator=cache_aligned_allocator<T>,
            ets_key_usage_type ETS_key_type=ets_no_key >
        class enumerable_thread_specific {
        public:
            // Basic types
            using value_type = T;
            using reference = T&;
            using const_reference = const T&;
            using pointer = T*;
            using size_type = /* implementation-defined */;
            using difference_type = /* implementation-defined */;
            using allocator_type = Allocator;

            // Iterator types
            using iterator = /* implementation-defined */;
            using const_iterator = /* implementation-defined */;

            // Parallel range types
            using range_type = /* implementation-defined */;
            using const_range_type = /* implementation-defined */;

            // Construction
            enumerable_thread_specific();
            template <typename Finit>
            explicit enumerable_thread_specific( Finit finit );
            explicit enumerable_thread_specific( const T& exemplar );
            explicit enumerable_thread_specific( T&& exemplar );
            template <typename... Args>
            enumerable_thread_specific( Args&&... args );

            // Destruction
            ~enumerable_thread_specific();

            // Copy constructors
            enumerable_thread_specific( const enumerable_thread_specific& other);
            template<typename Alloc, ets_key_usage_type Cachetype>
            enumerable_thread_specific( const enumerable_thread_specific<T, Alloc, Cachetype>& other);
            // Copy assignments
            enumerable_thread_specific& operator=( const enumerable_thread_specific& other );
            template<typename Alloc, ets_key_usage_type Cachetype>
            enumerable_thread_specific& operator=( const enumerable_thread_specific<T, Alloc, Cachetype>& other );

            // Move constructors
            enumerable_thread_specific( enumerable_thread_specific&& other);
            template<typename Alloc, ets_key_usage_type Cachetype>
            enumerable_thread_specific( enumerable_thread_specific<T, Alloc, Cachetype>&& other);
            // Move assignments
            enumerable_thread_specific& operator=( enumerable_thread_specific&& other );
            template<typename Alloc, ets_key_usage_type Cachetype>
            enumerable_thread_specific& operator=( enumerable_thread_specific<T, Alloc, Cachetype>&& other );

            // Other whole container operations
            void clear();

            // Concurrent operations
            reference local();
            reference local( bool& exists );
            size_type size() const;
            bool empty() const;

            // Combining
            template<typename BinaryFunc> T combine( BinaryFunc f );
            template<typename UnaryFunc> void combine_each( UnaryFunc f );

            // Parallel iteration
            range_type range( size_t grainsize=1 );
            const_range_type range( size_t grainsize=1 ) const;

            // Iterators
            iterator begin();
            iterator end();
            const_iterator begin() const;
            const_iterator end() const;
        };

    } // namespace tbb
    } // namespace oneapi

A class template ``enumerable_thread_specific`` provides TLS for elements of type ``T``.
A class template ``enumerable_thread_specific`` acts as a container by providing iterators and ranges across all of the thread-local elements.

The thread-local elements are created lazily. A freshly constructed ``enumerable_thread_specific`` has no elements.
When a thread requests access to an ``enumerable_thread_specific``, it creates an element corresponding to that thread.
The number of elements is equal to the number of distinct threads that have accessed the ``enumerable_thread_specific``
and not necessarily the number of threads in use by the application. Clearing an ``enumerable_thread_specific`` removes all its elements.

Use the ``ETS_key_usage_type`` parameter type to select an underlying implementation.

.. caution::

    ``enumerable_thread_specific`` uses the OS-specific value returned by ``std::this_thread::get_id()`` to identify threads.
    This value is not guaranteed to be unique except for the life of the thread.
    A newly created thread may get an OS-specific ID equal to that of an already destroyed thread.
    The number of elements of the ``enumerable_thread_specific`` may therefore be less than the number of actual distinct threads
    that have called ``local()``, and the element returned by the first reference
    by a thread to the ``enumerable_thread_specific`` may not be newly-constructed.

Member functions
----------------

.. toctree::
    :titlesonly:

    enumerable_thread_specific_cls/construct_destroy_copy.rst
    enumerable_thread_specific_cls/safe_modifiers.rst
    enumerable_thread_specific_cls/unsafe_modifiers.rst
    enumerable_thread_specific_cls/size_and_capacity.rst
    enumerable_thread_specific_cls/iteration.rst
    enumerable_thread_specific_cls/combining.rst

Non-member types and constants
------------------------------

.. cpp:enum:: ets_key_usage_type::ets_key_per_instance

Enumeration parameter type used to select an implementation that consumes 1 native TLS key per ``enumerable_thread_specific`` instance.
The number of native TLS keys may be limited and can be fairly small.

.. cpp:enum:: ets_key_usage_type::ets_no_key

Enumeration parameter type used to select an implementation that consumes no native TLS keys.
If no ``ETS_key_usage_type`` parameter type is provided, ``ets_no_key`` is used by default.

.. cpp:enum:: ets_key_usage_type::ets_suspend_aware

The ``oneapi::tbb::task::suspend`` function can change the value of the ``enumerable_thread_specific`` object. To avoid this problem,
use the ``ets_suspend_aware`` enumeration parameter type.
The ``local()`` value can be the same for different threads, but no two distinct threads can access the same value simultaneously.

