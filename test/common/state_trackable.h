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

#ifndef __TBB_test_common_state_trackable_H
#define __TBB_test_common_state_trackable_H

#include "common/test.h"
#include <map>
#include <atomic>

// Declarations for a class that can track operations applied to its objects.

struct StateTrackableBase {
    enum StateValue {
        ZeroInitialized     = 0,
        DefaultInitialized  = 0xDEFAUL,
        DirectInitialized   = 0xD1111,
        CopyInitialized     = 0xC0314,
        MoveInitialized     = 0xAAAAA,
        CopyAssigned        = 0x11AED,
        MoveAssigned        = 0x22AED,
        MovedFrom           = 0xFFFFF,
        Destroyed           = 0xDEADF00,
        Unspecified         = 0xEEEEE
    };

    class State {
    public:
        State() noexcept : state(Unspecified) {}

        State( const State& s ) : state(Unspecified) {
            assign_new_state(s.state);
        }

        State( StateValue s ) noexcept : state(Unspecified) {
            assign_new_state(s);
        }

        State& operator=( const State& st ) noexcept {
            assign_new_state(st.state);
            return *this;
        }

        State& operator=( StateValue s ) noexcept {
            assign_new_state(s);
            return *this;
        }

        operator StateValue() const noexcept { return state; }
    private:
        void assign_new_state( StateValue s ) noexcept;
        StateValue state;
    }; // class State
}; // struct StateTrackableBase

struct StateTrackableCounters {
    static void reset() {
        counters[StateTrackableBase::ZeroInitialized] = counters[StateTrackableBase::DefaultInitialized] =
        counters[StateTrackableBase::DirectInitialized] = counters[StateTrackableBase::CopyInitialized] =
        counters[StateTrackableBase::MoveInitialized] = counters[StateTrackableBase::CopyAssigned] =
        counters[StateTrackableBase::MoveAssigned] = counters[StateTrackableBase::MovedFrom] =
        counters[StateTrackableBase::Destroyed] = counters[StateTrackableBase::Unspecified] = 0;
    }

    static bool initialize() {
        reset();
        return true;
    }

    using counters_type = std::map<StateTrackableBase::StateValue, std::atomic<std::size_t>>;
    static counters_type counters;
}; // struct StateTrackableCounters

StateTrackableCounters::counters_type StateTrackableCounters::counters;
static const bool state_initialized = StateTrackableCounters::initialize();

void StateTrackableBase::State::assign_new_state( StateValue s ) noexcept {
    CHECK_FAST_MESSAGE(state_initialized, "State trackable counters are not initialized");
    CHECK_FAST_MESSAGE((s == StateTrackableBase::Unspecified ||
                     StateTrackableCounters::counters.find(s) != StateTrackableCounters::counters.end()),
                     "Current state value is unknown");
    CHECK_FAST_MESSAGE((state == StateTrackableBase::Unspecified ||
                     StateTrackableCounters::counters.find(state) != StateTrackableCounters::counters.end()),
                     "New state value is unknown");
    state = s;
    ++StateTrackableCounters::counters[state];
}

template <bool AllowZeroInitialized = false>
struct StateTrackable : StateTrackableBase {
    static constexpr bool allow_zero_initialized = AllowZeroInitialized;

    bool is_valid() const {
        return state == DefaultInitialized || state == DirectInitialized || state == CopyInitialized ||
               state == MoveInitialized || state == CopyAssigned || state == MoveAssigned || state == MovedFrom ||
               (allow_zero_initialized && state == ZeroInitialized);
    }

    StateTrackable( intptr_t ) noexcept : state(DirectInitialized) {}
    StateTrackable() noexcept : state(DefaultInitialized) {}

    StateTrackable( const StateTrackable& src ) noexcept : state(CopyInitialized) {
        CHECK_FAST_MESSAGE(src.is_valid(), "Bad source for copy ctor");
    }

    StateTrackable( StateTrackable&& src ) noexcept : state(MoveInitialized) {
        CHECK_FAST_MESSAGE(src.is_valid(), "Bad source for move ctor");
        src.state = MovedFrom;
    }

    StateTrackable& operator=( const StateTrackable& src ) noexcept {
        CHECK_FAST_MESSAGE(is_valid(), "Copy assignment to invalid instance");
        CHECK_FAST_MESSAGE(src.is_valid(), "Bad source for copy assignment");
        state = CopyAssigned;
        return *this;
    }

    StateTrackable& operator=( StateTrackable&& src ) noexcept {
        CHECK_FAST_MESSAGE(is_valid(), "Move assignment to invalid instance");
        CHECK_FAST_MESSAGE(src.is_valid(), "Bad source for move assignment");
        state = MoveAssigned;
        src.state = MovedFrom;
        return *this;
    }

    ~StateTrackable() noexcept {
        CHECK_FAST_MESSAGE(is_valid(), "Calling destructor on invalid instance. (May be twice)");
        state = Destroyed;
    }

    State state;
}; // struct StateTrackable

template <StateTrackableBase::StateValue desired_state, bool allow_zero_initialized>
bool is_state( const StateTrackable<allow_zero_initialized>& f ) {
    return f.state == desired_state;
}

template <StateTrackableBase::StateValue desired_state>
struct is_not_state_predicate {
    template <bool allow_zero_initialized>
    bool operator()( const StateTrackable<allow_zero_initialized>& f ) {
        return !is_state<desired_state>(f);
    }

    // To operate with the associative containers
    template <typename T, typename U>
    bool operator()( const std::pair<T, U>& p ) {
        return !is_state<desired_state>(p.second);
    }
};

template <StateTrackableBase::StateValue desired_state>
struct is_state_predicate {
    template <bool allow_zero_initialized>
    bool operator()( const StateTrackable<allow_zero_initialized>& f ) {
        return is_state<desired_state>(f);
    }

    template <typename T, typename U>
    bool operator()( const std::pair<T, U>& p ) {
        return is_state<desired_state>(p.second);
    }
};

#endif // __TBB_test_common_state_trackable_H
