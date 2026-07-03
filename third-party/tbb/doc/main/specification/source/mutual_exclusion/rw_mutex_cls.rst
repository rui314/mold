.. SPDX-FileCopyrightText: 2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=============
rw_mutex
=============
**[mutex.rw_mutex]**

A ``rw_mutex`` is a class that models :doc:`ReaderWriterMutex requirement <../named_requirements/mutexes/rw_mutex>`
using an adaptive approach, it guarantees that the thread that cannot acquire the lock spins before blocking.
The ``rw_mutex`` class satisfies all of the shared mutex requirements described in the [thread.sharedmutex.requirements] section of the ISO C++ standard.
The ``rw_mutex`` class is an unfair reader-writer lock with a writer preference.

.. code:: cpp

    // Defined in header <oneapi/tbb/rw_mutex.h>

    namespace oneapi {
        namespace tbb {
            class rw_mutex {
            public:
                rw_mutex() noexcept;
                ~rw_mutex();

                rw_mutex(const rw_mutex&) = delete;
                rw_mutex& operator=(const rw_mutex&) = delete;

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
        }
    }

Member classes
--------------

.. namespace:: oneapi::tbb::rw_mutex
	       
.. cpp:class:: scoped_lock

    The corresponding scoped-lock class. See :doc:`ReaderWriterMutex requirement <../named_requirements/mutexes/rw_mutex>`.

Member functions
----------------

.. cpp:function:: rw_mutex()

    Constructs an unlocked ``rw_mutex``.

--------------------------------------------------

.. cpp:function:: ~rw_mutex()

    Destroys an unlocked ``rw_mutex``.

--------------------------------------------------

.. cpp:function:: void lock()

    Acquires a lock. It uses an adaptive logic for waiting, thus it is blocked after a certain time of busy waiting.

--------------------------------------------------

.. cpp:function:: bool try_lock()

    Tries to acquire a lock (non-blocking) on write. Returns **true** if succeeded; **false** otherwise.

--------------------------------------------------

.. cpp:function:: void unlock()

    Releases the write lock held by the current thread.

--------------------------------------------------

.. cpp:function:: void lock_shared()

    Acquires a lock on read. It uses an adaptive logic for waiting, thus it is blocked after a certain time of busy waiting.

--------------------------------------------------

.. cpp:function:: bool try_lock_shared()

    Tries to acquire the lock (non-blocking) on read. Returns **true** if succeeded; **false** otherwise.

--------------------------------------------------

.. cpp:function:: void unlock_shared()

    Releases the read lock held by the current thread.
