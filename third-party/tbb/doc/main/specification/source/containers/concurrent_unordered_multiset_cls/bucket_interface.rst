.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

================
Bucket interface
================

The types ``concurrent_unordered_multiset::local_iterator``
and ``concurrent_unordered_multiset::const_local_iterator`` meet the requirements
of ``ForwardIterator`` from the [forward.iterators] ISO C++ Standard section.

These iterators are used to traverse the certain bucket.

All methods in this section can only be executed serially. The behavior is undefined in case of
concurrent execution of these member functions with other (either concurrently safe) methods.

Bucket begin and bucket end
---------------------------

    .. code:: cpp

        local_iterator unsafe_begin( size_type n );

        const_local_iterator unsafe_begin( size_type n ) const;

        const_local_iterator unsafe_cbegin( size_type n ) const;

    **Returns**: an iterator to the first element in the bucket number ``n``.

---------------------------------------------------------------------------------------------

    .. code:: cpp

        local_iterator unsafe_end( size_type n );

        const_local_iterator unsafe_end( size_type n ) const;

        const_local_iterator unsafe_cend( size_type n ) const;

    **Returns**: an iterator to the element that follows the last element in
    the bucket number ``n``.

The number of buckets
---------------------

    .. code:: cpp

        size_type unsafe_bucket_count() const;

    **Returns**: the number of buckets in the container.

---------------------------------------------------------------------------------------------

    .. code:: cpp

        size_type unsafe_max_bucket_count() const;

    **Returns**: the maximum number of buckets that container can hold.

Size of the bucket
------------------

    .. code:: cpp

        size_type unsafe_bucket_size( size_type n ) const;

    **Returns**: the number of elements in the bucket number ``n``.

Bucket number
-------------

    .. code:: cpp

        size_type unsafe_bucket( const key_type& key ) const;

    **Returns**: the number of the bucket in which the element with the key ``key`` is stored.
