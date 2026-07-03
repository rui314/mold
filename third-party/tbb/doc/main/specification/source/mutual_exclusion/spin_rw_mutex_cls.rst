.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============
spin_rw_mutex
=============
**[mutex.spin_rw_mutex]**

A ``spin_rw_mutex`` is a class that models the :doc:`ReaderWriterMutex requirement <../named_requirements/mutexes/rw_mutex>`
and satisfies all requirements of shared mutex type from the [thread.sharedmutex.requirements] ISO C++ section.

The ``spin_rw_mutex`` class is unfair spinning reader-writer lock with backoff and writer-preference.

.. code:: cpp

    // Defined in header <oneapi/tbb/spin_rw_mutex.h>

    namespace oneapi {
    namespace tbb {
        class spin_rw_mutex {
        public:
            spin_rw_mutex() noexcept;
            ~spin_rw_mutex();

            spin_rw_mutex(const spin_rw_mutex&) = delete;
            spin_rw_mutex& operator=(const spin_rw_mutex&) = delete;

            class scoped_lock;

            // exclusive ownership
            void lock();
            bool try_lock();
            void unlock();

            // shared ownership
            void lock_shared();
            bool try_lock_shared();
            void unlock_shared();

            static constexpr bool is_rw_mutex = true;
            static constexpr bool is_recursive_mutex = false;
            static constexpr bool is_fair_mutex = false;
        };
    } // namespace tbb
    } // namespace oneapi

Member classes
--------------

.. namespace:: oneapi::tbb::spin_rw_mutex
	       
.. cpp:class:: scoped_lock

    Corresponding scoped-lock class. See the :doc:`ReaderWriterMutex requirement <../named_requirements/mutexes/rw_mutex>`.

Member functions
----------------

.. cpp:function:: spin_rw_mutex()

    Constructs unlocked ``spin_rw_mutex``.

.. cpp:function:: ~spin_rw_mutex()

    Destroys unlocked ``spin_rw_mutex``.

.. cpp:function:: void lock()

    Acquires a lock. Spins if the lock is taken.

.. cpp:function:: bool try_lock()

    Attempts to acquire a lock (non-blocking) on write. Returns true if the lock is acquired on write; false otherwise.

.. cpp:function:: void unlock()

    Releases a write lock, held by the current thread.

.. cpp:function:: void lock_shared()

    Acquires a lock on read. Spins if the lock is taken on write already.

.. cpp:function:: bool try_lock_shared()

    Attempts to acquire the lock (non-blocking) on read. Returns true if the lock is acquired on read; false, otherwise.

.. cpp:function:: void unlock_shared()

    Releases a read lock held by the current thread.

