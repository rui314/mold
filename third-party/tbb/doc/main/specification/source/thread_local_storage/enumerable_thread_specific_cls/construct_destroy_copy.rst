.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==================================
Construction, destruction, copying
==================================

Empty container constructors
----------------------------

.. code:: cpp

    enumerable_thread_specific();

Constructs an ``enumerable_thread_specific`` where each thread-local element will be default-constructed.

.. code:: cpp

    template<typename Finit> explicit enumerable_thread_specific( Finit finit );

Constructs an ``enumerable_thread_specific`` such that any thread-local element will be created by copying the result of ``finit()``.

.. note::

    The expression ``finit()`` must be safe to evaluate concurrently by multiple threads.
    It is evaluated each time a thread-local element is created.

.. code:: cpp

    explicit enumerable_thread_specific( const T& exemplar );

Constructs an ``enumerable_thread_specific`` where each thread-local element will be copy-constructed from ``exemplar``.

.. code:: cpp

    explicit enumerable_thread_specific( T&& exemplar );

Constructs an ``enumerable_thread_specific`` object, move constructor of ``T`` can be used to store ``exemplar``
internally; however, thread-local elements are always copy-constructed.

.. code:: cpp

    template <typename... Args> enumerable_thread_specific( Args&&... args );

Constructs ``enumerable_thread_specific`` such that any thread-local element will be constructed by invoking ``T(args...)``.

.. note::

    This constructor does not participate in overload resolution if the type of the first argument in ``args...`` is ``T``,
    or ``enumerable_thread_specific<T>``, or ``foo()`` is a valid expression for a value ``foo`` of that type.

Copying constructors
--------------------

.. code:: cpp

    enumerable_thread_specific ( const enumerable_thread_specific& other );

    template<typename Alloc, ets_key_usage_type Cachetype> enumerable_thread_specific( const enumerable_thread_specific <T, Alloc, Cachetype>& other );

Constructs an ``enumerable_thread_specific`` as a copy of ``other``.
The values are copy-constructed from the values in ``other`` and have same thread correspondence.

Moving constructors
-------------------

.. code:: cpp

    enumerable_thread_specific ( enumerable_thread_specific&& other )

Constructs an ``enumerable_thread_specific`` by moving the content of ``other`` intact.
``other`` is left in an unspecified state, but can be safely destroyed.

.. code:: cpp

    template<typename Alloc, ets_key_usage_type Cachetype> enumerable_thread_specific( enumerable_thread_specific <T, Alloc, Cachetype>&& other )

Constructs an ``enumerable_thread_specific`` using per-element move construction from the values in ``other``, and
keeping their thread correspondence. ``other`` is left in an unspecified state, but can be safely destroyed.

Destructor
----------

.. code:: cpp

    ~enumerable_thread_specific()

Destroys all elements in ``*this``.
Destroys any native TLS keys that were created for this instance.

Assignment operators
--------------------

.. code:: cpp

    enumerable_thread_specific& operator=( const enumerable_thread_specific& other );

Copies the content of ``other`` to ``*this``. Returns a reference to ``this*``.

.. code:: cpp

    template<typename Alloc, ets_key_usage_type Cachetype>
    enumerable_thread_specific& operator=( const enumerable_thread_specific<T, Alloc, Cachetype>& other );

Copies the content of ``other`` to ``*this``. Returns a reference to ``this*``.

.. note::

    The allocator and key usage specialization is unchanged by this call.

.. code:: cpp

    enumerable_thread_specific& operator=( enumerable_thread_specific&& other );

Moves the content of ``other`` to ``*this`` intact.
An ``other`` is left in an unspecified state, but can be safely destroyed. Returns a reference to ``this*``.

.. code:: cpp

    template<typename Alloc, ets_key_usage_type Cachetype>
    enumerable_thread_specific& operator=( enumerable_thread_specific<T, Alloc, Cachetype>&& other );

Moves the content of ``other`` to ``*this`` using per-element move construction and keeping thread correspondence.
An ``other`` is left in an unspecified state, but can be safely destroyed. Returns a reference to ``this*``.

.. note::

    The allocator and key usage specialization is unchanged by this call.

