.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=========================
speculative_spin_rw_mutex
=========================
**[mutex.speculative_spin_rw_mutex]**

A ``speculative_spin_rw_mutex`` is a class that models the :doc:`ReaderWriterMutex requirement <../named_requirements/mutexes/rw_mutex>`,
and for processors that support hardware transactional memory (such as Intel® Transactional Synchronization Extensions (Intel® TSX))
may be implemented in a way that allows non-contending changes to the protected data to proceed in parallel.

The ``speculative_spin_rw_mutex`` class is not fair and not recursive.
The ``speculative_spin_rw_mutex`` class is like a ``spin_rw_mutex``, but it may provide better throughput than
non-speculative mutexes when the following conditions are met:

* Running on a processor that supports hardware transactional memory.
* Multiple threads can concurrently execute the critical section(s) protected by the mutex, mostly without conflicting.

Otherwise, it performs like a ``spin_rw_mutex``, possibly with worse throughput.

For processors that support hardware transactional memory, ``speculative_spin_rw_mutex`` may be implemented in a way that

* speculative readers and writers do not block each other
* a non-speculative reader blocks writers but allows speculative readers
* a non-speculative writer blocks all readers and writers

.. code:: cpp

    // Defined in header <oneapi/tbb/spin_rw_mutex.h>

    namespace oneapi {
    namespace tbb {
        class speculative_spin_rw_mutex {
        public:
            speculative_spin_rw_mutex() noexcept;
            ~speculative_spin_rw_mutex();

            speculative_spin_rw_mutex(const speculative_spin_rw_mutex&) = delete;
            speculative_spin_rw_mutex& operator=(const speculative_spin_rw_mutex&) = delete;

            class scoped_lock;

            static constexpr bool is_rw_mutex = true;
            static constexpr bool is_recursive_mutex = false;
            static constexpr bool is_fair_mutex = false;
        };
    } // namespace tbb
    } // namespace oneapi 

Member classes
--------------

.. namespace:: oneapi::tbb::speculative_spin_rw_mutex
	       
.. cpp:class:: scoped_lock

    Corresponding ``scoped_lock`` class. See the :doc:`ReaderWriterMutex requirement <../named_requirements/mutexes/rw_mutex>`.

Member functions
----------------

.. cpp:function:: speculative_spin_rw_mutex()

    Constructs ``speculative_spin_rw_mutex`` with unlocked state.

.. cpp:function:: ~speculative_spin_rw_mutex()

    Destroys an unlocked ``speculative_spin_rw_mutex``.

