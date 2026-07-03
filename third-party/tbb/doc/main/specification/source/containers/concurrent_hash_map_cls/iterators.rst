.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=========
Iterators
=========

The types ``concurrent_hash_map::iterator`` and ``concurrent_hash_map::const_iterator``
meet the requirements of ``ForwardIterator`` from the [forward.iterators] ISO C++ Standard section.

All member functions in this section can only be performed serially.
The behavior is undefined in case of concurrent execution of these member functions
with other (either concurrently safe) methods.

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

equal_range
-----------

    .. code:: cpp

        std::pair<iterator, iterator> equal_range( const key_type& key );

        std::pair<const_iterator, const_iterator> equal_range( const key_type& key ) const;

    **Returns**: a range containing an element that is equivalent to ``key``.
    If there is no such element in the container, returns ``{end(), end()}`` .

--------------------------

    .. code:: cpp

        template <typename K>
        std::pair<iterator, iterator> equal_range( const K& key );

        template <typename K>
        std::pair<const_iterator, const_iterator> equal_range( const K& key ) const;

    **Returns**: a range containing an element which compares equivalent to the value ``key``.
    If there is no such element in the container, returns ``{end(), end()}``.

    This overload only participates in the overload resolution if qualified-id
    ``hash_compare_type::is_transparent`` is valid and denotes a type.
