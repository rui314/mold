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

#ifndef __TBB_test_common_container_move_support_H
#define __TBB_test_common_container_move_support_H

#include "config.h"

#include <vector>
#include <memory>
#include <type_traits>
#include <algorithm>
#include "custom_allocators.h"
#include "state_trackable.h"

namespace move_support_tests {

std::atomic<std::size_t> foo_count;
std::size_t max_foo_count = 0;
static constexpr intptr_t initial_bar = 42;
static constexpr std::size_t serial_dead_state = std::size_t(-1);

struct limit_foo_count_in_scope {
    std::size_t previous_state;
    bool active;
    limit_foo_count_in_scope(std::size_t new_limit, bool an_active = true): previous_state(max_foo_count), active(an_active) {
        if (active){
            max_foo_count = new_limit;
        }
    }
    ~limit_foo_count_in_scope(){
        if (active) {
            max_foo_count = previous_state;
        }
    }
};

template<typename static_counter_allocator_type>
struct limit_allocated_items_in_scope {
    std::size_t previous_state;
    bool active;
    limit_allocated_items_in_scope(std::size_t new_limit, bool an_active = true) : previous_state(static_counter_allocator_type::max_items), active(an_active)  {
        if (active){
            static_counter_allocator_type::set_limits(new_limit);
        }
    }
    ~limit_allocated_items_in_scope(){
        if (active) {
            static_counter_allocator_type::set_limits(previous_state);
        }
    }
};

template<int line_n>
struct track_foo_count {
    bool active;
    std::size_t previous_state;
    track_foo_count(): active(true), previous_state(foo_count) { }
    ~track_foo_count(){
        if (active){
            this->verify_no_undestroyed_foo_left_and_dismiss();
        }
    }

    //TODO: ideally in most places this check should be replaced with "no foo created or destroyed"
    //TODO: deactivation of the check seems like a hack
    void verify_no_undestroyed_foo_left_and_dismiss() {
        REQUIRE_MESSAGE( foo_count == previous_state, "Some instances of Foo were not destroyed ?" );
        active = false;
    }
};

template<typename static_counter_allocator_type>
struct track_allocator_memory {
    using counters_type = typename static_counter_allocator_type::counters_type;

    counters_type previous_state;
    track_allocator_memory() { static_counter_allocator_type::init_counters(); }
    ~track_allocator_memory(){verify_no_allocator_memory_leaks();}

    void verify_no_allocator_memory_leaks() const{
        REQUIRE_MESSAGE( static_counter_allocator_type::items_allocated == static_counter_allocator_type::items_freed, "memory leak?" );
        REQUIRE_MESSAGE( static_counter_allocator_type::allocations == static_counter_allocator_type::frees, "memory leak?" );
    }
    void save_allocator_counters(){ previous_state = static_counter_allocator_type::counters(); }
    void verify_no_more_than_x_memory_items_allocated(std::size_t  expected_number_of_items_to_allocate){
        counters_type now = static_counter_allocator_type::counters();
        REQUIRE_MESSAGE( (now.items_allocated - previous_state.items_allocated) <= expected_number_of_items_to_allocate, "More then excepted memory allocated ?" );
    }
};
#if TBB_USE_EXCEPTIONS
struct FooException : std::bad_alloc {
    virtual const char* what() const noexcept override { return "out of Foo limit"; }
    virtual ~FooException() {}
};
#endif

struct FooLimit {
    FooLimit() {
        if (max_foo_count && foo_count >= max_foo_count) {
            TBB_TEST_THROW(FooException{});
        }
    }
};

// TODO: consider better naming
class Foo : FooLimit, public StateTrackable</*allow_zero_initialized = */true> {
protected:
    using state_trackable_type = StateTrackable<true>;
    intptr_t my_bar;
    std::size_t my_serial{};
    std::size_t my_thread_id{};
public:

    bool is_valid_or_zero() const {
        return is_valid() || (state == ZeroInitialized && !my_bar);
    }

