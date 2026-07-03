.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

======================
speculative_spin_mutex
======================
**[mutex.speculative_spin_mutex]**

A ``speculative_spin_mutex`` is a class that models the :doc:`Mutex requirement <../named_requirements/mutexes/mutex>` using a spin lock,
and for processors that support hardware transactional memory (such as Intel® Transactional Synchronization Extensions (Intel® TSX))
may be implemented in a way that allows non-contending changes to the protected data to proceed in parallel.

The ``speculative_spin_mutex`` is not fair and not recursive.
The ``speculative_spin_mutex`` is like a ``spin_mutex``, but it may provide better throughput than
non-speculative mutexes when the following conditions are met:

* Running on a processor that supports hardware transactional memory.
* Multiple threads can concurrently execute the critical section(s) protected by the mutex, mostly without conflicting.

Otherwise, it performs like a ``spin_mutex``, possibly with worse throughput.

.. code:: cpp

    // Defined in header <oneapi/tbb/spin_mutex.h>

    namespace oneapi {
    namespace tbb {
        class speculative_spin_mutex {
        public:
            speculative_spin_mutex() noexcept;
            ~speculative_spin_mutex();

            speculative_spin_mutex(const speculative_spin_mutex&) = delete;
            speculative_spin_mutex& operator=(const speculative_spin_mutex&) = delete;

            class scoped_lock;

            static constexpr bool is_rw_mutex = false;
            static constexpr bool is_recursive_mutex = false;
            static constexpr bool is_fair_mutex = false;
        };
    } // namespace tbb
    } // namespace oneapi

Member classes
--------------

.. namespace:: oneapi::tbb::speculative_spin_mutex
	       
.. cpp:class:: scoped_lock

    Corresponding ``scoped_lock`` class. See the :doc:`Mutex requirement <../named_requirements/mutexes/mutex>`.

Member functions
----------------

.. cpp:function:: speculative_spin_mutex()

    Constructs ``speculative_spin_mutex`` with unlocked state.

.. cpp:function:: ~speculative_spin_mutex()

    Destroys an unlocked ``speculative_spin_mutex``.

