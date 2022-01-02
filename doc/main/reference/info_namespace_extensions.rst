.. _info_namespace_extensions:

oneapi::tbb::info namespace extensions
======================================

.. note::
    To enable this feature, set the ``TBB_PREVIEW_TASK_ARENA_CONSTRAINTS_EXTENSION`` macro to 1.

.. contents::
    :local:
    :depth: 1

Description
***********

These extensions allow to query information about execution environment.

.. contents::
    :local:
    :depth: 1

API
***

Header
------

.. code:: cpp

    #include <oneapi/tbb/info.h>

Syntax
------

.. code:: cpp

    namespace oneapi {
        namespace tbb {
            using core_type_id = /*implementation-defined*/;
            namespace info {
                std::vector<core_type_id> core_types();
                int default_concurrency(task_arena::constraints c);
            }
        }
    }

Types
-----

``core_type_id`` - Represents core type identifier.

Functions
---------

.. cpp:function:: std::vector<core_type_id> core_types()

    Returns the vector of integral indexes that indicate available core types.
    The indexes are sorted from the least performant to the most performant core type.

    .. note::
        If error occurs during system topology parsing, returns vector containing single element
        that equals to ``task_arena::automatic``.

.. cpp:function:: int default_concurrency(task_arena::constraints c)

    Returns concurrency level for the given constraints.

See also:

* :doc:`task_arena::constraints class preview extensions <constraints_extensions>`
* `info namespace specification <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/info_namespace.html>`_
