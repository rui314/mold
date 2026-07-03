.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=========
Observers
=========

get_allocator
-------------

    .. code:: cpp

        allocator_type get_allocator() const;

    **Returns**: a copy of the allocator associated with ``*this``.

hash_function
-------------

    .. code:: cpp

        hasher hash_function() const;

    **Returns**: a copy of the hash function associated with ``*this``.

key_eq
------

    .. code:: cpp

        key_equal key_eq() const;

    **Returns**: a copy of the key equality predicate associated with ``*this``.
