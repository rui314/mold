.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

==========
null_mutex
==========
**[mutex.null_mutex]**

A ``null_mutex`` is a class that models the :doc:`Mutex requirement <../named_requirements/mutexes/mutex>` concept syntactically, but does nothing.
It is useful for instantiating a template that expects a Mutex, but no mutual exclusion is actually needed for that instance.

.. code:: cpp

    // Defined in header <oneapi/tbb/null_mutex.h>

    namespace oneapi {
    namespace tbb {
        class null_mutex {
        public:
            constexpr null_mutex() noexcept;
            ~null_mutex();

            null_mutex(const null_mutex&) = delete;
            null_mutex& operator=(const null_mutex&) = delete;

            class scoped_lock;

            void lock();
            bool try_lock();
            void unlock();

            static constexpr bool is_rw_mutex = false;
            static constexpr bool is_recursive_mutex = true;
            static constexpr bool is_fair_mutex = true;
        };
    } // namespace tbb
    } // namespace oneapi

Member classes
--------------

.. cpp:class:: scoped_lock

    Corresponding ``scoped_lock`` class. See the :doc:`Mutex requirement <../named_requirements/mutexes/mutex>`.

Member functions
----------------

.. cpp:function:: null_mutex()

    Constructs unlocked mutex.

.. cpp:function:: ~null_mutex()

    Destroys unlocked mutex.

.. cpp:function:: void lock()

    Acquires lock.

.. cpp:function:: bool try_lock()

    Tries acquiring lock (non-blocking).

.. cpp:function:: void unlock()

    Releases the lock.

