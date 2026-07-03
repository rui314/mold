.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============================
Non-member binary comparisons
=============================

Two objects of ``concurrent_vector`` are equal if:

* they contains an equal number of elements.
* the elements on the same positions are equal.

.. code:: cpp

    template <typename T, typename Allocator>
    bool operator==( const concurrent_vector<T, Allocator>& lhs,
                     const concurrent_vector<T, Allocator>& rhs );

**Returns**: ``true`` if ``lhs`` is equal to ``rhs``, ``false`` otherwise.

---------------------------------------------

.. code:: cpp

    template <typename T, typename Allocator>
    bool operator!=( const concurrent_vector<T, Allocator>& lhs,
                     const concurrent_vector<T, Allocator>& rhs );

**Returns**: ``true`` if ``lhs`` is not equal to ``rhs``, ``false`` otherwise.
