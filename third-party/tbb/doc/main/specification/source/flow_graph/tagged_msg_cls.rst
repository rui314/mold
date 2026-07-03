.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==========
tagged_msg
==========
**[flow_graph.tagged_msg]**

A class template composed of a tag and a message. The message is a
value that can be one of several defined types.

.. code:: cpp

    // Defined in header <oneapi/tbb/flow_graph.h>

    namespace oneapi {
    namespace tbb {
    namespace flow {

        template<typename TagType, typename... TN>
        class tagged_msg {
        public:
            template<typename T, typename R>
            tagged_msg(T const &index, R const &val);

            TagType tag() const;

            template<typename V>
            const V& cast_to() const;

            template<typename V>
            bool is_a() const;

        };

    } // namespace flow
    } // namespace tbb
    } // namespace oneapi

Requirements:

* All types in ``TN`` template parameter pack must meet the
  `CopyConstructible` requirements from [copyconstructible] ISO C++ Standard
  section.
* The type `TagType` must be an integral unsigned type.

The ``tagged_msg`` class template is intended for messages whose type is determined at runtime.
A message of one of the types ``TN`` is tagged with a tag of type ``TagType``. The tag then can
serve to identify the message. In the flow graph, ``tagged_msg`` is used as the output of
:doc:`indexer_node <indexer_node_cls>`.

Member functions
----------------

.. cpp:function:: template<typename T, typename R> \
                  tagged_msg(T const &index, R const &value)

    Requirements:

    * The type `R` must be the same as one of the ``TN`` types.
    * The type `T` must be acceptable as a ``TagType`` constructor parameter.

    Constructs a ``tagged_msg`` with tag ``index`` and value ``val``.

.. cpp:function:: TagType tag() const

    Returns the current tag.

.. cpp:function:: template<typename V> \
                  const V& cast_to() const

    Requirements:

    * The type ``V`` must be the same as one of the ``TN`` types.

    Returns the value stored in ``tagged_msg``. If the value is not of type ``V``, the
    ``std::runtime_error`` exception is thrown.

.. cpp:function:: template<typename V> \
                  bool is_a() const

    Requirements:

    * The type ``V`` must be the same as one of the ``TN`` types.

    Returns true if ``V`` is the type of the value held by the ``tagged_msg``.  Returns false, otherwise.

Non-member functions
--------------------

.. code:: cpp

    template<typename V, typename T>
    const V& cast_to(T const &t) {
        return t.cast_to<V>();
    }

    template<typename V, typename T>
    bool is_a(T const &t);

Requirements:

* The type ``T`` must be an instantiated ``tagged_msg`` class template.
* The type ``V`` must be the same as one of the corresponding template arguments for ``tagged_msg``.

The free-standing template functions ``cast_to`` and  ``is_a`` applied to a ``tagged_msg`` object
are equivalent to the calls of the corresponding methods of that object.

See also:

* :doc:`indexer_node class template <indexer_node_cls>`