    intptr_t& zero_bar() {
        CHECK_FAST(is_valid_or_zero());
        return my_bar;
    }

    intptr_t zero_bar() const {
        CHECK_FAST(is_valid_or_zero());
        return my_bar;
    }

    intptr_t& bar() {
        CHECK_FAST(is_valid());
        return my_bar;
    }

    intptr_t bar() const {
        CHECK_FAST(is_valid());
        return my_bar;
    }

    void set_serial( std::size_t s ) {
        my_serial = s;
    }

    std::size_t get_serial() const {
        return my_serial;
    }

    void set_thread_id( std::size_t t ) {
        my_thread_id = t;
    }

    std::size_t get_thread_id() const {
        return my_thread_id;
    }

    operator intptr_t() const { return bar(); }

    Foo( intptr_t br ) : state_trackable_type(0) {
        my_bar = br;
        ++foo_count;
    }

    Foo() {
        my_bar = initial_bar;
        ++foo_count;
    }

    Foo( const Foo& foo ) : state_trackable_type(foo) {
        my_bar = foo.my_bar;
        ++foo_count;
        my_serial = foo.my_serial;
        my_thread_id = foo.my_thread_id;
    }

    Foo( Foo&& foo ) : state_trackable_type(std::move(foo)) {
        my_bar = foo.my_bar;
        my_serial = foo.my_serial;
        my_thread_id = foo.my_thread_id;
        ++foo_count;
    }

    ~Foo() {
        my_bar = ~initial_bar;
        my_serial = serial_dead_state;
        my_thread_id = serial_dead_state;
        if (state != ZeroInitialized) {
            --foo_count;
        }
    }

    Foo& operator=( const Foo& x ) {
        state_trackable_type::operator=(x);
        my_bar = x.my_bar;
        my_serial = x.my_serial;
        my_thread_id = x.my_thread_id;
        return *this;
    }

    Foo& operator=( Foo&& x ) {
        state_trackable_type::operator=(std::move(x));
        my_bar = x.my_bar;
        my_serial = x.my_serial;
        my_thread_id = x.my_thread_id;
        x.my_serial = serial_dead_state;
        x.my_thread_id = serial_dead_state;
        x.my_bar = -1;
        return *this;
    }

    friend bool operator==( const int& lhs, const Foo& rhs ) {
        CHECK_FAST_MESSAGE(rhs.is_valid_or_zero(), "Comparing invalid objects");
        return lhs == rhs.my_bar;
    }

    friend bool operator==( const Foo& lhs, const int& rhs ) {
        CHECK_FAST_MESSAGE(lhs.is_valid_or_zero(), "Comparing invalid objects");
        return lhs.my_bar == rhs;
    }

    friend bool operator==( const Foo& lhs, const Foo& rhs ) {
        CHECK_FAST_MESSAGE(lhs.is_valid_or_zero(), "Comparing invalid objects");
        CHECK_FAST_MESSAGE(rhs.is_valid_or_zero(), "Comparing invalid objects");
        return lhs.my_bar == rhs.my_bar;
    }

    friend bool operator<( const Foo& lhs, const Foo& rhs ) {
        CHECK_FAST_MESSAGE(lhs.is_valid_or_zero(), "Comparing invalid objects");
        CHECK_FAST_MESSAGE(rhs.is_valid_or_zero(), "Comparing invalid objects");
        return lhs.my_bar < rhs.my_bar;
    }

    bool is_const() const { return true; }
    bool is_const() { return false; }

protected:
    char reserve[1];
}; // struct Foo

struct FooWithAssign : Foo {
    FooWithAssign() = default;
    FooWithAssign( intptr_t b ) : Foo(b) {}
    FooWithAssign( const FooWithAssign& ) = default;
    FooWithAssign( FooWithAssign&& ) = default;

    FooWithAssign& operator=( const FooWithAssign& f ) {
        return static_cast<FooWithAssign&>(Foo::operator=(f));
    }

