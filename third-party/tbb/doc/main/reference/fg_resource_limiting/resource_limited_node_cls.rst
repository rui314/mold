.. _resource_limited_node_cls:

``resource_limited_node`` Class
===============================

Description
***********

The ``resource_limited_node<Input, OutputTuple>`` receives messages at a single input port
and requests access to resources provided by one or several resource providers before processing
the message.

It may generate one or more output messages that are broadcast to successors using ``N`` output ports,
where ``N`` is ``std::tuple_size<OutputTuple>::value``.

The concurrency threshold of the node can be set to one of the
`predefined values <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/flow_graph/predefined_concurrency_limits>`_,
or any value of type ``std::size_t``.

When the input message arrives at the node, it occupies one slot of the node's concurrency threshold.
If the threshold allows, and access to all required resources is granted by each associated ``resource_limiter``,
the node executes the user-provided body on the input message.

After execution of the user-provided body, all the resources are returned to their respective ``resource_limiter`` objects.

If the concurrency threshold is exceeded, or  access to one or more required resources is not granted, the input message is queued in
the internal buffer and is processed once concurrency allows and access to all resources is granted.

``resource_limited_node`` is a ``graph_node`` and a ``receiver<Input>``.

It has a tuple of ``sender<Output>`` output ports, where ``Output`` is a type of element in ``OutputTuple``.

``resource_limited_node`` has the *discarding* and *broadcast-push*
`properties <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/flow_graph/forwarding_and_buffering>`_.


API
***

Header
------

.. code:: cpp

    #define TBB_PREVIEW_FLOW_GRAPH_FEATURES 1
    // or
    #define TBB_PREVIEW_FLOW_GRAPH_RESOURCE_LIMITING 1

    #include <oneapi/tbb/flow_graph.h>

Synopsis
--------

.. code:: cpp

    namespace oneapi {
        namespace tbb {
            namespace flow {

                template <typename Input, typename OutputTuple>
                class resource_limited_node
                    : public graph_node, public receiver<Input>
                {
                public:
                    using output_ports_type = /*unspecified*/;

                    template <typename Body, typename ResourceLimiter, typename... ResourceLimiters>
                    resource_limited_node(graph& g, std::size_t concurrency,
                                          std::tuple<ResourceLimiter&, ResourceLimiters&...> resource_limiters,
                                          Body body);

                    resource_limited_node(const resource_limited_node& other);
                    ~resource_limited_node();

                    bool try_put(const Input& input);
                }; // class resource_limited_node

            } // namespace flow
        } // namespace tbb
    } // namespace oneapi

Requirements
------------

* The ``Input`` type must meet the ``DefaultConstructible`` requirements from [defaultconstructible]
  and the ``CopyConstructible`` requirements from [copyconstructible] sections of the ISO C++ Standard.
* The ``OutputTuple`` type must be a specialization of ``std::tuple``.

Member Types
------------

.. code:: cpp

    using output_ports_type = /*unspecified*/;

An alias for a ``std::tuple`` of the node's output ports.

Member Functions
----------------

.. code:: cpp

    template <typename Body, typename ResourceLimiter, typename... ResourceLimiters>
    resource_limited_node(graph& g, std::size_t concurrency,
                          std::tuple<ResourceLimiter&, ResourceLimiters&...> resource_limiters,
                          Body body);

**Requirements**:

1. The type ``Body`` must meet the requirements of :ref:`ResourceLimitedNodeBody <resource_limited_node_body_named_requirement>`.
2. The type ``ResourceLimiter`` and each type ``RL`` in ``ResourceLimiters`` must be specializations of
   :ref:`oneapi::tbb::flow::resource_limiter <resource_limiter_cls>`.

Constructs a ``resource_limited_node`` that

* Belongs to the graph ``g``.
* Has concurrency threshold set to ``concurrency``.
* Uses the ``body`` object.
* Consumes the resources provided by each element in ``resource_limiters``.

.. note::

    The body object passed to the constructor is copied. Updates to member variables do not affect the original
    object used to construct the node. If the state held within a body object must be inspected from outside
    the node, the `copy_body function <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/flow_graph/copy_body_func>`_
    can be used to obtain an updated body.

------------------------------------------------------

.. code:: cpp

    resource_limited_node(const resource_limited_node& other);

Constructs a ``resource_limited_node`` with the same initial state that ``other`` had when it was constructed:

* It belongs to the same ``graph`` object.
* It has the same concurrency threshold.
* It uses a copy of the initial body of ``other``.
* It consumes the same set of resources.

The predecessors and successors of ``other`` are not copied.

The new body object is copy-constructed from a copy of the original body provided to ``other`` at its construction.
Changes made to member variables in ``other``'s body after the construction of ``other`` do not affect the body of the
new ``resource_limited_node``.

------------------------------------------------------

.. code:: cpp

    ~resource_limited_node();

Destroys the ``resource_limited_node`` object.

------------------------------------------------------

.. code:: cpp

    bool try_put(const Input& input);

Passes the incoming message ``input`` to the node. Once the concurrency threshold allows and the
access to all required resources is granted, the node executes the user-provided body on ``input``.

**Returns**: ``true``.
