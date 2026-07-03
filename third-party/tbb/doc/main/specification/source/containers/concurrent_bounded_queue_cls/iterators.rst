.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=========
Iterators
=========

The types ``concurrent_bounded_queue::iterator`` and ``concurrent_bounded_queue::const_iterator``
meet the requirements of ``ForwardIterator`` from the [forward.iterators] ISO C++ Standard section.

All member functions in this section can only be performed serially. The behavior is undefined in
case of concurrent execution of these methods with other (either concurrently safe) methods.

unsafe_begin and unsafe_cbegin
------------------------------

    .. code:: cpp

        iterator unsafe_begin();

        const_iterator unsafe_begin() const;

        const_iterator unsafe_cbegin() const;

    **Returns**: an iterator to the first element in the container.

unsafe_end and unsafe_cend
--------------------------

    .. code:: cpp

        iterator unsafe_end();

        const_iterator unsafe_end() const;

        const_iterator unsafe_cend() const;

    **Returns**: an iterator to the element that follows the last element in the container.