    FooWithAssign& operator=( FooWithAssign&& f ) {
        return static_cast<FooWithAssign&>(Foo::operator=(std::move(f)));
    }
}; // struct FooWithAssign

template <typename FooIteratorType>
class FooIteratorBase {
protected:
    intptr_t x_bar;
private:
    FooIteratorType& as_derived() { return *static_cast<FooIteratorType*>(this); }
public:
    FooIteratorBase( intptr_t x ) : x_bar(x) {}

    FooIteratorType& operator++() {
        ++x_bar;
        return as_derived();
    }

    FooIteratorType operator++(int) {
        FooIteratorType tmp(as_derived());
        ++x_bar;
        return tmp;
    }

    friend bool operator==( const FooIteratorType& lhs, const FooIteratorType& rhs ) {
        return lhs.x_bar == rhs.x_bar;
    }

    friend bool operator!=( const FooIteratorType& lhs, const FooIteratorType& rhs ) {
        return !(lhs == rhs);
    }
}; // class FooIteratorBase

class FooIterator : public FooIteratorBase<FooIterator> {
    using base_type = FooIteratorBase<FooIterator>;
public:
    using iterator_category = std::input_iterator_tag;
    using value_type = FooWithAssign;
    using difference_type = std::ptrdiff_t;
    using pointer = value_type*;
    using reference = value_type&;

    using base_type::base_type;

    value_type operator*() {
        return value_type(x_bar);
    }
}; // class FooIterator

class FooPairIterator : public FooIteratorBase<FooPairIterator> {
    using base_type = FooIteratorBase<FooPairIterator>;
public:
    using iterator_category = std::input_iterator_tag;
    using value_type = std::pair<FooWithAssign, FooWithAssign>;
    using difference_type = std::ptrdiff_t;
    using pointer = value_type*;
    using reference = value_type&;

    using base_type::base_type;

    value_type operator*() {
        FooWithAssign foo;
        foo.bar() = x_bar;
        return std::make_pair(foo, foo);
    }
}; // class FooPairIterator

struct MemoryLocations {
    std::vector<const void*> locations;

    template <typename ContainerType>
    MemoryLocations( const ContainerType& source ) : locations(source.size()) {
        for (auto it = source.begin(); it != source.end(); ++it) {
            locations[std::distance(source.begin(), it)] = &*it;
        }
    }

    template <typename ContainerType>
    bool content_location_unchanged( const ContainerType& dst ) {
        auto is_same_location = []( const typename ContainerType::value_type& v, const void* location ) {
            return &v == location;
        };

        return std::equal(dst.begin(), dst.end(), locations.begin(), is_same_location);
    }

    template <typename ContainerType>
    bool content_location_changed( const ContainerType& dst ) {
        auto is_not_same_location = []( const typename ContainerType::value_type& v, const void* location ) {
            return &v != location;
        };
        return std::equal(dst.begin(), dst.end(), locations.begin(), is_not_same_location);
    }
}; // struct MemoryLocations

template <typename T, typename POCMA = std::false_type>
struct ArenaAllocatorFixture {
    using allocator_type = ArenaAllocator<T, POCMA>;
    using arena_data_type = typename allocator_type::arena_data_type;

    std::vector<typename std::aligned_storage<sizeof(T)>::type> storage;
    arena_data_type arena_data;
    allocator_type allocator;

    ArenaAllocatorFixture( std::size_t size_to_allocate )
        : storage(size_to_allocate),
          arena_data(reinterpret_cast<T*>(&storage.front()), storage.size()),
          allocator(arena_data) {}

    ArenaAllocatorFixture( const ArenaAllocatorFixture& ) = delete;
}; // struct ArenaAllocatorFixture

template <typename T, typename POCMA = std::false_type>
struct TwoMemoryArenasFixture {
    using arena_fixture_type = ArenaAllocatorFixture<T, POCMA>;
    using allocator_type = typename arena_fixture_type::allocator_type;

