.. _custom_mutex_chmap:

The customizing mutex type for ``concurrent_hash_map``
======================================================

.. note::
    To enable this feature, define the ``TBB_PREVIEW_CONCURRENT_HASH_MAP_EXTENSIONS`` macro to 1.

.. contents::
    :local:
    :depth: 1

Description
***********

oneTBB ``concurrnent_hash_map`` class uses reader-writer mutex
to provide thread safety and avoid data races for insert, lookup, and erasure operations. This feature adds an extra template parameter
for ``concurrent_hash_map`` that allows to customize the type of the reader-writer mutex.

API
***

Header
------

.. code:: cpp

    #include <oneapi/tbb/concurrent_hash_map.h>

Synopsis
--------

.. code:: cpp

    namespace oneapi {
    namespace tbb {

        template <typename Key, typename T,
                typename HashCompare = tbb_hash_compare<Key>,
                typename Allocator = tbb_allocator<std::pair<const Key, T>>,
                typename Mutex = spin_rw_mutex>
        class concurrent_hash_map {
            using mutex_type = Mutex;
        };

    } // namespace tbb
    } // namespace oneapi

Type requirements
-----------------

The type of the mutex passed as a template argument for ``concurrent_hash_map`` should
meet the requirements of `ReaderWriterMutex <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/named_requirements/mutexes/rw_mutex.html>`_.
It should also provide the following API:

.. cpp:function:: bool ReaderWriterMutex::scoped_lock::is_writer() const;

**Returns**: ``true`` if the ``scoped_lock`` object acquires the mutex as a writer, ``false`` otherwise.

The behavior is undefined if the ``scoped_lock`` object does not acquire the mutex.

``oneapi::tbb::spin_rw_mutex``, ``oneapi::tbb::speculative_spin_rw_mutex``, ``oneapi::tbb::queuing_rw_mutex``, ``oneapi::tbb::null_rw_mutex``,
and ``oneapi::tbb::rw_mutex`` meet the requirements above.

.. rubric:: Example

The example below demonstrates how to wrap ``std::shared_mutex`` (C++17) to meet the requirements
of `ReaderWriterMutex` and how to customize ``concurrent_hash_map`` to use this mutex.

.. code:: cpp

    #define TBB_PREVIEW_CONCURRENT_HASH_MAP_EXTENSIONS 1
    #include "oneapi/tbb/concurrent_hash_map.h"
    #include <shared_mutex>

    class SharedMutexWrapper {
    public:
        // ReaderWriterMutex requirements

        static constexpr bool is_rw_mutex = true;
        static constexpr bool is_recursive_mutex = false;
        static constexpr bool is_fair_mutex = false;

        class scoped_lock {
        public:
            scoped_lock() : my_mutex_ptr(nullptr), my_writer_flag(false) {}
            scoped_lock(SharedMutexWrapper& mutex, bool write = true)
                : my_mutex_ptr(&mutex), my_writer_flag(write)
            {
                if (my_writer_flag) {
                    my_mutex_ptr->my_mutex.lock();
                } else {
                    my_mutex_ptr->my_mutex.lock_shared();
                }
            }

            ~scoped_lock() {
                if (my_mutex_ptr) release();
            }

            void acquire(SharedMutexWrapper& mutex, bool write = true) {
                if (my_mutex_ptr) release();

                my_mutex_ptr = &mutex;
                my_writer_flag = write;

                if (my_writer_flag) {
                    my_mutex_ptr->my_mutex.lock();
                } else {
                    my_mutex_ptr->my_mutex.lock_shared();
                }
            }

            void release() {
                if (my_writer_flag) {
                    my_mutex_ptr->my_mutex.unlock();
                } else {
                    my_mutex_ptr->my_mutex.unlock_shared();
                }
            }

            bool upgrade_to_writer() {
                // std::shared_mutex does not have the upgrade/downgrade parallel_for_each_semantics
                if (my_writer_flag) return true; // Already a writer

                my_mutex_ptr->my_mutex.unlock_shared();
                my_mutex_ptr->my_mutex.lock();
                return false; // The lock was reacquired
            }

            bool downgrade_to_reader() {
                if (!my_writer_flag) return true; // Already a reader

                my_mutex_ptr->my_mutex.unlock();
                my_mutex_ptr->my_mutex.lock_shared();
                return false;
            }

            bool is_writer() const {
                return my_writer_flag;
            }

        private:
            SharedMutexWrapper* my_mutex_ptr;
            bool                my_writer_flag;
        };
    private:
        std::shared_mutex my_mutex;
    }; // struct SharedMutexWrapper

    int main() {
        using map_type = oneapi::tbb::concurrent_hash_map<int, int,
                                                          oneapi::tbb::tbb_hash_compare<int>,
                                                          oneapi::tbb::tbb_allocator<std::pair<const int, int>>,
                                                          SharedMutexWrapper>;

        map_type map; // This object will use SharedMutexWrapper for thread safety of insert/find/erase operations
    }
