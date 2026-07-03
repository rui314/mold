.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=================
ReaderWriterMutex
=================
**[req.rw_mutex]**

The `ReaderWriterMutex` requirement extends the :doc:`Mutex Requirement <mutex>` to include the notion of reader-writer locks.
It introduces a boolean parameter ``write`` that specifies whether a writer lock (``write = true``) or reader lock (``write = false``) is being requested.
Multiple reader locks can be held simultaneously on a `ReaderWriterMutex` if it does not have a writer lock on it.
A writer lock on a `ReaderWriterMutex` excludes all other threads from holding a lock on the mutex at the same time.

.. code:: cpp

    class RWM {
        // Implementation specifics
        // ...

        // Represents acquisition of a mutex.
        class scoped_lock {
        public:
            constexpr scoped_lock() noexcept;
            scoped_lock(RWM& m, bool write = true);
            ~scoped_lock();

            scoped_lock(const scoped_lock&) = delete;
            scoped_lock& operator=(const scoped_lock&) = delete;

            void acquire(RWM& m, bool write = true);
            bool try_acquire(RWM& m, bool write = true);
            void release();

            bool upgrade_to_writer();
            bool downgrade_to_reader();
        };
    };

A type `RWM` satisfies `ReaderWriterMutex` if it meets the following requirements.
They form a superset of the :doc:`Mutex requirements <mutex>`.

.. cpp:type:: RWM::scoped_lock

    Corresponding scoped-lock type.

.. namespace:: RWM::scoped_lock
	       
.. cpp:function:: RWM::scoped_lock()

    Constructs ``scoped_lock`` without acquiring any mutex.

.. cpp:function:: RWM::scoped_lock(RWM&, bool write = true)

    Constructs ``scoped_lock`` and acquires a lock on a given mutex. The lock is a writer lock if ``write`` is true; a reader lock otherwise.

.. cpp:function:: RWM::~scoped_lock()

    Releases a lock (if acquired).

.. cpp:function:: void RWM::scoped_lock::acquire(RWM&, bool write = true)

    Acquires a lock on a given mutex. The lock is a writer lock if ``write`` is true; it is a reader lock, otherwise.

.. cpp:function:: bool RWM::scoped_lock::try_acquire(RWM&, bool write = true)

    Attempts to acquire a lock on a given mutex. The lock is a writer lock if ``write`` is true; it is a reader lock, otherwise.
    Returns ``true`` if the lock is acquired, ``false`` otherwise.

.. cpp:function:: RWM::scoped_lock::release()

    Releases a lock. The effect is undefined if no lock is held.

.. cpp:function:: bool RWM::scoped_lock::upgrade_to_writer()

    Changes a reader lock to a writer lock. Returns ``false`` if lock was released and reacquired. Otherwise, returns ``true``, including the case when the lock was already a writer lock.

.. cpp:function:: bool RWM::scoped_lock::downgrade_to_reader()

    Changes a writer lock to a reader lock. Returns ``false`` if lock was released and reacquired. Otherwise, returns ``true``, including the case when the lock was already a reader lock.

Like the `Mutex` requirement, `ReaderWriterMutex` also requires a set of traits to be defined.

.. cpp:member:: static constexpr bool M::is_rw_mutex

    True if mutex is a reader-writer mutex; false, otherwise.

.. cpp:member:: static constexpr bool M::is_recursive_mutex

    True if mutex is a recursive mutex; false, otherwise.

.. cpp:member:: static constexpr bool M::is_fair_mutex

    True if mutex is fair; false, otherwise.

The following table summarizes the library classes that model the `ReaderWriterMutex` requirement and provided guarantees.

.. table:: Provided guarantees for Mutexes that model the ReaderWriterMutex requirement

    ============================= ============ =============
    .                             **Fair**     **Reentrant**
    ============================= ============ =============
    ``rw_mutex``                  No           No
    ----------------------------- ------------ -------------
    ``spin_rw_mutex``             No           No
    ----------------------------- ------------ -------------
    ``speculative_spin_rw_mutex`` No           No
    ----------------------------- ------------ -------------
    ``queuing_rw_mutex``          Yes          No
    ----------------------------- ------------ -------------
    ``null_rw_mutex``             Yes          Yes
    ============================= ============ =============

.. note::

    Implementation is allowed to have an opposite guarantees (positive) in case of negative statements from the table above.

.. note::

    For all currently provided reader-writer mutexes,

    * ``is_recursive_mutex`` is ``false``
    * ``scoped_lock::downgrade_to_reader`` always returns ``true``

    However, other implementations of the ReaderWriterMutex requirement are not required to do the same.

See also:

* :doc:`rw_mutex <../../mutual_exclusion/rw_mutex_cls>`
* :doc:`spin_rw_mutex <../../mutual_exclusion/spin_rw_mutex_cls>`
* :doc:`speculative_spin_rw_mutex <../../mutual_exclusion/speculative_spin_rw_mutex_cls>`
* :doc:`queuing_rw_mutex <../../mutual_exclusion/queuing_rw_mutex_cls>`
* :doc:`null_rw_mutex <../../mutual_exclusion/null_rw_mutex_cls>`

