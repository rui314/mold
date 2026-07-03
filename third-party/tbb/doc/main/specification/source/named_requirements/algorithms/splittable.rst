.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==========
Splittable
==========
**[req.splittable]**

A type is splittable if it has a *splitting constructor* that allows an instance to be split into
two pieces. The splitting constructor takes as arguments a reference to the original object,
and a dummy argument of type ``split``, which is defined by the library. The dummy argument
distinguishes the splitting constructor from a copy constructor. After the constructor runs,
``x`` and the newly constructed object should represent the two pieces of the original
``x.`` The library uses splitting constructors in two contexts:

* *Partitioning* a range into two subranges that can be processed concurrently.
* *Forking* a body (function object) into two bodies that can run concurrently.

Types that meet the :doc:`Range requirements <range>` may additionally define an optional *proportional splitting constructor*,
distinguished by an argument of type :doc:`proportional_split Class <../../algorithms/split_tags/proportional_split_cls>`.

A type `X` satisfies `Splittable` if it meets the following requirements:

------------------------------------------------------

**Splittable Requirements: Pseudo-Signature, Semantics**

.. cpp:function:: X::X(X& x, split)

    Split ``x`` into ``x`` and newly constructed object.

See also:

* :doc:`Range requirements <range>`
