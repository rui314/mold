.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=========
Iterators
=========

The types ``concurrent_unordered_multiset::iterator`` and ``concurrent_unordered_multiset::const_iterator``
meet the requirements of ``ForwardIterator`` from the [forward.iterators] ISO C++ Standard section.

begin and cbegin
----------------

    .. code:: cpp

        iterator begin();

        const_iterator begin() const;

        const_iterator cbegin() const;

    **Returns**: an iterator to the first element in the container.

end and cend
------------

    .. code:: cpp

        iterator end();

        const_iterator end() const;

        const_iterator cend() const;

    **Returns**: an iterator to the element that follows the last element in the container.
