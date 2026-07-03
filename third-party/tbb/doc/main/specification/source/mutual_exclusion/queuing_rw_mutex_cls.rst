.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

================
queuing_rw_mutex
================
**[mutex.queuing_rw_mutex]**

A ``queuing_rw_mutex`` is a class that models the :doc:`ReaderWriterMutex requirement <../named_requirements/mutexes/rw_mutex>` concept.
The ``queuing_rw_mutex`` is not recursive. The ``queuing_rw_mutex`` is fair, threads acquire a lock on a mutex in the order that they request it.

.. code:: cpp

    // Defined in header <oneapi/tbb/queuing_rw_mutex.h>

    namespace oneapi {
    namespace tbb {
        class queuing_rw_mutex {
        public:
            queuing_rw_mutex() noexcept;
            ~queuing_rw_mutex();

            queuing_rw_mutex(const queuing_rw_mutex&) = delete;
            queuing_rw_mutex& operator=(const queuing_rw_mutex&) = delete;

            class scoped_lock;

            static constexpr bool is_rw_mutex = true;
            static constexpr bool is_recursive_mutex = false;
            static constexpr bool is_fair_mutex = true;
        };
    } // namespace tbb
    } // namespace oneapi

Member classes
--------------

.. namespace:: oneapi::tbb::queueing_rw_mutex
	       
.. cpp:class:: scoped_lock

    Corresponding ``scoped_lock`` class. See the :doc:`ReaderWriterMutex requirement <../named_requirements/mutexes/rw_mutex>`.

Member functions
----------------

.. cpp:function:: queuing_rw_mutex()

    Constructs unlocked ``queuing_rw_mutex``.

.. cpp:function:: ~queuing_rw_mutex()

    Destroys unlocked ``queuing_rw_mutex``.

