.. _try_put_and_wait:

Waiting for Single Messages in Flow Graph
=========================================

.. contents::
    :local:
    :depth: 1

Description
***********

This feature adds a new ``try_put_and_wait`` interface to the receiving nodes in the Flow Graph.
This function puts a message as an input into a Flow Graph and waits until all work related to
that message is complete.
``try_put_and_wait`` may reduce latency compared to calling ``graph::wait_for_all`` since
``graph::wait_for_all`` waits for all work, including work that is unrelated to the input message, to complete.

``node.try_put_and_wait(msg)`` performs ``node.try_put(msg)`` on the node and waits until the work on ``msg`` is completed.
Therefore, the following conditions are true:

* Any task initiated by any node in the Flow Graph that involves working with ``msg`` or any other intermediate result
  computed from ``msg`` is completed.
* No intermediate results computed from ``msg`` remain in any buffers in the graph.

.. caution::

    To prevent ``try_put_and_wait`` calls from infinite waiting, avoid using buffering nodes at the end of the Flow Graph since the final result
    will not be automatically consumed by the Flow Graph.

.. caution::

    The ``multifunction_node`` and ``async_node`` classes are not currently supported by this feature. Including one of these nodes in the
    Flow Graph may cause ``try_put_and_wait`` to exit early, even if the computations on the initial input message are
    still in progress.

API
***

Header
------

.. code:: cpp

    #define TBB_PREVIEW_FLOW_GRAPH_FEATURES // macro option 1
    #define TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT // macro option 2
    #include <oneapi/tbb/flow_graph.h>

Synopsis
--------

.. code:: cpp

    namespace oneapi {
        namespace tbb {
            template <typename Output, typename Policy = /*default-policy*/>
            class continue_node {
            public:
                bool try_put_and_wait(const continue_msg& input);
            }; // class continue_node

            template <typename Input, typename Output = continue_msg, typename Policy = /*default-policy*/>
            class function_node {
            public:
                bool try_put_and_wait(const Input& input);
            }; // class function_node

            template <typename T>
            class overwrite_node {
            public:
                bool try_put_and_wait(const T& input);
            }; // class overwrite_node

            template <typename T>
            class write_once_node {
            public:
                bool try_put_and_wait(const T& input);
            }; // class write_once_node

            template <typename T>
            class buffer_node {
            public:
                bool try_put_and_wait(const T& input);
            }; // class buffer_node

            template <typename T>
            class queue_node {
            public:
                bool try_put_and_wait(const T& input);
            }; // class queue_node

            template <typename T, typename Compare = std::less<T>>
            class priority_queue_node {
            public:
                bool try_put_and_wait(const T& input);
            }; // class priority_queue_node

            template <typename T>
            class sequencer_node {
            public:
                bool try_put_and_wait(const T& input);
            }; // class sequencer_node

            template <typename T, typename DecrementType = continue_msg>
            class limiter_node {
            public:
                bool try_put_and_wait(const T& input);
            }; // class limiter_node

            template <typename T>
            class broadcast_node {
            public:
                bool try_put_and_wait(const T& input);
            }; // class broadcast_node

            template <typename TupleType>
            class split_node {
            public:
                bool try_put_and_wait(const TupleType& input);
            }; // class split_node
        } // namespace tbb
    } // namespace oneapi

Member Functions
----------------

.. code:: cpp

    template <typename Output, typename Policy>
    bool continue_node<Output, Policy>::try_put_and_wait(const continue_msg& input)

**Effects**: Increments the count of input signals received. If the incremented count is equal to the number
of known predecessors, performs the ``body`` function object execution.

Waits for the completion of the ``input`` in the Flow Graph, meaning all tasks created by each node and
related to ``input`` are executed, and no related objects remain in any buffer within the graph.

**Returns**: ``true``.

.. code:: cpp

    template <typename Input, typename Output, typename Policy>
    bool function_node<Input, Output, Policy>::try_put_and_wait(const Input& input)

**Effects**: If the concurrency limit allows, executes the user-provided body on the incoming message ``input``.
Otherwise, depending on the ``Policy`` of the node, either queues the incoming message ``input`` or rejects it.

Waits for the completion of the ``input`` in the Flow Graph, meaning all tasks created by each node and
related to ``input`` are executed, and no related objects remain in any buffer within the graph.

**Returns**: ``true`` if the input is accepted, ``false`` otherwise.

.. code:: cpp

    template <typename T>
    bool overwrite_node<T>::try_put_and_wait(const T& input)

**Effects**: Stores ``input`` in the internal single-item buffer and broadcasts it to all successors.

Waits for the completion of the ``input`` in the Flow Graph, meaning all tasks created by each node and
related to ``input`` are executed, and no related objects remain in any buffer within the graph.

