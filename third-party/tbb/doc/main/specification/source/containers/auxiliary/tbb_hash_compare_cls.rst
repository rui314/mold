.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

================
tbb_hash_compare
================
**[containers.tbb_hash_compare]**

``oneapi::tbb::tbb_hash_compare`` is a class template for hash support. Use it with the ``oneapi::tbb::concurrent_hash_map``
associative container to calculate hash codes and compare keys for equality.

``tbb_hash_compare`` meets the :doc:`HashCompare requirements <../../named_requirements/containers/hash_compare>`.

Class Template Synopsis
-----------------------

.. code:: cpp

    // Defined in header <oneapi/tbb/concurrent_hash_map.h>

    namespace oneapi {
        namespace tbb {

            template <typename Key>
            class tbb_hash_compare {
                static std::size_t hash( const Key& k );
                static bool equal( const Key& k1, const Key& k2 );
            }; // class tbb_hash_compare

        } // namespace tbb
    } // namespace oneapi

Member functions
----------------

.. code:: cpp

    static std::size_t hash( const Key& k );

**Returns**: a hash code for a key ``k``.

---------------------

.. code:: cpp

    static bool equal( const Key& k1, const Key& k2 );

Equivalent to ``k1 == k2``.

**Returns**: ``true`` if the keys are equal; ``false``, otherwise.
