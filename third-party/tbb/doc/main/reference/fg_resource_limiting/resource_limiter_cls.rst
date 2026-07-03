.. _resource_limiter_cls:

``resource_limiter`` Class
==========================

Description
***********

The class ``resource_limiter<ResourceHandle>`` represents a *Provider* that manages one or more
resource handles of type ``ResourceHandle``.

It provides exclusive access to managed resources to the consumers -- ``resource_limited_node`` instances.

For some resource types, the ``ResourceHandle`` represents the resource itself. For other resource types, the
``ResourceHandle`` may represent a lightweight entity used to access the resource.

.. code:: cpp

    tbb::flow::resource_limiter<int> int_limiter{1, 2, 3};

    using db_resource_handle = std::unique_ptr<Database, CloseDatabase>;
    tbb::flow::resource_limiter<db_resource_handle> db_limiter{open_database()};

In the example above, ``int_limiter`` manages three resources of type ``int``, and ``db_limiter`` manages a single
handle to a resource of type ``Database``.

All the resource handles managed by the ``resource_limiter`` are considered equivalent, and the order in which the access
to resources is granted to consumers is unspecified.

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

                template <typename ResourceHandle>
                class resource_limiter {
                public:
                    using resource_handle_type = ResourceHandle;

                    template <typename Handle, typename... Handles>
                    resource_limiter(Handle&& handle, Handles&&... handles);

                    ~resource_limiter();
                }; // class resource_limiter

            } // namespace flow
        } // namespace tbb
    } // namespace oneapi

Requirements
------------

``ResourceHandle`` type must meet the ``MoveConstructible`` requirements from [moveconstructible]
and the ``MoveAssignable`` requirements from [moveassignable] sections of the ISO C++ Standard.

Member Types
------------

.. code:: cpp

    using resource_handle_type = ResourceHandle;

An alias to the resource handle type.

Member Functions
----------------

.. code:: cpp

    template <typename Handle, typename... Handles>
    resource_limiter(Handle&& handle, Handles&&... handles);

**Requirements**: ``ResourceHandle`` type must be constructible from ``std::forward<Handle>(handle)``,
and from ``std::forward<H>(h)`` for each ``H`` in ``Handles`` and for each ``h`` in ``handles``.

Constructs a ``resource_limiter`` that manages at least one resource.

Each resource is constructed from the corresponding argument in ``handle`` or ``handles``.

------------------------------------------------------

.. code:: cpp

    ~resource_limiter();

Destroys the ``resource_limiter``. 

If there are consumers that still reference the limiter, the behavior is undefined.