    arena_fixture_type source_arena_fixture;
    arena_fixture_type dst_arena_fixture;

    allocator_type& source_allocator;
    allocator_type& dst_allocator;

    TwoMemoryArenasFixture( std::size_t size_to_allocate )
        : source_arena_fixture(size_to_allocate),
          dst_arena_fixture(size_to_allocate),
          source_allocator(source_arena_fixture.allocator),
          dst_allocator(dst_arena_fixture.allocator)
    {
        REQUIRE_MESSAGE(&(*source_arena_fixture.storage.begin()) != &(*dst_arena_fixture.storage.begin()),
                        "source and destination arena instances should use difference memory regions");
        REQUIRE_MESSAGE(source_allocator != dst_allocator, "arenas using difference memory regions should not compare equal");
        using Traits_POCMA = typename tbb::detail::allocator_traits<allocator_type>::propagate_on_container_move_assignment;
        REQUIRE_MESSAGE(POCMA::value == Traits_POCMA::value,
                        "POCMA::value should be the same as in allocator_traits");

        allocator_type source_allocator_copy(source_allocator);
        allocator_type dst_allocator_copy(dst_allocator);
        allocator_type source_previous_state(source_allocator);

        REQUIRE_MESSAGE(source_previous_state == source_allocator,
                        "Copy of the allocator should compare equal with it's source");
        dst_allocator_copy = std::move(source_allocator_copy);
        REQUIRE_MESSAGE(dst_allocator_copy == source_previous_state,
                        "Move initialized allocator should compare equal with it's source before movement");
    }

    TwoMemoryArenasFixture( const TwoMemoryArenasFixture& ) = delete;

    void verify_allocator_was_moved( const allocator_type& result_allocator ) {
        // TODO: add assert that move ctor/assignment was called
        REQUIRE_MESSAGE(result_allocator == source_allocator, "allocator was not moved");
        REQUIRE_MESSAGE(result_allocator != dst_allocator, "allocator_was_not_moved");
    }

}; // struct TwoMemoryArenasFixture

template <typename ContainerTraits, typename Allocator>
struct MoveFixture {
    using element_type = typename Allocator::value_type;
    using container_value_type = typename ContainerTraits::template container_value_type<element_type>;
    using allocator_type = typename tbb::detail::allocator_traits<Allocator>::template rebind_alloc<container_value_type>;
    using container_type = typename ContainerTraits::template container_type<element_type, allocator_type>;
    using init_iterator_type = typename ContainerTraits::init_iterator_type;

    static constexpr std::size_t default_container_size = 100;
    const std::size_t container_size;

    typename std::aligned_storage<sizeof(container_type)>::type source_storage;
    container_type& source;

    MemoryLocations locations;

    MoveFixture( std::size_t cont_size = default_container_size )
        : container_size(cont_size),
          source(ContainerTraits::template construct_container<container_type>(source_storage,
                                                                               init_iterator_type(0), init_iterator_type(cont_size))),
          locations(source)
    {
        init();
    }

    MoveFixture( const Allocator& a, std::size_t cont_size = default_container_size )
        : container_size(cont_size),
          source(ContainerTraits::template construct_container<container_type>(source_storage, init_iterator_type(0),
                                                                               init_iterator_type(cont_size), a)),
          locations(source)
    {
        init();
    }

    MoveFixture( const MoveFixture& ) = delete;

    ~MoveFixture() {
        reinterpret_cast<container_type*>(&source)->~container_type();
    }

    void init() {
        verify_size(source);
        verify_content_equal_to_source(source);
        verify_size(locations.locations);
    }

    bool content_location_unchanged( const container_type& dst ) {
        return locations.content_location_unchanged(dst);
    }

    bool content_location_changed( const container_type& dst ) {
        return locations.content_location_changed(dst);
    }

    template <typename ContainerType>
    void verify_size( const ContainerType& dst ) {
        REQUIRE(container_size == dst.size());
    }

