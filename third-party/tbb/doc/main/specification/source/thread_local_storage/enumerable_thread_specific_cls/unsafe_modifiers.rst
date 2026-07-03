.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============================
Concurrently unsafe modifiers
=============================

All member functions in this section can only be performed serially. The behavior is undefined in
case of concurrent execution of these methods with other (either concurrently safe) methods.

clear
-----

.. code:: cpp

    void clear();

Destroys all elements in ``*this``.

