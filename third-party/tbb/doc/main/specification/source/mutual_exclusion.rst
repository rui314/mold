.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

================
Mutual Exclusion
================
**[mutex]**

The library provides a set of mutual exclusion primitives to simplify writing race-free code.
A mutex object facilitates protection against data races and provides safe synchronization of data between threads.

Mutex Classes
-------------

.. toctree::
    :titlesonly:

    mutual_exclusion/mutex_cls.rst
    mutual_exclusion/rw_mutex_cls.rst
    mutual_exclusion/spin_mutex_cls.rst
    mutual_exclusion/spin_rw_mutex_cls.rst
    mutual_exclusion/speculative_spin_mutex_cls.rst
    mutual_exclusion/speculative_spin_rw_mutex_cls.rst
    mutual_exclusion/queuing_mutex_cls.rst
    mutual_exclusion/queuing_rw_mutex_cls.rst
    mutual_exclusion/null_mutex_cls.rst
    mutual_exclusion/null_rw_mutex_cls.rst

