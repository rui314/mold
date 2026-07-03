.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===========================
accessor and const_accessor
===========================

Member classes ``concurrent_hash_map::accessor`` and ``concurrent_hash_map::const_accessor`` are called `accessors`.
Accessors allow multiple threads to concurrently access the key-value pairs in ``concurrent_hash_map``.
An accessor is called `empty` if it does not point to any item.

``accessor`` member class
-------------------------

Member class ``concurrent_hash_map::accessor`` provides read-write access to the key-value pair in ``concurrent_hash_map``.

.. code:: cpp

    namespace oneapi {
        namespace tbb {

            template <typename Key, typename T, typename HashCompare, typename Allocator>
            class concurrent_hash_map<Key, T, HashCompare, Allocator>::accessor {
                using value_type = std::pair<const Key, T>;

                accessor();
                ~accessor();

                bool empty() const;
                value_type& operator*() const;
                value_type* operator->() const;

                void release();
            }; // class accessor

        } // namespace tbb
    } // namespace oneapi

``const_accessor`` member class
-------------------------------

Member class ``concurrent_hash_map::const_accessor`` provides read only access to the key-value pair in ``concurrent_hash_map``.

.. code:: cpp

    namespace oneapi {
        namespace tbb {

            template <typename Key, typename T, typename HashCompare, typename Allocator>
            class concurrent_hash_map<Key, T, HashCompare, Allocator>::const_accessor {
                using value_type = const std::pair<const Key, T>;

                const_accessor();
                ~const_accessor();

                bool empty() const;
                value_type& operator*() const;
                value_type* operator->() const;

                void release();
            }; // class const_accessor

        } // namespace tbb
    } // namespace oneapi

Member functions
----------------

Construction and destruction
~~~~~~~~~~~~~~~~~~~~~~~~~~~~

    .. code:: cpp

        accessor();

        const_accessor();

    Constructs an empty accessor.

--------------------------

    .. code:: cpp

        ~accessor();

        ~const_accessor();

    Destroys the accessor. If ``*this`` is not empty, releases the ownership of the element.

Emptiness
~~~~~~~~~

    .. code:: cpp

        bool empty() const;

    **Returns**: ``true`` if the accessor is empty; ``false``, otherwise.

Key-value pair access
~~~~~~~~~~~~~~~~~~~~~

    .. code:: cpp

        value_type& operator*() const;

    **Returns**: a reference to the key-value pair to which the accessor points.

    The behavior is undefined if the accessor is empty.

--------------------------

    .. code:: cpp

        value_type* operator->() const;

    **Returns**: a pointer to the key-value pair to which the accessor points.

    The behavior is undefined if the accessor is empty.

Releasing
~~~~~~~~~

    .. code:: cpp

        void release();

    If ``*this`` is not empty, releases the ownership of the element. ``*this`` becomes empty.
