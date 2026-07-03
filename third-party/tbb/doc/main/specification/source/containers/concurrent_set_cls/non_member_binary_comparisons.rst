.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============================
Non-member binary comparisons
=============================

Two ``oneapi::tbb::concurrent_set`` objects are equal if they have the same number of elements
and each element in one container is equal to the element in other container on the same position.

.. code:: cpp

    template <typename T, typename Compare, typename Allocator>
    bool operator==( const concurrent_set<T, Compare, Allocator>& lhs,
                     const concurrent_set<T, Compare, Allocator>& rhs )

**Returns**: ``true`` if ``lhs`` is equal to ``rhs``; ``false``, otherwise.

-----------------------------------------------------

.. code:: cpp

    template <typename T, typename Compare, typename Allocator>
    bool operator!=( const concurrent_set<T, Compare, Allocator>& lhs,
                     const concurrent_set<T, Compare, Allocator>& rhs )

**Returns**: ``true`` if ``lhs`` is not equal to ``rhs``; ``false``, otherwise.
