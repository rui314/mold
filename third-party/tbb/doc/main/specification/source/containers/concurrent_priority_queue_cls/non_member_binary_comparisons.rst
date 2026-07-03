.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============================
Non-member binary comparisons
=============================

.. code:: cpp
  
    template <typename T, typename Compare, typename Allocator>
    bool operator==( const concurrent_priority_queue<T, Compare, Allocator>& lhs,
                     const concurrent_priority_queue<T, Compare, Allocator>& rhs );
          
Checks if ``lhs`` is equal to ``rhs``, that is they have the same number of elements and ``lhs``
contains all elements from ``rhs`` with the same priority.

**Returns**: ``true`` if ``lhs`` is equal to ``rhs``; ``false``, otherwise.

.. code:: cpp

    template <typename T, typename Compare, typename Allocator>
    bool operator!=( const concurrent_priority_queue<T, Compare, Allocator>& lhs,
                     const concurrent_priority_queue<T, Compare, Allocator>& rhs );

Equivalent to ``!(lhs == rhs)``.

**Returns**: ``true`` if ``lhs`` is not equal to ``rhs``; ``false``, otherwise.
