.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=================
Concurrent growth
=================

All member functions in this section can be performed concurrently with each other,
element access methods and while traversing the container.

grow_by
-------

    .. code:: cpp

        iterator grow_by( size_type delta );

    Appends a sequence comprising ``delta`` new default-constructed
    in-place elements to the end of the vector.

    **Returns**: iterator to the beginning of the appended sequence.

    **Requirements**: the type ``value_type`` must meet the ``DefaultConstructible`` and ``EmplaceConstructible`` requirements
    from [defaultconstructible] and [container.requirements] ISO C++ sections.

---------------------------------------------

    .. code:: cpp

        iterator grow_by( size_type delta, const value_type& value );

    Appends a sequence comprising ``delta`` copies of ``value`` to the end of the vector.

    **Returns**: iterator to the beginning of the appended sequence.

    **Requirements**: the type ``value_type`` must meet the ``CopyInsertable`` requirements from
    the [container.requirements] ISO C++ Standard section.

---------------------------------------------

    .. code:: cpp

        template <typename InputIterator>
        iterator grow_by( InputIterator first, InputIterator last );

    Appends a sequence comprising all elements from the half-open interval ``[first, last)``
    to the end of the vector.

    **Returns**: iterator to the beginning of the appended sequence.

    This overload participates in overload resolution only if the type ``InputIterator``
    meets the requirements of `InputIterator` from the [input.iterators] ISO C++ Standard section.

---------------------------------------------

    .. code:: cpp

        iterator grow_by( std::initializer_list<value_type> init );

    Equivalent to ``grow_by(init.begin(), init.end())``.

grow_to_at_least
----------------

    .. code:: cpp

        iterator grow_to_at_least( size_type n );

    Appends minimal sequence of default constructed in-place elements such that ``size() >= n``.

    **Returns**: iterator to the beginning of the appended sequence.

    **Requirements**: the type ``value_type`` must meet the ``DefaultConstructible`` and ``EmplaceConstructible`` requirements
    from [defaultconstructible] and [container.requirements] ISO C++ sections.

---------------------------------------------

    .. code:: cpp

        iterator grow_to_at_least( size_type n, const value_type& value );

    Appends minimal sequence of comprising copies of ``value`` such that ``size() >= n``.

    **Returns**: iterator to the beginning of the appended sequence.

    **Requirements**: the type ``value_type`` must meet the ``CopyInsertable`` requirements from the
    [container.requirements] ISO C++ Standard section.

push_back
---------

    .. code:: cpp

        iterator push_back( const value_type& value );

    Appends a copy of ``value`` to the end of the vector.

    **Returns**: iterator to the appended element.

    **Requirements**: the type ``value_type`` must meet the ``CopyInsertable`` requirements from the
    [container.requirements] ISO C++ Standard section.

---------------------------------------------

    .. code:: cpp

        iterator push_back( value_type&& value );

    Appends ``value`` to the end of the vector using move semantics.

    ``value`` is left in a valid, but unspecified state.

    **Returns**: iterator to the appended element.

    **Requirements**: the type ``value_type`` must meet the ``MoveInsertable`` requirements from the
    [container.requirements] ISO C++ Standard section.

emplace_back
------------

    .. code:: cpp

        template <typename... Args>
        iterator emplace_back( Args&&... args );

    Appends an element constructed in-place from ``args`` to the end of the vector.

    **Returns**: iterator to the appended element.

    **Requirements**: the type ``value_type`` must meet the ``EmplaceConstructible`` requirements
    from the [container.requirements] ISO C++ section.
