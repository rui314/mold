.. _memory_pool_allocator_cls:

memory_pool_allocator
=====================

.. note::
   To enable this feature, set the ``TBB_PREVIEW_MEMORY_POOL`` macro to 1.

A class template that provides a memory pool with a C++ allocator interface.

.. contents::
    :local:
    :depth: 1

Description
***********

``memory_pool_allocator`` meets the allocator requirements from the [allocator.requirements] ISO C++ Standard section
It also provides a constructor to allocate and deallocate memory.
This constructor is linked with an instance of either the ``memory_pool`` or the ``fixed_pool`` class.
The class is mainly intended for enabling memory pools within STL containers.

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
            template<typename T>
            class memory_pool_allocator {
            public:
                using value_type = T;
                using pointer = value_type*;
                using const_pointer = const value_type*;
                using reference = value_type&;
                using const_reference = const value_type&;
                using size_type = size_t;
                using difference_type = ptrdiff_t;
                template<typename U> struct rebind {
                    using other = memory_pool_allocator<U>;
                };
                explicit memory_pool_allocator(memory_pool &pool) throw();
                explicit memory_pool_allocator(fixed_pool &pool) throw();
                memory_pool_allocator(const memory_pool_allocator& src) throw();
                template<typename U>
                memory_pool_allocator(const memory_pool_allocator<U,P>& src) throw();
                pointer address(reference x) const;
                const_pointer address(const_reference x) const;
                pointer allocate(size_type n, const void* hint=0);
                void deallocate(pointer p, size_type);
                size_type max_size() const throw();
                void construct(pointer p, const T& value);
                void destroy(pointer p);
            };

            template<>
            class memory_pool_allocator<void> {
            public:
                using pointer = void*;
                using const_pointer = const void*;
                using value_type = void;
                template<typename U> struct rebind {
                    using other = memory_pool_allocator<U>;
                };
                memory_pool_allocator(memory_pool &pool) throw();
                memory_pool_allocator(fixed_pool &pool) throw();
                memory_pool_allocator(const memory_pool_allocator& src) throw();
                template<typename U>
                memory_pool_allocator(const memory_pool_allocator<U>& src) throw();
            };
        } // namespace tbb
    } // namespace oneapi

    template<typename T, typename U>
    inline bool operator==( const memory_pool_allocator<T>& a,
                           const memory_pool_allocator<U>& b);
    template<typename T, typename U>
    inline bool operator!=( const memory_pool_allocator<T>& a,
                            const memory_pool_allocator<U>& b);

Member Functions
----------------

.. cpp:function:: explicit memory_pool_allocator(memory_pool &pool)

    **Effects**: Constructs a memory pool allocator serviced by ``memory_pool`` instance pool.

-------------------------------------------------------

.. cpp:function:: explicit memory_pool_allocator(fixed_pool &pool)

    **Effects**: Constructs a memory pool allocator serviced by ``fixed_pool`` instance pool.

Examples
********

The code below provides a simple example of container construction with the use of a memory pool.

.. literalinclude:: ../examples/memory_pool_allocator_example.cpp
    :language: c++
    :start-after: /*begin_memory_pool_allocator_example*/
    :end-before: /*end_memory_pool_allocator_example*/
