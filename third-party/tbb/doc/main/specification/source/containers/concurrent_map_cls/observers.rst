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

key_comp
--------

    .. code:: cpp

        key_compare key_comp() const;

    **Returns**: a copy of the key comparison functor associated with ``*this``.

value_comp
----------

    .. code:: cpp

        value_compare value_comp() const;

    **Returns**: an object of the ``value_compare`` class that is used to compare
    ``value_type`` objects.
