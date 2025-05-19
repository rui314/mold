.. _blocked_nd_range_ctad:

Deduction Guides for ``blocked_nd_range``
=========================================

.. note::
    To enable this feature, define the ``TBB_PREVIEW_BLOCKED_ND_RANGE_DEDUCTION_GUIDES`` macro to 1.

.. contents::
    :local:
    :depth: 1

Description
***********

The ``blocked_nd_range`` class represents a recursively divisible N-dimensional half-open interval for the oneTBB
parallel algorithms. This feature extends ``blocked_nd_range`` to support Class Template Argument
Deduction (starting from C++17). With that, you do not need to specify template arguments explicitly
while creating a ``blocked_nd_range`` object if they can be inferred from the constructor arguments:

.. literalinclude:: ./examples/blocked_nd_range_ctad_example.cpp
    :language: c++
    :start-after: /*begin_blocked_nd_range_ctad_example_1*/
    :end-before: /*end_blocked_nd_range_ctad_example_1*/

.. note::
    For more detailed description of the implementation of this feature or to leave comments or feedback on the API, please
    refer to the [corresponding RFC](https://github.com/uxlfoundation/oneTBB/tree/master/rfcs/experimental/blocked_nd_range_ctad).

API
***

Header
------

.. code:: cpp
    
    #include <oneapi/tbb/blocked_nd_range.h>

Synopsis
--------

.. code:: cpp

    namespace oneapi {
    namespace tbb {

        template <typename Value, unsigned int N>
        class blocked_nd_range {
        public:
            // Member types and constructors defined as part of oneTBB specification
            using value_type = Value;
            using dim_range_type = blocked_range<value_type>;
            using size_type = typename dim_range_type::size_type;

            blocked_nd_range(const dim_range_type& dim0, /*exactly N arguments of type const dim_range_type&*/); // [1]
            blocked_nd_range(const value_type (&dim_size)[N], size_type grainsize = 1);                          // [2]
            blocked_nd_range(blocked_nd_range& r, split);                                                        // [3]
            blocked_nd_range(blocked_nd_range& r, proportional_split);                                           // [4]
        }; // class blocked_nd_range

        // Explicit deduction guides
        template <typename Value, typename... Values>
        blocked_nd_range(blocked_range<Value>, blocked_range<Values>...)
        -> blocked_nd_range<Value, 1 + sizeof...(Values)>;

        template <typename Value, unsigned int... Ns>
        blocked_nd_range(const Value (&...)[Ns])
        -> blocked_nd_range<Value, sizeof...(Ns)>;

        template <typename Value, unsigned int N>
        blocked_nd_range(const Value (&)[N], typename blocked_nd_range<Value, N>::size_type = 1)
        -> blocked_nd_range<Value, N>;

        template <typename Value, unsigned int N>
        blocked_nd_range(blocked_nd_range<Value, N>, split)
        -> blocked_nd_range<Value, N>;

        template <typename Value, unsigned int N>
        blocked_nd_range(blocked_nd_range<Value, N>, proportional_split)
        -> blocked_nd_range<Value, N>;
    } // namespace tbb
    } // namespace oneapi

Deduction Guides
----------------

The copy and move constructors of ``blocked_nd_range`` provide implicitly generated deduction guides. 
In addition, the following explicit deduction guides are provided:

.. code:: cpp

    template <typename Value, typename... Values>
    blocked_nd_range(blocked_range<Value>, blocked_range<Values>...)
    -> blocked_nd_range<Value, 1 + sizeof...(Values)>;

**Effects**: Enables deduction when a set of ``blocked_range`` objects is passed to the ``blocked_nd_range`` constructor ``[1]``.

**Constraints**: Participates in overload resolution only if all of the types in `Values` are same as `Value`.

.. code:: cpp

    template <typename Value, unsigned int... Ns>
    blocked_nd_range(const Value (&...)[Ns])
    -> blocked_nd_range<Value, sizeof...(Ns)>;

**Effects**: Enables deduction when a set of ``blocked_range`` objects is provided as braced-init-lists 
to the ``blocked_nd_range`` constructor ``[1]``.

**Constraints**: Participates in overload resolution only if ``sizeof...(Ns) >= 2``, and each integer ``Ni`` in ``Ns``
is either ``2`` or ``3``, corresponding to ``blocked_range`` constructors with 2 and 3 arguments, respectively.

.. note:: 
    The guide allows a deduction only from braced-init-lists containing objects of the same type. 
    For ranges with non-integral ``value_type``, setting an explicit grainsize argument
    is not supported by the deduction guides and requires specifying explicit template arguments.

.. code:: cpp

    template <typename Value, unsigned int N>
    blocked_nd_range(const Value (&)[N], typename blocked_nd_range<Value, N>::size_type = 1)
    -> blocked_nd_range<Value, N>;

**Effects**: Allows deduction from a single C array object indicating a set of dimension sizes to constructor 
``2`` of ``blocked_nd_range``.

.. code:: cpp

    template <typename Value, unsigned int N>
    blocked_nd_range(blocked_nd_range<Value, N>, split)
    -> blocked_nd_range<Value, N>;

**Effects**: Allows deduction while using the splitting constructor ``3`` of ``blocked_nd_range``.

.. code:: cpp

    template <typename Value, unsigned int N>
    blocked_nd_range(blocked_nd_range<Value, N>, proportional_split)
    -> blocked_nd_range<Value, N>;

**Effects**: Allows deduction while using the proportional splitting constructor ``4`` of ``blocked_nd_range``.

Example
-------

.. literalinclude:: ./examples/blocked_nd_range_ctad_example.cpp
    :language: c++
    :start-after: /*begin_blocked_nd_range_ctad_example_2*/
    :end-before: /*end_blocked_nd_range_ctad_example_2*/
