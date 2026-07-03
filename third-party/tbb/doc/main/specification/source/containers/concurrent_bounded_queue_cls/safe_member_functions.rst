.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==================================
Concurrently safe member functions
==================================

All member functions in this section can be performed concurrently with each other.

Pushing elements
----------------

    .. code:: cpp

        void push( const value_type& value );

    Waits until the number of items in the queue is less than the capacity and
    pushes a copy of ``value`` into the container.

    **Requirements**: the type ``T`` must meet the ``CopyInsertable`` requirements from the
    [container.requirements] ISO C++ Standard section.

-----------------------------

    .. code:: cpp

        bool try_push( const value_type& value );

    If the number of items in the queue is less than the capacity, pushes a copy
    of ``value`` into the container.

    **Requirements**: the type ``T`` must meet the ``CopyInsertable`` requirements from the
    [container.requirements] ISO C++ Standard section.

    **Returns**: ``true`` if the item was pushed; ``false``, otherwise.

-----------------------------

    .. code:: cpp

        void push( value_type&& value );

    Waits until the number of items in the queue is less than ``capacity()`` and
    pushes ``value`` into the container using move semantics.

    **Requirements**: the type ``T`` must meet the ``MoveInsertable`` requirements from the
    [container.requirements] ISO C++ Standard section.

    ``value`` is left in a valid, but unspecified state.

-----------------------------

    .. code:: cpp

        bool try_push( value_type&& value );

    If the number of items in the queue is less than the capacity, pushes ``value`` into
    the container using move semantics.

    **Requirements**: the type ``T`` must meet the ``MoveInsertable`` requirements from the
    [container.requirements] ISO C++ Standard section.

    ``value`` is left in a valid, but unspecified state.

    **Returns**: ``true`` if the item was pushed; ``false``, otherwise.

-----------------------------

    .. code:: cpp

        template <typename... Args>
        void emplace( Args&&... args );

    Waits until the number of items in the queue is less than ``capacity()`` and
    pushes a new element constructed from ``args`` into the container.

    **Requirements**: the type ``T`` must meet the ``EmplaceConstructible`` requirements from the
    [container.requirements] ISO C++ Standard section.

-----------------------------

    .. code:: cpp

        template <typename... Args>
        bool try_emplace( Args&&... args );

    If the number of items in the queue is less than the capacity, pushes a
    new element constructed from ``args`` into the container.

    **Requirements**: the type ``T`` must meet the ``EmplaceConstructible`` requirements from the
    [container.requirements] ISO C++ Standard section.

    **Returns**: ``true`` if the item was pushed; ``false``, otherwise.

Popping elements
----------------

    .. code:: cpp

        void pop( value_type& value );

    Waits until the item becomes available, copies it from the container, and
    assigns it to the ``value``. The popped element is destroyed.

    **Requirements**: the type ``T`` must meet the ``MoveAssignable`` requirements from the [moveassignable]
    ISO C++ Standard section.

-----------------------------

    .. code:: cpp

        bool try_pop( value_type& value );

    If the container is empty, does nothing.

    Otherwise, copies the last element from the container and assigns it to the ``value``.
    The popped element is destroyed.

    **Requirements**: the type ``T`` must meet the ``MoveAssignable`` requirements from the [moveassignable]
    ISO C++ Standard section.

    **Returns**: ``true`` if the element was popped; ``false``, otherwise.

abort
-----

    .. code:: cpp

        void abort();

    Wakes up any threads that are waiting on the queue via ``push``, ``pop``, or ``emplace``
    operations and raises the ``oneapi::tbb::user_abort`` exception on those threads.

Capacity of the queue
---------------------

    .. code:: cpp

        size_type capacity() const;

    **Returns**: the maximum number of items that the queue can hold.

-----------------------------

    .. code:: cpp

        void set_capacity( size_type new_capacity ) const;

    Sets the maximum number of items that the queue can hold to ``new_capacity``.


get_allocator
-------------

    .. code:: cpp

        allocator_type get_allocator() const;

    **Returns**: a copy of the allocator, associated with ``*this``.
