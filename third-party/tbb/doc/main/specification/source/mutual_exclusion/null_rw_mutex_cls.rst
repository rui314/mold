.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============
null_rw_mutex
=============
**[mutex.null_rw_mutex]**

A ``null_rw_mutex`` is a class that models the :doc:`ReaderWriterMutex requirement <../named_requirements/mutexes/rw_mutex>` syntactically, but does nothing.
The ``null_rw_mutex`` class also satisfies all syntactic requirements of shared mutex type from the [thread.sharedmutex.requirements] ISO C++ section, but does nothing.
It is useful for instantiating a template that expects a ReaderWriterMutex, but no mutual exclusion is actually needed for that instance.

.. code:: cpp

    // Defined in header <oneapi/tbb/null_rw_mutex.h>

    namespace oneapi {
    namespace tbb {
        class null_rw_mutex {
        public:
            constexpr null_rw_mutex() noexcept;
            ~null_rw_mutex();

            null_rw_mutex(const null_rw_mutex&) = delete;
            null_rw_mutex& operator=(const null_rw_mutex&) = delete;

            class scoped_lock;

            void lock();
            bool try_lock();
            void unlock();

            void lock_shared();
            bool try_lock_shared();
            void unlock_shared();

            static constexpr bool is_rw_mutex = true;
            static constexpr bool is_recursive_mutex = true;
            static constexpr bool is_fair_mutex = true;
        };
    } // namespace tbb
    } // namespace oneapi

Member classes
--------------

.. namespace:: oneapi::tbb::null_rw_mutex

.. cpp:class:: scoped_lock

    Corresponding ``scoped_lock`` class. See the :doc:`ReaderWriterMutex requirement <../named_requirements/mutexes/rw_mutex>`.

Member functions
----------------

.. cpp:function:: null_rw_mutex()

    Constructs unlocked mutex.

.. cpp:function:: ~null_rw_mutex()

    Destroys unlocked mutex.

.. cpp:function:: void lock()

    Acquires a lock.

.. cpp:function:: bool try_lock()

    Attempts to acquire a lock (non-blocking) on write. Returns **true**.

.. cpp:function:: void unlock()

    Releases a write lock held by the current thread.

.. cpp:function:: void lock_shared()

    Acquires a lock on read.

.. cpp:function:: bool try_lock_shared()

    Attempts to acquire the lock (non-blocking) on read. Returns **true**.

.. cpp:function:: void unlock_shared()

    Releases a read lock held by the current thread.

