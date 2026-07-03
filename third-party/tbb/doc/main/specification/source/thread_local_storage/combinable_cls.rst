.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==========
combinable
==========
**[tls.combinable]**

A class template for holding thread-local values during a parallel computation
that will be merged into a final value.

A ``combinable`` provides each thread with its own instance of type ``T``.

.. code:: cpp

    // Defined in header <oneapi/tbb/combinable.h>

    namespace oneapi {
    namespace tbb {
        template <typename T>
        class combinable {
        public:
            combinable();

            combinable(const combinable& other);
            combinable(combinable&& other);

            template <typename FInit>
            explicit combinable(FInit finit);

            ~combinable();

            combinable& operator=( const combinable& other);
            combinable& operator=( combinable&& other);

            void clear();

            T& local();
            T& local(bool & exists);

            template<typename BinaryFunc> T combine(BinaryFunc f);
            template<typename UnaryFunc> void combine_each(UnaryFunc f);
        };
    } // namespace tbb
    } // namespace oneapi

Member functions
----------------

.. namespace:: oneapi::tbb::combinable
	       
.. cpp:function:: combinable()

    Constructs ``combinable`` such that thread-local instances of ``T`` will be default-constructed.

.. cpp:function:: template<typename FInit> explicit combinable(FInit finit)

    Constructs ``combinable`` such that thread-local elements will be created by copying the result of *finit()*.

    .. caution::

        The expression *finit()* must be safe to evaluate concurrently by multiple threads.
        It is evaluated each time a new thread-local element is created.

.. cpp:function:: combinable( const combinable& other )

    Constructs a copy of *other*, so that it has copies of each element in *other* with the same thread mapping.

.. cpp:function:: combinable( combinable&& other )

    Constructs ``combinable`` by moving the content of *other* intact.
    *other* is left in an unspecified state but can be safely destroyed.

.. cpp:function:: ~combinable()

    Destroys all elements in ``*this``.

.. cpp:function:: combinable& operator=( const combinable& other )

    Sets ``*this`` to be a copy of *other*.
    Returns a reference to ``*this``.

.. cpp:function:: combinable& operator=( combinable&& other )

    Moves the content of *other* to ``*this`` intact.
    *other* is left in an unspecified state but can be safely destroyed.
    Returns a reference to ``*this``.

.. cpp:function:: void clear()

    Removes all elements from ``*this``.

.. cpp:function:: T& local()

    If an element does not exist for the current thread, creates it.

    **Returns**: Reference to thread-local element.

.. cpp:function:: T& local( bool& exists )

    Similar to ``local()``, except that *exists* is set to true
    if an element was already present for the current thread; false, otherwise.

    **Returns**: Reference to thread-local element.

.. cpp:function:: template<typename BinaryFunc> T combine(BinaryFunc f)

    **Requires**: A ``BinaryFunc`` must meet the `Function Objects` requirements described in the [function.objects] section of the ISO C++ standard.
    Specifically, the type should be an associative binary functor with the signature ``T BinaryFunc(T,T)`` or ``T BinaryFunc(const T&,const T&)``.
    A ``T`` type must be the same as a corresponding template parameter for the ``combinable`` object.

    **Effects**: Computes a reduction over all elements using binary functor *f*.
    All evaluations of *f* are done sequentially in the calling thread.
    If there are no elements, creates the result using the same rules as for creating a new element.

    **Returns**: Result of the reduction.

.. cpp:function:: template<typename UnaryFunc> void combine_each(UnaryFunc f)

    **Requires**: An ``UnaryFunc`` must meet the `Function Objects` requirements described in the [function.objects] section of the ISO C++ standard.
    Specifically, the type should be an unary functor with the one of the signatures: ``void UnaryFunc(T)``, ``void UnaryFunc(T&)``, or ``void UnaryFunc(const T&)``
    A ``T`` type must be the same as a corresponding template parameter for the ``enumerable_thread_specific`` object.

    **Effects**: Evaluates *f(x)* for each thread-local element *x* in ``*this``.
    All evaluations are done sequentially in the calling thread.

.. note::

   Methods of ``class combinable`` are not thread-safe, except for ``local``.

