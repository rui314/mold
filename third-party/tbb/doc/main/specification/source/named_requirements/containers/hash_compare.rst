.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===========
HashCompare
===========
**[req.hash_compare]**

HashCompare is an object which is used to calculate hash code for an object and compare
two objects for equality.

The type ``H`` satisfies ``HashCompare`` if it meets the following requirements:

----------------------------------------------------------------

**HashCompare Requirements: Pseudo-Signature, Semantics**

.. cpp:function:: H::H( const H& )

    Copy constructor.

.. cpp:function:: H::~H()

    Destructor.

.. cpp:function:: std::size_t H::hash(const KeyType& k) const

    Calculates the hash for a provided key.

.. cpp:function:: ReturnType H::equal(const KeyType& k1, const KeyType& k2) const

    Requirements:

    * The type ``ReturnType`` should be implicitly convertible to ``bool``.

    Compares ``k1`` and ``k2`` for equality.

    If this function returns ``true``, ``H::hash(k1)`` should be equal to ``H::hash(k2)``.
