.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===========
Hash policy
===========

Hash policy of ``concurrent_unordered_multimap`` manages the number of buckets in the container and
the allowed maximum number of elements per bucket (load factor). If the maximum load factor is exceeded,
the container can automatically increase the number of buckets.

Load factor
-----------

    .. code:: cpp

        float load_factor() const;

    **Returns**: the average number of elements per bucket, which is ``size()/unsafe_bucket_count()``.

---------------------------------------------------------------------------------------------

    .. code:: cpp

        float max_load_factor() const;

    **Returns**: the maximum number of elements per bucket.

---------------------------------------------------------------------------------------------

    .. code:: cpp

        void max_load_factor( float ml );

    Sets the maximum number of elements per bucket to ``ml``.

Manual rehashing
----------------

    .. code:: cpp

        void rehash( size_type n );

    Sets the number of buckets to ``n`` and rehashes the container.

---------------------------------------------------------------------------------------------

    .. code:: cpp

        void reserve( size_type n );

    Sets the number of buckets to the value that is needed to store ``n`` elements.
