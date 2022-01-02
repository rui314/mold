.. _mutex:

mutex
==========

.. note::
   To enable this feature, define the ``TBB_PREVIEW_MUTEXES`` macro to 1.

Description
***********

A ``mutex`` is a class that models the `Mutex requirement <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/named_requirements/mutexes/mutex.html>`_,
using adaptive approach: the combination of spinlock and waiting on system primitives.
The ``mutex`` class satisfies all of the mutex requirements described in the [thread.mutex.requirements] section of the ISO C++ standard.
The ``mutex`` class is not fair or recursive.

API
***

Header
------

.. code:: cpp

    #include <oneapi/tbb/mutex.h>

Synopsis
--------

.. code:: cpp

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

.. namespace:: tbb::mutex
	       
.. cpp:class:: scoped_lock

    The corresponding ``scoped_lock`` class. See the `Mutex requirement <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/named_requirements/mutexes/mutex.html>`_.

Member functions
----------------

.. cpp:function:: mutex()

    Constructs ``mutex`` with unlocked state.

--------------------------------------------------

.. cpp:function:: ~mutex()

    Destroys an unlocked ``mutex``.

--------------------------------------------------

.. cpp:function:: void lock()

    Acquires a lock. It uses adaptive logic for waiting: it blocks after particular time period of busy wait.

--------------------------------------------------

.. cpp:function:: bool try_lock()

    Tries to acquire a lock (non-blocking). Returns **true** if succeeded; **false** otherwise.

--------------------------------------------------

.. cpp:function:: void unlock()

    Releases the lock held by a current thread.
