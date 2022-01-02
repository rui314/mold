.. _heterogeneous_extensions_chmap:

Heterogeneous overloads for ``concurrent_hash_map`` member functions
====================================================================

.. note::
    To enable this feature, define the ``TBB_PREVIEW_CONCURRENT_HASH_MAP_EXTENSIONS`` macro to 1.

A set of overloads for ``concurrent_hash_map`` member functions that allow to search, erase, and insert
elements into the container without creating a temporary ``key_type`` object.

.. contents::
    :local:
    :depth: 1

Description
***********

Heterogeneous overloads allow you to perform insert, lookup, and erasure operations on ``concurrent_hash_map`` object
using an object of the type that is different from ``key_type`` but comparable with it.

All member functions described below only participate in overload resolution if ``HashCompareType::is_transparent``
is valid and denotes a type.``HashCompareType`` is a type of the ``HashCompare`` passed as a template argument
for ``concurrent_hash_map``. It means that the ``HashCompare`` object calculates a hash and compares keys for
equality without creating a temporary ``key_type`` object.

API
***

Header
------

.. code:: cpp

    #include "oneapi/tbb/concurrent_hash_map.h"

Synopsis
--------

.. code:: cpp

    namespace oneapi {
        namespace tbb {
            template <typename Key, typename Mapped,
                      typename HashCompare = tbb_hash_compare<Key>,
                      typename Allocator = tbb_allocator<std::pair<const Key, Mapped>>>
            class concurrent_hash_map {
            public:
                // Insertion
                template <typename K>
                bool insert( accessor& result, const K& k );

                template <typename K>
                bool insert( const_accessor& result, const K& k );

                // Lookup
                template <typename K>
                bool find( accessor& result, const K& k );

                template <typename K>
                bool find( const_accessor& result, const K& k ) const;

                template <typename K>
                size_type count( const K& k ) const;

                template <typename K>
                std::pair<iterator, iterator> equal_range( const K& k );

                template <typename K>
                std::pair<const_iterator, const_iterator> equal_range( const K& k ) const;

                // Erasure
                template <typename K>
                bool erase( const K& k );
            };
        } // namespace tbb
    } // namespace oneapi

Member functions
----------------

Insertion
^^^^^^^^^

.. code:: cpp

    template <typename K>
    bool insert( accessor& result, const K& k );

    template <typename K>
    bool insert( const_accessor& result, const K& k );

If the accessor ``result`` is not empty, releases the ``result`` and tries to
insert the value constructed from ``{k, mapped_type()}`` into the container.

Sets the ``result`` to provide access to the inserted element or to the element with the key that
compares `equivalent` to the value ``k``.

This overload only participates in overload resolution if ``std::is_constructible<key_type, const K&>`` is ``true``.

**Returns**: ``true`` if the insertion was applied, ``false`` otherwise.

Lookup
^^^^^^

.. code:: cpp

    template <typename K>
    bool find( accessor& result, const K& k );

    template <typename K>
    bool find( const_accessor& result, const K& k ) const;

If the accessor ``result`` is not empty, releases the ``result``.

If an element with the key that compares `equivalent` to the value ``k`` exists,
sets the ``result`` to provide access to this element.

**Returns**: ``true`` if an element with the key that compares `equivalent` to the value ``k`` is found,
``false`` otherwise.

------------------------------------------------

.. code:: cpp

    template <typename K>
    size_type count( const K& k ) const;

**Returns**: ``1`` if an element with the key that compares `equivalent` to the value ``k`` exists, ``0`` otherwise.

------------------------------------------------

.. code:: cpp

    template <typename K>
    std::pair<iterator, iterator> equal_range( const K& k );

    template <typename K>
    std::pair<const_iterator, const_iterator> equal_range( const K& k ) const;

**Returns**:

- A pair of iterators ``{f, l}`` if an element with the key that compares `equivalent` to the value ``k`` exists in the container.
  Here ``f`` is an iterator to this element, ``l`` is ``std::next(f)``.
- ``{end(), end()}`` otherwise.

.. rubric:: Example

The example below demonstrates how to use heterogeneous lookup feature to find an object with the key of type ``std::string``
using an object of type ``const char*`` without conversions.

.. code:: cpp

    #define TBB_PREVIEW_CONCURRENT_HASH_MAP_EXTENSIONS 1
    #include "oneapi/tbb/concurrent_hash_map.h"
    #include <string>
    #include <cstring>

    // HashCompare an object that can calculate the hash code for
    // std::string only and compare strings for equality
    class RegularHashCompare {
    private:
        std::hash<std::string> my_hasher;
    public:
        std::size_t hash( const std::string& key ) const {
            return my_hasher(key);
        }

        bool equal( const std::string& key1, const std::string& key2 ) const {
            return key1 == key2;
        }
    };

    // HashCompare an object that can calculate the hash code for
    // std::string and const char*, and compare them for equality
    class TransparentHashCompare {
    private:
        std::hash<char> my_hasher;

        // Calculates a hash for the array of chars
        std::size_t calculate_hash( const char* ptr ) const {
            std::size_t h = 0;
            for (auto c = ptr; *c; ++c) {
                h = h ^ my_hasher(*c);
            }
            return h;
        }
    public:
        using is_transparent = void;

        std::size_t hash( const char* key ) const {
            return calculate_hash(key);
        }

        std::size_t hash( const std::string& key ) const {
            return calculate_hash(key.c_str());
        }

        bool equal( const char* key1, const char* key2 ) const {
            return std::strcmp(key1, key2) == 0;
        }

        bool equal( const char* key1, const std::string& key2 ) const {
            return std::strcmp(key1, key2.c_str()) == 0;
        }

        bool equal( const std::string& key1, const char* key2 ) const {
            return std::strcmp(key1.c_str(), key2) == 0;
        }

        bool equal( const std::string& key1, const std::string& key2 ) const {
            return std::strcmp(key1.c_str(), key2.c_str()) == 0;
        }
    };

    int main() {
        using regular_hash_map =
            oneapi::tbb::concurrent_hash_map<std::string, int, RegularHashCompare>;
        using transparent_hash_map =
            oneapi::tbb::concurrent_hash_map<std::string, int, TransparentHashCompare>;

        using regular_accessor = typename regular_hash_map::accessor;
        using transparent_accessor = typename transparent_hash_map::accessor;

        // Accessors
        regular_accessor reg_accessor;
        transparent_accessor tran_accessor;

        // Maps
        regular_hash_map regular_map;
        transparent_hash_map tran_map;

        // Heterogeneous overloads do not participate in overload resolution
        // Such a call matches on the find overload, which accepts key_type (std::string)
        // Creates a temporary key_type (std::string) object because of implicit conversion
        bool result = regular_map.find(reg_accessor, "abc");

        // Heterogeneous overloads participate in overload resolution
        // No implicit conversion from const char* to std::string takes place
        result = tran_map.find(tran_accessor, "abc");
    }
