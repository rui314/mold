/*
    Copyright (c) 2025 Intel Corporation

    Licensed under the Apache License, Version 2.0 (the "License");
    you may not use this file except in compliance with the License.
    You may obtain a copy of the License at

        http://www.apache.org/licenses/LICENSE-2.0

    Unless required by applicable law or agreed to in writing, software
    distributed under the License is distributed on an "AS IS" BASIS,
    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
    See the License for the specific language governing permissions and
    limitations under the License.
*/

#if __cplusplus >= 201703L

/*begin_custom_mutex_chmap_example*/
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

        bool try_acquire(SharedMutexWrapper& mutex, bool write = true) {
            if (my_mutex_ptr) release();

            my_mutex_ptr = &mutex;

            bool result = false;

            if (my_writer_flag) {
                result = my_mutex_ptr->my_mutex.try_lock();
            } else {
                result = my_mutex_ptr->my_mutex.try_lock_shared();
            }

            if (result) my_writer_flag = write;
            return result;
        }

        void release() {
            if (my_writer_flag) {
                my_mutex_ptr->my_mutex.unlock();
            } else {
                my_mutex_ptr->my_mutex.unlock_shared();
            }
        }

        bool upgrade_to_writer() {
            // std::shared_mutex does not have the upgrade/downgrade semantics
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
/*end_custom_mutex_chmap_example*/

#else // C++17
// Skip
int main() {}
#endif
