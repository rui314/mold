.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===========
flattened2d
===========
**[tls.flattened2d]**

The class template ``flattened2d`` is an adaptor that provides a flattened view of a container of containers.

.. code:: cpp

    // Defined in header <oneapi/tbb/enumerable_thread_specific.h>

    namespace oneapi {
    namespace tbb {

        template<typename Container>
        class flattened2d {
        public:
            // Basic types
            using size_type = /* implementation-defined */;
            using difference_type = /* implementation-defined */;
            using allocator_type = /* implementation-defined */;
            using value_type = /* implementation-defined */;
            using reference = /* implementation-defined */;
            using const_reference = /* implementation-defined */;
            using pointer = /* implementation-defined */;
            using const_pointer = /* implementation-defined */;

            using iterator = /* implementation-defined */;
            using const_iterator = /* implementation-defined */;

            explicit flattened2d( const Container& c );

            flattened2d( const Container& c,
                         typename Container::const_iterator first,
                         typename Container::const_iterator last );

            iterator begin();
            iterator end();
            const_iterator begin() const;
            const_iterator end() const;

            size_type size() const;
        };

        template <typename Container>
        flattened2d<Container> flatten2d(const Container &c);

        template <typename Container>
        flattened2d<Container> flatten2d(
            const Container &c,
            const typename Container::const_iterator first,
            const typename Container::const_iterator last);

    } // namespace tbb
    } // namespace oneapi

Requirements:

* A ``Container`` type must meet the container requirements from the [container.requirements.general] ISO C++ section.

Iterating from ``begin()`` to ``end()`` visits all of the elements in the inner containers.
The class template supports forward iterators only.

The utility function ``flatten2d`` creates a ``flattened2d`` object from a specified container.

Member functions
----------------

.. namespace:: oneapi::tbb::flatten2d
	       
.. cpp:function:: explicit flattened2d( const Container& c )

    Constructs a ``flattened2d`` representing the sequence of
    elements in the inner containers contained by outer container c.

    **Safety**: these operations must not be invoked concurrently on the same ``flattened2d``.

.. cpp:function:: flattened2d( const Container& c, typename Container::const_iterator first, typename Container::const_iterator last )

    Constructs a ``flattened2d`` representing the sequence of elements
    in the inner containers in the half-open interval ``[first, last)`` of a container ``c``.

    **Safety**: these operations must not be invoked concurrently on the same ``flattened2d``.

.. cpp:function:: size_type size() const

    Returns the sum of the sizes of the inner containers that are viewable in the ``flattened2d``.

    **Safety**: These operations may be invoked concurrently on the same ``flattened2d``.

.. cpp:function:: iterator begin()

    Returns ``iterator`` pointing to the beginning of the set of local copies.

.. cpp:function:: iterator end()

    Returns ``iterator`` pointing to the end of the set of local copies.

.. cpp:function:: const_iterator begin() const

    Returns ``const_iterator`` pointing to the beginning of the set of local copies.

.. cpp:function:: const_iterator end() const

    Returns ``const_iterator`` pointing to the end of the set of local copies.

Non-member functions
--------------------

.. cpp:function:: template <typename Container>  flattened2d<Container> \
        flatten2d(const Container &c, const typename Container::const_iterator b, const typename Container::const_iterator e)

    Constructs and returns a ``flattened2d`` object that provides iterators that traverse the elements
    in the containers within the half-open range ``[b, e)`` of a container ``c``.

.. cpp:function:: template <typename Container> flattened2d( const Container &c )

    Constructs and returns a ``flattened2d`` that provides iterators that
    traverse the elements in all of the containers within a container ``c``.