**Returns**: ``true``.

.. caution::

    Since the input element is not retrieved from ``overwrite_node`` once accepted by the successor,
    retrieve it by explicitly calling the ``clear()`` method or by overwriting with another element to prevent
    ``try_put_and_wait`` from indefinite waiting.

.. code:: cpp

    template <typename T>
    bool write_once_node<T>::try_put_and_wait(const T& input)

**Effects**: Stores ``input`` in the internal single-item buffer if it does not contain a valid value already.
If a new value is set, the node broadcasts it to all successors.

Waits for the completion of the ``input`` in the Flow Graph, meaning all tasks created by each node and
related to ``input`` are executed, and no related objects remain in any buffer within the graph.

**Returns**: ``true`` for the first time after construction or a call to ``clear()``.

.. caution::

    Since the input element is not retrieved from the ``write_once_node`` once accepted by the successor,
    retrieve it by explicitly calling the ``clear()`` method to prevent ``try_put_and_wait`` from indefinite waiting.

.. code:: cpp

    template <typename T>
    bool buffer_node<T>::try_put_and_wait(const T& input)

**Effects**: Adds ``input`` to the set of items managed by the node and tries forwarding it to a successor.

Waits for the completion of the ``input`` in the Flow Graph, meaning all tasks created by each node and
related to ``input`` are executed, and no related objects remain in any buffer within the graph.

**Returns**: ``true``.

.. code:: cpp

    template <typename T>
    bool queue_node<T>::try_put_and_wait(const T& input)

**Effects**: Adds ``input`` to the set of items managed by the node and tries forwarding the least recently added item
to a successor.

Waits for the completion of the ``input`` in the Flow Graph, meaning all tasks created by each node and
related to ``input`` are executed, and no related objects remain in any buffer within the graph.

**Returns**: ``true``.

.. code:: cpp

    template <typename T, typename Compare>
    bool priority_queue_node<T>::try_put_and_wait(const T& input)

**Effects**: Adds ``input`` to the ``priority_queue_node`` and attempts to forward the item with the highest
priority among all items added to the node but not yet forwarded to the successors.

Waits for the completion of the ``input`` in the Flow Graph, meaning all tasks created by each node and
related to ``input`` are executed, and no related objects remain in any buffer within the graph.

**Returns**: ``true``.

.. code:: cpp

    template <typename T>
    bool sequencer_node<T>::try_put_and_wait(const T& input)

**Effects**: Adds ``input`` to the ``sequencer_node`` and tries forwarding the next item in sequence to a successor.

Waits for the completion of the ``input`` in the Flow Graph, meaning all tasks created by each node and
related to ``input`` are executed, and no related objects remain in any buffer within the graph.

**Returns**: ``true``.

.. code:: cpp

    template <typename T, typename DecrementType>
    bool limiter_node<T, DecrementType>::try_put_and_wait(const T& input)

**Effects**: If the broadcast count is below the threshold, broadcasts ``input`` to all successors.

Waits for the completion of the ``input`` in the Flow Graph, meaning all tasks created by each node and
related to ``input`` are executed, and no related objects remain in any buffer within the graph.

**Returns**: ``true`` if ``input`` is broadcasted; ``false`` otherwise.

.. code:: cpp

    template <typename T>
    bool broadcast_node<T>::try_put_and_wait(const T& input)

**Effects**: Broadcasts ``input`` to all successors.

Waits for the completion of the ``input`` in the Flow Graph, meaning all tasks created by each node and
related to ``input`` are executed, and no related objects remain in any buffer within the graph.

**Returns**: ``true`` even if the node cannot successfully forward the message to any of its successors.

.. code:: cpp

    template <typename TupleType>
    bool split_node<TupleType>::try_put_and_wait(const TupleType& input);

**Effects**: Broadcasts each element in the incoming tuple to the nodes connected to the ``split_node`` output ports.
The element at index ``i`` of ``input`` is broadcasted through the output port number ``i``.

Waits for the completion of the ``input`` in the Flow Graph, meaning all tasks created by each node and
related to ``input`` are executed, and no related objects remain in any buffer within the graph.

**Returns**: ``true``.

Example
*******

.. literalinclude:: ./examples/try_put_and_wait_example.cpp
    :language: c++
    :start-after: /*begin_try_put_and_wait_example*/
    :end-before: /*end_try_put_and_wait_example*/

Each iteration of ``parallel_for`` submits an input into the Flow Graph. After returning from ``try_put_and_wait(input)``, it is
guaranteed that all of the work related to the completion of ``input`` is done by all of the nodes in the graph. Tasks related to inputs
submitted by other calls are not guaranteed to be completed.