    void verify_content_equal_to_source( const container_type& dst ) {
        REQUIRE(ContainerTraits::equal(dst, init_iterator_type(0), init_iterator_type(container_size)));
    }

    void verify_content_equal_to_source( const container_type& dst, std::size_t number_of_constructed_items ) {
        REQUIRE(number_of_constructed_items <= dst.size());
        REQUIRE(std::equal(dst.begin(), dst.begin() + number_of_constructed_items, init_iterator_type(0)));
    }

    void verify_content_shallow_moved( const container_type& dst ) {
        verify_size(dst);
        REQUIRE_MESSAGE(content_location_unchanged(dst), "Container move ctor actually changed element locations, while should not");
        REQUIRE_MESSAGE(source.empty(), "Moved from container should not contain any elements");
        verify_content_equal_to_source(dst);
    }

    void verify_content_deep_moved( const container_type& dst ) {
        verify_size(dst);
        REQUIRE_MESSAGE(content_location_changed(dst), "Container did not changed element locations for unequal allocators");
        REQUIRE_MESSAGE(std::all_of(dst.begin(), dst.end(), is_state_predicate<Foo::MoveInitialized>()),
                        "Container did not move construct some elements");
        REQUIRE_MESSAGE(std::all_of(source.begin(), source.end(), is_state_predicate<Foo::MovedFrom>()),
                        "Container did not move all the elements");
        verify_content_equal_to_source(dst);
    }

    void verify_part_of_content_deep_moved(container_type const& dst, std::size_t number_of_constructed_items){
        REQUIRE_MESSAGE(content_location_changed(dst), "Vector actually did not changed element locations for unequal allocators, while should");
        REQUIRE_MESSAGE(std::all_of(dst.begin(), dst.begin() + number_of_constructed_items, is_state_predicate<Foo::MoveInitialized>{}), "Vector did not move construct some elements?");
        if (dst.size() != number_of_constructed_items) {
            REQUIRE_MESSAGE(std::all_of(dst.begin() + number_of_constructed_items, dst.end(), is_state_predicate<Foo::ZeroInitialized>{}), "Failed to zero-initialize items left not constructed after the exception?" );
        }
        verify_content_equal_to_source(dst, number_of_constructed_items);

        REQUIRE_MESSAGE(std::all_of(source.begin(), source.begin() + number_of_constructed_items, is_state_predicate<Foo::MovedFrom>{}),  "Vector did not move all the elements?");
        REQUIRE_MESSAGE(std::all_of(source.begin() + number_of_constructed_items, source.end(), is_not_state_predicate<Foo::MovedFrom>{}),  "Vector changed elements in source after exception point?");
    }
}; // struct MoveFixture

template <typename StaticCountingAllocatorType>
struct TrackAllocatorMemory {
    using counters_type = typename StaticCountingAllocatorType::counters_type;

    counters_type previous_state;

    TrackAllocatorMemory() {
        StaticCountingAllocatorType::init_counters();
    }

    TrackAllocatorMemory( const TrackAllocatorMemory& ) = delete;

    ~TrackAllocatorMemory() {
        verify_no_allocator_memory_leaks();
    }

    void verify_no_allocator_memory_leaks() const {
        REQUIRE_MESSAGE(StaticCountingAllocatorType::items_allocated == StaticCountingAllocatorType::items_freed, "Memory leak");
        REQUIRE_MESSAGE(StaticCountingAllocatorType::allocations == StaticCountingAllocatorType::frees, "Memory leak");
        REQUIRE_MESSAGE(StaticCountingAllocatorType::items_constructed == StaticCountingAllocatorType::items_destroyed,
                        "The number of constructed items is not equal to the number of destroyed items");
    }

    void save_allocator_counters() { previous_state = StaticCountingAllocatorType::counters(); }

