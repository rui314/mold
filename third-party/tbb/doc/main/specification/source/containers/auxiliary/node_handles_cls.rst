.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

============
Node handles
============
**[containers.node_handles]**

Concurrent associative containers (``concurrent_map``, ``concurrent_multimap``, ``concurrent_set``, ``concurrent_multiset``,
``concurrent_unordered_map``, ``concurrent_unordered_multimap``, ``concurrent_unordered_set``,
and ``concurrent_unordered_multiset``) store elements in individually allocated, connected nodes.
These containers support data transfer between containers with compatible node types by changing the connections
without copying or moving the actual data.

Class synopsis
--------------

.. code:: cpp

    class node-handle { // Exposition-only name
    public:
        using key_type = <container-specific>;    // Only for maps
        using mapped_type = <container-specific>; // Only for maps
        using value_type = <container-specific>;  // Only for sets
        using allocator_type = <container-specific>;

        node-handle();
        node-handle( node-handle&& other );

        ~node-handle();

        node-handle& operator=( node-handle&& other );

        void swap( node-handle& nh );

        bool empty() const;
        explicit operator bool() const;

        key_type& key() const;       // Only for maps
        mapped_type& mapped() const; // Only for maps
        value_type& value() const;   // Only for sets

        allocator_type get_allocator() const;
    };

A ``node handle`` is a container-specific move-only nested type (exposed as `container::node_type`) that
represents a node outside of any container instance. It allows reading and modifying the data stored in the node,
and inserting the node into a compatible container instance. The following containers have compatible node types and
may exchange nodes:

* ``concurrent_map`` and ``concurrent_multimap`` with the same ``key_type``, ``mapped_type`` and
  ``allocator_type``.
* ``concurrent_set`` and ``concurrent_multiset`` with the same ``value_type`` and ``allocator_type``.
* ``concurrent_unordered_map`` and ``concurrent_unordered_multimap`` with the same ``key_type``, ``mapped_type`` and
  ``allocator_type``.
* ``concurrent_unordered_set`` and ``concurrent_unordered_multiset`` with the same ``value_type`` and ``allocator_type``.

Default or moved-from node handles are `empty` and do not represent a valid node.
A non-empty node handle is typically created when a node is extracted out of a container, for example, with the ``unsafe_extract``
method. It stores the node along with a copy of the container's allocator.
Upon assignment or destruction a non-empty node handle destroys the stored data and deallocates the node.

Member functions
----------------

Constructors
~~~~~~~~~~~~

    .. code:: cpp

        node-handle();

    Constructs an empty node handle.

-----------------------------------------------------------------------------

    .. code:: cpp

        node-handle( node-handle&& other );

    Constructs a node handle that takes ownership of the node from ``other``.

    ``other`` is left in an empty state.

Assignment
~~~~~~~~~~

    .. code:: cpp

        node-handle& operator=( node-handle&& other );

    Transfers ownership of the node from ``other`` to ``*this``.
    If ``*this`` was not empty before transferring, destroys and deallocates the stored node.

    Move assigns the stored allocator if ``std::allocator_traits<allocator_type>::propagate_on_container_move_assignment::value``
    is ``true``.

    ``other`` is left in an empty state.

Destructor
~~~~~~~~~~

    .. code:: cpp

        ~node-handle();

    Destroys the node handle. If it is not empty, destroys and deallocates the owned node.

Swap
~~~~

    .. code:: cpp

        void swap( node-handle& other )

    Exchanges the nodes owned by ``*this`` and ``other``.

    Swaps the stored allocators if ``std::allocator_traits<allocator_type>::propagate_on_container_swap::value`` is ``true``.

State
~~~~~

    .. code:: cpp

        bool empty() const;

    **Returns**: ``true`` if the node handle is empty, ``false`` otherwise.

-----------------------------------------------------------------------------

    .. code:: cpp

        explicit operator bool() const;

    Equivalent to ``!empty()``.

Access to the stored element
~~~~~~~~~~~~~~~~~~~~~~~~~~~~

    .. code:: cpp

        key_type& key() const;

    Available only for map node handles.

    **Returns**: a reference to the key of the element stored in the owned node.

    The behavior is undefined if the node handle is empty.

-----------------------------------------------------------------------------

    .. code:: cpp

        mapped_type& mapped() const;

    Available only for map node handles.

    **Returns**: a reference to the value of the element stored in the owned node.

    The behavior is undefined if the node handle is empty.

-----------------------------------------------------------------------------

    .. code:: cpp

        value_type& value() const;

    Available only for set node handles.

    **Returns**: a reference to the element stored in the owned node.

    The behavior is undefined if the node handle is empty.

get_allocator
~~~~~~~~~~~~~

    .. code:: cpp

        allocator_type get_allocator() const;

    **Returns**: a copy of the allocator stored in the node handle.

    The behavior is undefined if the node handle is empty.
