.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============
value_compare
=============

``concurrent_map::value_compare`` is a function object that is used to compare
``concurrent_map::value_type`` objects by comparing their first components.

Class Synopsis
--------------

    .. code:: cpp

        namespace oneapi {
            namespace tbb {

                template <typename Key, typename T,
                          typename Compare, typename Allocator>
                class concurrent_map<Key, T, Compare, Allocator>::value_compare {
                protected:
                    key_compare comp;

                    value_compare( key_compare c );

                public:
                    bool operator()( const value_type& lhs, const value_type& rhs ) const;
                }; // class value_compare

            } // namespace tbb
        } // namespace oneapi

Member objects
--------------

    .. code:: cpp

        key_compare comp;

    The key comparison function object.

Member functions
----------------

    .. code:: cpp

        value_compare( key_compare c );

    Constructs a ``value_compare`` with the stored key comparison function object ``c``.

-----------------------------------------------

    .. code:: cpp

        bool operator()( const value_type& lhs, const value_type& rhs ) const;

    Compares ``lhs.first`` and ``rhs.first`` by calling the stored key comparison function ``comp``.

    **Returns**: ``true`` if first components of ``lhs`` and ``rhs`` are equal; ``false``, otherwise.
