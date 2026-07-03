.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============
Thread Safety
=============
**[thread_safety]**

Unless otherwise stated, the thread safety rules for the library are as follows:

* Two threads can invoke a method or function concurrently on different objects, but not the same object.
* It is unsafe for two threads to invoke concurrently methods or functions on the same object.

Departures from this convention are noted in the classes descriptions. For example, the concurrent containers are more liberal. By their nature, they do permit some concurrent operations on the same container object.