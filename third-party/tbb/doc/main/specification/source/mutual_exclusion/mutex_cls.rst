.. SPDX-FileCopyrightText: 2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=====
mutex
=====
**[mutex.mutex]**

A ``mutex`` is a class that models :doc:`Mutex requirement <../named_requirements/mutexes/mutex>`
using an adaptive approach, it guarantees that the thread that cannot acquire the lock spins before blocking.
The ``mutex`` class satisfies all of the mutex requirements described in the [thread.mutex.requirements] section of the ISO C++ standard.
The ``mutex`` class is not fair or recursive.

.. code:: cpp

    // Defined in header <oneapi/tbb/mutex.h>

    namespace oneapi {
        namespace tbb {
            class mutex {
            public:
                mutex() noexcept;
                ~mutex();

                mutex(const mutex&) = delete;
                mutex& operator=(const mutex&) = delete;

                class scoped_lock;

                void lock();
                bool try_lock();
                void unlock();

                static constexpr bool is_rw_mutex = false;
                static constexpr bool is_recursive_mutex = false;
                static constexpr bool is_fair_mutex = false;
            };
        }
    }

Member classes
--------------

.. namespace:: oneapi::tbb::mutex
	       
.. cpp:class:: scoped_lock

    The corresponding ``scoped_lock`` class. See :doc:`Mutex requirement <../named_requirements/mutexes/mutex>`.

Member functions
----------------

.. cpp:function:: mutex()

    Constructs a ``mutex`` with the unlocked state.

--------------------------------------------------

.. cpp:function:: ~mutex()

    Destroys an unlocked ``mutex``.

--------------------------------------------------

.. cpp:function:: void lock()

    Acquires a lock. It uses an adaptive logic for waiting, thus it is blocked after a certain time of busy waiting.

--------------------------------------------------

.. cpp:function:: bool try_lock()

    Tries to acquire a lock (non-blocking). Returns **true** if succeeded; **false** otherwise.

--------------------------------------------------

.. cpp:function:: void unlock()

    Releases the lock held by a current thread.
