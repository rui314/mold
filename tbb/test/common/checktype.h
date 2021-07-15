/*
    Copyright (c) 2005-2021 Intel Corporation

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

#ifndef __TBB_test_common_checktype_H
#define __TBB_test_common_checktype_H

#include "config.h"

#include <atomic>

// Type that checks that there are no operations with the destroyed object
class DestroyedTracker {
protected:
    enum StateType {
        LIVE = 0x56781234,
        DEAD = 0xDEADBEEF
    };
    StateType my_state;
public:
    DestroyedTracker() : my_state(LIVE) {}
    DestroyedTracker( const DestroyedTracker& src ) : my_state(LIVE) {
        CHECK_FAST_MESSAGE(src.is_alive(), "Constructing from the dead source");
    }

    ~DestroyedTracker() {
        CHECK_FAST_MESSAGE(is_alive(), "Destructing the dead object");
        my_state = DEAD;
    }

    DestroyedTracker& operator=( const DestroyedTracker& src ) {
        CHECK_FAST_MESSAGE(is_alive(), "Assignment to the dead object");
        CHECK_FAST_MESSAGE(src.is_alive(), "Assignment from the dead source");
        return *this;
    }

    bool is_alive() const {
        return my_state == LIVE;
    }
}; // class DestroyedTracker

// Type that checks construction and destruction
template <typename Counter>
class CheckType : DestroyedTracker {
public:
    static std::atomic<int> check_type_counter;

    CheckType( Counter n = 0 ) : my_id(n), am_ready(false) {
        ++check_type_counter;
    }

    CheckType( const CheckType& other ) : DestroyedTracker(other) {
        CHECK_FAST(is_alive());
        CHECK_FAST(other.is_alive());
        my_id = other.my_id;
        am_ready = other.am_ready;
        ++check_type_counter;
    }

    operator int() const { return int(my_id); }
    CheckType& operator++() {
        ++my_id;
        return *this;
    }

    CheckType& operator=( const CheckType& other ) {
        CHECK_FAST(is_alive());
        CHECK_FAST(other.is_alive());
        my_id = other.my_id;
        am_ready = other.am_ready;
        return *this;
    }

    ~CheckType() {
        CHECK_FAST(is_alive());
        --check_type_counter;
        CHECK_FAST_MESSAGE(check_type_counter >= 0, "Too many destructions");
    }

    Counter id() const {
        CHECK_FAST(is_alive());
        return my_id;
    }

    bool is_ready() {
        CHECK_FAST(is_alive());
        return am_ready;
    }

    void get_ready() {
        CHECK_FAST(is_alive());
        if (my_id == Counter(0)) {
            my_id = Counter(1);
            am_ready = true;
        }
    }

private:
    Counter my_id;
    bool am_ready;
}; // class CheckType

namespace std {
template <typename Counter>
struct hash<CheckType<Counter>> {
    std::size_t operator()( const CheckType<Counter>& obj ) const {
        return std::size_t(obj.id());
    }
};

}

template <typename Counter>
std::atomic<int> CheckType<Counter>::check_type_counter;

// A dummy class
template <typename T>
struct Checker {
    Checker() {} // do nothing
    ~Checker() {} // do nothing
};

// A specialization for CheckType that initialize a counter on creation
// and checks that the constructions and destructions of CheckType match
template <typename Counter>
struct Checker<CheckType<Counter>> {
    Checker() { CheckType<Counter>::check_type_counter = 0; }
    ~Checker() {
        CHECK_MESSAGE(CheckType<Counter>::check_type_counter == 0,
                        "CheckType constructions and destructions don't match");
    }
}; // struct Checker

#endif // __TBB_test_common_checktype_H
