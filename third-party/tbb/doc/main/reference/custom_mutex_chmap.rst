.. _custom_mutex_chmap:

The customizing mutex type for ``concurrent_hash_map``
======================================================

.. note::
    To enable this feature, define the ``TBB_PREVIEW_CONCURRENT_HASH_MAP_EXTENSIONS`` macro to 1.

.. contents::
    :local:
    :depth: 1

Description
***********

oneTBB ``concurrnent_hash_map`` class uses reader-writer mutex
to provide thread safety and avoid data races for insert, lookup, and erasure operations. This feature adds an extra template parameter
for ``concurrent_hash_map`` that allows to customize the type of the reader-writer mutex.

API
***

Header
------

.. code:: cpp

    #include <oneapi/tbb/concurrent_hash_map.h>

Synopsis
--------

.. code:: cpp

    namespace oneapi {
    namespace tbb {

        template <typename Key, typename T,
                typename HashCompare = tbb_hash_compare<Key>,
                typename Allocator = tbb_allocator<std::pair<const Key, T>>,
                typename Mutex = spin_rw_mutex>
        class concurrent_hash_map {
            using mutex_type = Mutex;
        };

    } // namespace tbb
    } // namespace oneapi

Type requirements
-----------------

The type of the mutex passed as a template argument for ``concurrent_hash_map`` should
meet the requirements of `ReaderWriterMutex <https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/named_requirements/mutexes/rw_mutex>`_.
It should also provide the following API:

.. cpp:function:: bool ReaderWriterMutex::scoped_lock::is_writer() const;

**Returns**: ``true`` if the ``scoped_lock`` object acquires the mutex as a writer, ``false`` otherwise.

The behavior is undefined if the ``scoped_lock`` object does not acquire the mutex.

``oneapi::tbb::spin_rw_mutex``, ``oneapi::tbb::speculative_spin_rw_mutex``, ``oneapi::tbb::queuing_rw_mutex``, ``oneapi::tbb::null_rw_mutex``,
and ``oneapi::tbb::rw_mutex`` meet the requirements above.

.. rubric:: Example

The example below demonstrates how to wrap ``std::shared_mutex`` (C++17) to meet the requirements
of `ReaderWriterMutex` and how to customize ``concurrent_hash_map`` to use this mutex.

.. literalinclude:: ./examples/custom_mutex_chmap_example.cpp
    :language: c++
    :start-after: /*begin_custom_mutex_chmap_example*/
    :end-before: /*end_custom_mutex_chmap_example*/
