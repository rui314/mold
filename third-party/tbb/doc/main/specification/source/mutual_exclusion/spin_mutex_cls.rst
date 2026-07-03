.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==========
spin_mutex
==========
**[mutex.spin_mutex]**

A ``spin_mutex`` is a class that models the :doc:`Mutex requirement <../named_requirements/mutexes/mutex>` using a spin lock.
The ``spin_mutex`` class satisfies all requirements of mutex type from the [thread.mutex.requirements] ISO C++ section.
The ``spin_mutex`` class is not fair or recursive.

.. code:: cpp

    // Defined in header <oneapi/tbb/spin_mutex.h>

    namespace oneapi {
    namespace tbb {
        class spin_mutex {
        public:
            spin_mutex() noexcept;
            ~spin_mutex();

            spin_mutex(const spin_mutex&) = delete;
            spin_mutex& operator=(const spin_mutex&) = delete;

            class scoped_lock;

            void lock();
            bool try_lock();
            void unlock();

            static constexpr bool is_rw_mutex = false;
            static constexpr bool is_recursive_mutex = false;
            static constexpr bool is_fair_mutex = false;
        };
    } // namespace tbb
    } // namespace oneapi 

Member classes
--------------

.. namespace:: oneapi::tbb::spin_mutex
	       
.. cpp:class:: scoped_lock

    Corresponding ``scoped_lock`` class. See the :doc:`Mutex requirement <../named_requirements/mutexes/mutex>`.

Member functions
----------------

.. cpp:function:: spin_mutex()

    Constructs ``spin_mutex`` with unlocked state.

.. cpp:function:: ~spin_mutex()

    Destroys an unlocked ``spin_mutex``.

.. cpp:function:: void lock()

    Acquires a lock. Spins if the lock is taken.

.. cpp:function:: bool try_lock()

    Attempts to acquire a lock (non-blocking). Returns **true** if lock is acquired; **false**, otherwise.

.. cpp:function:: void unlock()

    Releases a lock held by a current thread.