    void verify_no_more_than_x_memory_items_allocated( std::size_t expected ) {
        counters_type now = StaticCountingAllocatorType::counters();
        REQUIRE_MESSAGE((now.items_allocated - previous_state.items_allocated) <= expected,
                        "More then expected memory allocated");
    }
}; // struct TrackAllocatorMemory

struct TrackFooCount {
    TrackFooCount() : active(true), previous_state(foo_count) {}

    TrackFooCount( const TrackFooCount& ) = delete;

    ~TrackFooCount() {
        if (active) {
            verify_no_undestroyed_foo_left_and_dismiss();
        }
    }

    void verify_no_undestroyed_foo_left_and_dismiss() {
        REQUIRE_MESSAGE(foo_count == previous_state, "Some instances of Foo were not destroyed");
        active = false;
    }

    bool active;
    std::size_t previous_state;
}; // struct TrackFooCount

template <typename ContainerTraits, typename POCMA = std::false_type, typename T = FooWithAssign>
struct DefaultStatefulFixtureHelper {
    using allocator_fixture_type = TwoMemoryArenasFixture<T, POCMA>;
    using allocator_type = StaticSharedCountingAllocator<typename allocator_fixture_type::allocator_type>;

    using move_fixture_type = MoveFixture<ContainerTraits, allocator_type>;

    using leaks_tracker_type = TrackAllocatorMemory<allocator_type>;
    using foo_leaks_in_test_tracker_type = TrackFooCount;

    struct DefaultStatefulFixture
        : leaks_tracker_type,
          allocator_fixture_type,
          move_fixture_type,
          foo_leaks_in_test_tracker_type
    {
        //TODO: calculate needed size for allocator_fixture_type more accurately
        //allocate twice more storage to handle case when copy constructor called instead of move one
        DefaultStatefulFixture()
            : leaks_tracker_type(),
              allocator_fixture_type(2 * 4 * move_fixture_type::default_container_size),
              move_fixture_type(allocator_fixture_type::source_allocator),
              foo_leaks_in_test_tracker_type()
        {
            leaks_tracker_type::save_allocator_counters();
        }

        void verify_no_more_than_x_memory_items_allocated() {
            auto n = ContainerTraits::expected_number_of_items_to_allocate_for_steal_move;
            leaks_tracker_type::verify_no_more_than_x_memory_items_allocated(n);
        }

        using allocator_type = typename move_fixture_type::container_type::allocator_type;
    }; // struct DefaultStatefulFixture

    using type = DefaultStatefulFixture;
}; // struct DefaultStatefulFixtureHelper

template <typename StaticCountingAllocatorType>
struct LimitAllocatedItemsInScope {
    LimitAllocatedItemsInScope( std::size_t limit, bool act = true )
        : previous_state(StaticCountingAllocatorType::max_items), active(act)
    {
        if (active) {
            StaticCountingAllocatorType::set_limits(limit);
        }
    }

    LimitAllocatedItemsInScope( const LimitAllocatedItemsInScope& ) = delete;

    ~LimitAllocatedItemsInScope() {
        if (active) {
            StaticCountingAllocatorType::set_limits(previous_state);
        }
    }

    std::size_t previous_state;
    bool active;
}; // struct LimitAllocatedItemsInScope

struct LimitFooCountInScope {
    LimitFooCountInScope( std::size_t limit, bool act = true )
        : previous_state(max_foo_count), active(act)
    {
        if (active) {
            max_foo_count = limit;
        }
    }

    LimitFooCountInScope( const LimitFooCountInScope& ) = delete;

    ~LimitFooCountInScope() {
        if (active) {
            max_foo_count = previous_state;
        }
    }

