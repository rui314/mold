.. _rw_mutex:

rw_mutex
=============

.. note::
   To enable this feature, define the ``TBB_PREVIEW_MUTEXES`` macro to 1.

Description
***********

A ``rw_mutex`` is a class that models the `ReaderWriterMutex requirement <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/named_requirements/mutexes/rw_mutex.html>`_,
using adaptive approach: the combination of spinlock and waiting on system primitives.
The ``rw_mutex`` class satisfies all of the shared mutex requirements described in the [thread.sharedmutex.requirements] section of the ISO C++ standard.
The ``rw_mutex`` class is unfair reader-writer lock with writer-preference.

API
***

Header
------

.. code:: cpp

    #include <oneapi/tbb/rw_mutex.h>

Synopsis
--------
.. code:: cpp

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

.. namespace:: tbb::rw_mutex
	       
.. cpp:class:: scoped_lock

    The corresponding scoped-lock class. See the `ReaderWriterMutex requirement <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/named_requirements/mutexes/rw_mutex.html>`_.

Member functions
----------------

.. cpp:function:: rw_mutex()

    Constructs unlocked ``rw_mutex``.

--------------------------------------------------

.. cpp:function:: ~rw_mutex()

    Destroys unlocked ``rw_mutex``.

--------------------------------------------------

.. cpp:function:: void lock()

    Acquires a lock. It uses adaptive logic for waiting: it blocks after particular time period of busy wait.

--------------------------------------------------

.. cpp:function:: bool try_lock()

    Tries to acquire a lock (non-blocking) on write. Returns **true** if succeeded; **false** otherwise.

--------------------------------------------------

.. cpp:function:: void unlock()

    Releases the write lock held by the current thread.

--------------------------------------------------

.. cpp:function:: void lock_shared()

    Acquires a lock on read. It uses adaptive logic for waiting: it blocks after particular time period of busy wait.

--------------------------------------------------

.. cpp:function:: bool try_lock_shared()

    Tries to acquire the lock (non-blocking) on read. Returns **true** if succeeded; **false** otherwise.

--------------------------------------------------

.. cpp:function:: void unlock_shared()

    Releases the read lock held by the current thread.
