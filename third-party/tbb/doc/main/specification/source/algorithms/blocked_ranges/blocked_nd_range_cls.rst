.. SPDX-FileCopyrightText: 2019-2025 Intel Corporation
.. SPDX-FileCopyrightText: Contributors to the oneAPI Specification project.
..
.. SPDX-License-Identifier: CC-BY-4.0

================
blocked_nd_range
================
**[algorithms.blocked_nd_range]**

Class template that represents a recursively divisible N-dimensional half-open interval.

A ``blocked_nd_range`` is the N-dimensional extension of ``blocked_range``.
You can interpret it as a Cartesian product of N instances of ``blocked_range``.
It meets the :doc:`Range requirements <../../named_requirements/algorithms/range>`.

Unlike ``blocked_range2d`` and ``blocked_range3d``, all dimensions of ``blocked_nd_range`` must be specified
over the same ``Value`` type. The class constructors also differ from those of ``blocked_range2d/3d``.
Different naming patterns indicate the distinction between the classes.
For example, ``blocked_nd_range<int,2>`` is analogous but not identical to ``blocked_range2d<int,int>``.

.. code:: cpp

    namespace oneapi {
        namespace tbb {

            template<typename Value, unsigned int N>
            class blocked_nd_range {
            public:
                // Types
                using value_type = Value;
                using dim_range_type = blocked_range<value_type>;
                using size_type = typename dim_range_type::size_type;

                // Constructors
                blocked_nd_range(const dim_range_type& dim0 /*, ... - exactly N parameters of the same type*/);
                blocked_nd_range(const value_type (&dim_size)[N], size_type grainsize = 1);
                blocked_nd_range(blocked_nd_range& r, split); 
                blocked_nd_range(blocked_nd_range& r, proportional_split proportion); 

                // Capacity
                static constexpr unsigned int dim_count();
                bool empty() const;

                // Access
                bool is_divisible() const;
                const dim_range_type& dim(unsigned int dimension) const;
            }; // class blocked_nd_range

            // Deduction Guides
            template <typename Value, typename... Values>
            blocked_nd_range(blocked_range<Value>, blocked_range<Values>...)
            -> blocked_nd_range<Value, 1 + sizeof...(Values)>;

            template <typename Value, unsigned int N>
            blocked_nd_range(const Value (&)[N], typename blocked_nd_range<Value, N>::size_type = 1)
            -> blocked_nd_range<Value, N>;

            template <typename Value, /*a template parameter pack*/... Xs>
            blocked_nd_range(/*a type deducible from a braced initialization list of Value objects*/... args)
            -> blocked_nd_range<Value, sizeof...(args)>; // see below

        } // namespace tbb
    } // namespace oneapi        

Requirements:

* ``N`` must be greater than 0.
* The *Value* must meet the :doc:`BlockedRangeValue requirements <../../named_requirements/algorithms/blocked_range_val>`.

Member types
------------

.. code:: cpp

    using value_type = Value;

The type of the range values.

.. code:: cpp

    using dim_range_type = blocked_range<value_type>;

The type that represents one out of the N dimensions.

.. code:: cpp

    using size_type = typename dim_range_type::size_type;

The type for measuring the size of a dimension.

Member functions
----------------

.. code:: cpp

    blocked_nd_range( const dim_range_type& dim0 /*, ... - exactly N parameters of the same type*/ );

**Effects:**  Constructs a ``blocked_nd_range`` representing an N-dimensional space of values.
The space is the half-open Cartesian product of one-dimensional ranges ``dim0 x ...``.
The constructor must take exactly N arguments, which types match ``const dim_range_type&``.

**Example:** For ``blocked_nd_range<int,4>``, this constructor is equivalent to
``blocked_nd_range( const blocked_range<int>&, const blocked_range<int>&, const blocked_range<int>&, const blocked_range<int>& )``.

.. note::
    This constructor cannot be substituted with a variadic template constructor
    ``template <typename... Dims> blocked_nd_range( const Dims&... dims )``, even if the latter
    is constrained by the size and type requirements for the parameter pack ``Dims``.
    That is because the types in ``Dims`` could not be automatically deduced from arguments specified as
    braced initialization lists, and so expressions like ``blocked_nd_range<int,4>{{0,1},{0,2},{0,3},{0,4}}``
    would fail to compile.

.. code:: cpp

    blocked_nd_range( const value_type (&dim_size)[N], size_type grainsize = 1 );

**Effects:**  Constructs a ``blocked_nd_range`` representing an N-dimensional space of values.
The space is the half-open Cartesian product of ranges ``[0, dim_size[0]) x [0, dim_size[1]) x ...``
each having the same grain size.