    std::size_t previous_state;
    bool active;
}; // struct LimitFooCountInScope

template <typename ContainerTraits>
void test_move_ctor_single_argument() {
    using fixture_type = typename DefaultStatefulFixtureHelper<ContainerTraits>::type;
    using container_type = typename fixture_type::container_type;

    fixture_type fixture;

    container_type dst(std::move(fixture.source));

    fixture.verify_content_shallow_moved(dst);
    fixture.verify_allocator_was_moved(dst.get_allocator());
    fixture.verify_no_more_than_x_memory_items_allocated();
    fixture.verify_no_undestroyed_foo_left_and_dismiss();
}

template <typename ContainerTraits>
void test_move_ctor_with_equal_allocator() {
    using fixture_type = typename DefaultStatefulFixtureHelper<ContainerTraits>::type;
    using container_type = typename fixture_type::container_type;

    fixture_type fixture;

    container_type dst(std::move(fixture.source), fixture.source.get_allocator());

    fixture.verify_content_shallow_moved(dst);
    fixture.verify_no_more_than_x_memory_items_allocated();
    fixture.verify_no_undestroyed_foo_left_and_dismiss();
}

template <typename ContainerTraits>
void test_move_ctor_with_unequal_allocator() {
    using fixture_type = typename DefaultStatefulFixtureHelper<ContainerTraits>::type;
    using container_type = typename fixture_type::container_type;

    fixture_type fixture;

    typename container_type::allocator_type alloc(fixture.dst_allocator);
    container_type dst(std::move(fixture.source), alloc);

    fixture.verify_content_deep_moved(dst);
}

template <typename ContainerTraits>
void test_move_assignment_POCMA_true_stateful_allocator() {
    using fixture_type = typename DefaultStatefulFixtureHelper<ContainerTraits, /*POCMA = */std::true_type>::type;
    using container_type = typename fixture_type::container_type;

    fixture_type fixture;

    container_type dst(fixture.dst_allocator);
    dst = std::move(fixture.source);

    fixture.verify_content_shallow_moved(dst);
    fixture.verify_allocator_was_moved(dst.get_allocator());
    fixture.verify_no_more_than_x_memory_items_allocated();
    fixture.verify_no_undestroyed_foo_left_and_dismiss();
}

template <typename ContainerTraits>
void test_move_assignment_POCMA_true_stateless_allocator() {
    // POCMA is true for std::allocator since C++14, is_always_equal is true since C++17
    // Behavior can be unexpected, TODO: consider another allocator type
    using allocator_type = std::allocator<FooWithAssign>;
    using fixture_type = MoveFixture<ContainerTraits, allocator_type>;
    using container_type = typename fixture_type::container_type;

    fixture_type fixture;

    REQUIRE_MESSAGE(fixture.source.get_allocator() == allocator_type(), "Incorrect test setup: allocator is stateful");

    container_type dst;
    dst = std::move(fixture.source);

    fixture.verify_content_shallow_moved(dst);
}

template <typename ContainerTraits>
void test_move_assignment_POCMA_false_equal_allocator() {
    using fixture_type = typename DefaultStatefulFixtureHelper<ContainerTraits, /*POCMA = */std::false_type>::type;
    using container_type = typename fixture_type::container_type;

    fixture_type fixture;
    container_type dst(fixture.source_allocator);
    REQUIRE_MESSAGE(fixture.source.get_allocator() == dst.get_allocator(), "Incorrect test setup: allocators should be equal");

    fixture.save_allocator_counters();

    dst = std::move(fixture.source);

    fixture.verify_content_shallow_moved(dst);
    fixture.verify_no_more_than_x_memory_items_allocated();
    fixture.verify_no_undestroyed_foo_left_and_dismiss();
}

template <typename ContainerTraits>
void test_move_assignment_POCMA_false_unequal_allocator() {
    using fixture_type = typename DefaultStatefulFixtureHelper<ContainerTraits, /*POCMA = */std::false_type>::type;
    using container_type = typename fixture_type::container_type;

    fixture_type fixture;

    container_type dst(fixture.dst_allocator);
    dst = std::move(fixture.source);

    fixture.verify_content_deep_moved(dst);
}

#define REQUIRE_THROW_EXCEPTION(expr, exception_type)            \
        try {                                                    \
            expr;                                                \
            REQUIRE_MESSAGE(false, "Exception should be thrown");\
        } catch (exception_type&) {                              \
        } catch (...) {                                          \
            REQUIRE_MESSAGE(false, "Unexpected exception");      \
        }                                                        \

#if TBB_USE_EXCEPTIONS
template <typename ContainerTraits>
void test_ex_move_ctor_unequal_allocator_memory_failure() {
    using fixture_type = typename DefaultStatefulFixtureHelper<ContainerTraits>::type;
    using container_type = typename fixture_type::container_type;
    using allocator_type = typename container_type::allocator_type;

    fixture_type fixture;

    std::size_t limit = allocator_type::items_allocated + fixture.container_size / 4;
    LimitAllocatedItemsInScope<allocator_type> alloc_limit(limit);

    REQUIRE_THROW_EXCEPTION(container_type dst(std::move(fixture.source), fixture.dst_allocator), std::bad_alloc);
}

template <typename ContainerTraits>
void test_ex_move_ctor_unequal_allocator_element_ctor_failure() {
    using fixture_type = typename DefaultStatefulFixtureHelper<ContainerTraits>::type;
    using container_type = typename fixture_type::container_type;

    fixture_type fixture;

    std::size_t limit = foo_count + fixture.container_size / 4;
    LimitFooCountInScope foo_limit(limit);
    REQUIRE_THROW_EXCEPTION(container_type dst(std::move(fixture.source), fixture.dst_allocator), FooException);
}

template <typename ContainerTraits>
void test_ex_move_constructor() {
    test_ex_move_ctor_unequal_allocator_memory_failure<ContainerTraits>();
    test_ex_move_ctor_unequal_allocator_element_ctor_failure<ContainerTraits>();
    // TODO: add test for move assignment exceptions
}
#endif

template <typename ContainerTraits>
void test_move_constructor() {
    test_move_ctor_single_argument<ContainerTraits>();
    test_move_ctor_with_equal_allocator<ContainerTraits>();
    test_move_ctor_with_unequal_allocator<ContainerTraits>();
}

template <typename ContainerTraits>
void test_move_assignment() {
    test_move_assignment_POCMA_true_stateful_allocator<ContainerTraits>();
    test_move_assignment_POCMA_true_stateless_allocator<ContainerTraits>();
    test_move_assignment_POCMA_false_equal_allocator<ContainerTraits>();
    test_move_assignment_POCMA_false_unequal_allocator<ContainerTraits>();
}

template<typename container_traits>
void test_constructor_with_move_iterators(){
    using fixture_type = typename move_support_tests::DefaultStatefulFixtureHelper<container_traits>::type;
    using container_type = typename fixture_type::container_type;

    fixture_type fixture;

    container_type dst(std::make_move_iterator(fixture.source.begin()), std::make_move_iterator(fixture.source.end()), fixture.dst_allocator);

    fixture.verify_content_deep_moved(dst);
}

template<typename container_traits>
void test_assign_with_move_iterators(){
    using fixture_type = typename move_support_tests::DefaultStatefulFixtureHelper<container_traits>::type;
    using container_type = typename fixture_type::container_type;

    fixture_type fixture;

    container_type dst(fixture.dst_allocator);
    dst.assign(std::make_move_iterator(fixture.source.begin()), std::make_move_iterator(fixture.source.end()));

    fixture.verify_content_deep_moved(dst);
}

} // namespace move_support_tests

namespace std {
template <>
struct hash<move_support_tests::Foo> {
    std::size_t operator()( const move_support_tests::Foo& f ) const {
        return std::size_t(f.bar());
    }
};

template <>
struct hash<move_support_tests::FooWithAssign> {
    std::size_t operator()( const move_support_tests::FooWithAssign& f ) const {
        return std::hash<move_support_tests::Foo>{}(f);
    }
};
}

#endif // __TBB_test_common_container_move_support_H
