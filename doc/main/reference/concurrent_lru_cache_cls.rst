.. _concurrent_lru_cache:

concurrent_lru_cache
====================

.. note::
   To enable this feature, define the ``TBB_PREVIEW_CONCURRENT_LRU_CACHE`` macro to 1.

A Class Template for Least Recently Used cache with concurrent operations.

.. contents::
    :local:
    :depth: 1

Description
***********

A ``concurrent_lru_cache`` container maps keys to values with the ability
to limit the number of stored unused values. For each key, there is at most one item
stored in the container.

The container permits multiple threads to concurrently retrieve items from it.

The container tracks which items are in use by returning a proxy
``concurrent_lru_cache::handle`` object that refers to an item instead of its value.
Once there are no ``handle`` objects holding reference to an item, it is considered unused.

The container stores all the items that are currently in use plus a limited
number of unused items. Excessive unused items are erased according to
least recently used policy.

When no item is found for a given key, the container calls the user-specified
``value_function_type`` object to construct a value for the key, and stores that value.
The ``value_function_type`` object must be thread-safe.

API
***

Header
------

.. code:: cpp

    #include "oneapi/tbb/concurrent_lru_cache.h"

Synopsis
--------

.. code:: cpp

    namespace oneapi {
        namespace tbb {
            template <typename Key, typename Value, typename ValueFunctionType = Value (*)(Key)>
            class concurrent_lru_cache {
            public:
                using key_type = Key;
                using value_type = Value;
                using pointer = value_type*;
                using const_pointer = const value_type*;
                using reference = value_type&;
                using const_reference = const value_type&;

                using value_function_type = ValueFunctionType;

                class handle {
                public:
                    handle();
                    handle( handle&& other );

                    ~handle();

                    handle& operator=( handle&& other );

                    operator bool() const;
                    value_type& value();
                }; // class handle

                concurrent_lru_cache( value_function_type f, std::size_t number_of_lru_history_items );
                ~concurrent_lru_cache();

                handle operator[]( key_type key );
            }; // class concurrent_lru_cache
        } // namespace tbb
    } // namespace oneapi

Member Functions
----------------

.. cpp:function:: concurrent_lru_cache( value_function_type f, std::size_t number_of_lru_history_items );

    **Effects**: Constructs an empty cache that can keep up to ``number_of_lru_history_items``
    unused values, with a function object ``f`` for constructing new values.

-------------------------------------------------------

.. cpp:function:: ~concurrent_lru_cache();

    **Effects**: Destroys the ``concurrent_lru_cache``. Calls the destructors of the stored elements and
    deallocates the used storage.

The behavior is undefined in case of concurrent operations with ``*this``.

-------------------------------------------------------

.. cpp:function:: handle operator[]( key_type k );

    **Effects**: Searches the container for an item that corresponds to the given key.
    If such an item is not found, the user-specified function object is called to
    construct a value that is inserted into the container.

    **Returns**: a ``handle`` object holding reference to the matching value.

Member Objects
--------------

``handle`` class
^^^^^^^^^^^^^^^^

**Member Functions**

.. cpp:function:: handle();

    **Effects**: Constructs a ``handle`` object that does not refer to any value.

--------------------------------------------------

.. cpp:function:: handle( handle&& other );

    **Effects**: Transfers the reference to the value stored in ``concurrent_lru_cache``
    from ``other`` to the newly constructed object. Upon completion,
    ``other`` no longer refers to any value.

---------------------------------------------------

.. cpp:function:: ~handle();

    **Effects**: Releases the reference (if it exists) to a value stored in ``concurrent_lru_cache``.

The behavior is undefined for concurrent operations with ``*this``.

---------------------------------------------------

.. cpp:function:: handle& operator=( handle&& other );

    **Effects**: Transfers the reference to a value stored in ``concurrent_lru_cache`` from ``other``
    to ``*this``. If existed, the previous reference held by ``*this`` is released. Upon
    completion ``other`` no longer refers to any value.

    **Returns**: a reference to ``*this``.

---------------------------------------------------

.. cpp:function:: operator bool() const;

    **Returns**: ``true`` if ``*this`` holds reference to a value, ``false`` otherwise.

---------------------------------------------------

.. cpp:function:: value_type& value();

    **Returns**: a reference to a ``value_type`` object stored in ``concurrent_lru_cache``.

The behavior is undefined if ``*this`` does not refer to any value.