**Example:**  The ``blocked_nd_range<int,4> r( {5,6,7,8}, 4 );`` statement constructs a four-dimensional
space that contains all value tuples ``(i, j, k, l)``, where ``i`` ranges from 0 (included)
to 5 (excluded) with a grain size of 4, ``j`` ranges from 0 to 6 with a grain size of 4, and so forth.

.. code:: cpp

    blocked_nd_range( blocked_nd_range& range, split );

Basic splitting constructor.

**Requirements**: ``is_divisible()`` is true.

**Effects**: Partitions ``range`` into two subranges. The newly constructed ``blocked_nd_range`` is approximately
the half of the original ``range``, and ``range`` is updated to be the remainder.
Splitting is done across one dimension, while other dimensions and the grain sizes for
each subrange remain the same as in the original ``range``.

.. note::
    It is recommended to split across the dimension with the biggest size-to-grainsize ratio,
    so that, after repeated splitting, subranges become of approximately square/cubic/hypercubic shape
    if all grain sizes are the same.

.. code:: cpp

    blocked_nd_range( blocked_nd_range& range, proportional_split proportion );

Proportional splitting constructor.

**Requirements**: ``is_divisible()`` is true.

**Effects**: Partitions ``range`` into two subranges in the given ``proportion`` across one of its dimensions.
The effect is similar to the basic splitting constructor, except for proportional splitting of the selected
dimension, as specified for :doc:`blocked_range <blocked_range_cls>`.
Other dimensions and the grain sizes for each subrange remain the same as in the original ``range``.

.. code:: cpp

   static constexpr unsigned int dim_count();

**Returns:** The number of dimensions set by the class template argument ``N``.

.. code:: cpp

    bool empty() const;

**Effects**: Determines if the range is empty.

**Returns:** True if for any of the range dimensions ``empty()`` is true; false, otherwise.

.. code:: cpp

    bool is_divisible() const;

**Effects**: Determines if the range can be split into subranges.

**Returns:** True if for any of the range dimensions ``is_divisible()`` is true; false, otherwise.

.. code:: cpp

    const dim_range_type& dim(unsigned int dimension) const;

**Requirements**: 0 <= ``dimension`` < N.

**Returns:** ``blocked_range`` containing the value space along the dimension specified by the argument.

Deduction Guides
----------------

.. code:: cpp

    template <typename Value, typename... Values>
    blocked_nd_range(blocked_range<Value>, blocked_range<Values>...)
    -> blocked_nd_range<Value, 1 + sizeof...(Values)>;

**Effects:**: Enables deduction when a set of ``blocked_range`` objects is passed to the ``blocked_nd_range`` constructor.

**Constraints:**: Participates in overload resolution only if all of the types in ``Values`` are same as ``Value``.

.. code:: cpp

    template <typename Value, unsigned int N>
    blocked_nd_range(const Value (&)[N], typename blocked_nd_range<Value, N>::size_type = 1)
    -> blocked_nd_range<Value, N>;

**Effects:**: Enables deduction from a single C array object indicating a set of dimension sizes.

.. code:: cpp

    template <typename Value, /*a template parameter pack*/... Xs>
    blocked_nd_range(/*a type deducible from a braced initialization list of Value objects*/... args)
    -> blocked_nd_range<Value, sizeof...(args)>;

**Effects:**: Enables deduction when a set of ``blocked_range`` objects is provided as braced initialization lists to the ``blocked_nd_range`` constructor.

.. note::

    If a single braced initialization list is provided, it is interpreted as a C array of dimension sizes, not as a ``blocked_range``.

**Example**: ``blocked_nd_range range({0, 10}, {0, 10, 5})`` should deduce ``range`` as ``blocked_nd_range<int, 2>``.

This deduction guide should be implemented as one of the following alternatives:

    .. code:: cpp

        template <typename Value, typename... Values>
        blocked_nd_range(std::initializer_list<Value>, std::initializer_list<Values>...)
        -> blocked_nd_range<Value, 1 + sizeof...(Values)>;

    **Constraints:** Participates in overload resolution only if ``sizeof...(Values) > 0`` and all types in ``Values`` are the same as ``Value``.

or

    .. code:: cpp

        template <typename Value, unsigned int... Ns>
        blocked_nd_range(const Value (&... dim)[Ns])
        -> blocked_nd_range<Value, sizeof...(Ns)>;

    **Constraints:** Participates in overload resolution only if ``sizeof...(Ns) > 1`` and ``N == 2`` or ``N == 3`` for each ``N`` in ``Ns``.

In addition to the explicit deduction guides above, the implementation shall provide implicit or explicit deduction guides for copy constructor,
move constructor and constructors taking ``split`` and ``proportional_split`` arguments.

See also:

* :doc:`blocked_range <blocked_range_cls>`
* :doc:`blocked_range2d <blocked_range2d_cls>`
* :doc:`blocked_range3d <blocked_range3d_cls>`
