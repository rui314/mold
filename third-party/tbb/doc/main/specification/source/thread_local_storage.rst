.. SPDX-FileCopyrightText: 2019-2020 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

====================
Thread Local Storage
====================
**[thread_local_storage]**

oneAPI Threading Building Blocks provides class templates for thread local storage (TLS).
Each provides a thread-local element per thread and lazily creates elements on demand.

.. toctree::
    :titlesonly:
    :maxdepth: 1

    thread_local_storage/combinable_cls.rst
    thread_local_storage/enumerable_thread_specific_cls.rst

This section also describes class template ``flatten2d``, which assists a common idiom where
an ``enumerable_thread_specific`` represents a container partitioner across threads.

.. toctree::
    :titlesonly:

    thread_local_storage/flattened2d_cls.rst

