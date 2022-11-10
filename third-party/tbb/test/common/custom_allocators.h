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

#ifndef __TBB_test_common_custom_allocators_H
#define __TBB_test_common_custom_allocators_H

#include "test.h"
#include <oneapi/tbb/detail/_allocator_traits.h>
#include <memory>
#include <atomic>
#include <scoped_allocator>

template <typename CounterType>
struct ArenaData {
    char* const my_buffer;
    const std::size_t my_size;
    CounterType my_allocated; // in bytes

    template <typename T>
    ArenaData( T* buf, std::size_t sz ) noexcept
        : my_buffer(reinterpret_cast<char*>(buf)),
          my_size(sz * sizeof(T))
    {
        my_allocated = 0;
    }

    ArenaData& operator=( const ArenaData& ) = delete;
}; // struct ArenaData

template <typename T, typename POCMA = std::false_type, typename CounterType = std::size_t>
struct ArenaAllocator {
    using arena_data_type = ArenaData<CounterType>;

    arena_data_type* my_data;

    using value_type = T;
    using propagate_on_container_move_assignment = POCMA;

    template <typename U>
    struct rebind {
        using other = ArenaAllocator<U, POCMA, CounterType>;
    };

    ArenaAllocator() = default;
    ArenaAllocator( arena_data_type& data ) noexcept : my_data(&data) {}

    template <typename U, typename POCMA2>
    ArenaAllocator( const ArenaAllocator<U, POCMA2, CounterType>& other ) noexcept
        : my_data(other.my_data) {}

    friend void swap( ArenaAllocator& lhs, ArenaAllocator& rhs ) {
        using std::swap;
        swap(lhs.my_data, rhs.my_data);
    }

    value_type* address( value_type& x ) const { return &x; }
    const value_type* address( const value_type& x ) const { return &x; }

    value_type* allocate( std::size_t n ) {
        std::size_t new_size = (my_data->my_allocated += n * sizeof(T));
        REQUIRE_MESSAGE(my_data->my_allocated <= my_data->my_size, "Trying to allocate more than was reserved");
        char* result = &(my_data->my_buffer[new_size - n * sizeof(T)]);
        return reinterpret_cast<value_type*>(result);
    }

    void deallocate( value_type* ptr, std::size_t n ) {
        char* p = reinterpret_cast<char*>(ptr);
        REQUIRE_MESSAGE((p >= my_data->my_buffer && p <= my_data->my_buffer + my_data->my_size),
                        "Trying to deallocate pointer not from arena");
        REQUIRE_MESSAGE((p + n * sizeof(T) <= my_data->my_buffer + my_data->my_size),
                        "Trying to deallocate pointer not from arena");
        // utils::suppress_unused_warning(p, n);
    }

    std::size_t max_size() const noexcept {
        return my_data->my_size / sizeof(T);
    }
}; // class ArenaAllocator

template <typename T, typename U, typename POCMA, typename C>
bool operator==( const ArenaAllocator<T, POCMA, C>& lhs, const ArenaAllocator<U, POCMA, C>& rhs ) {
    return lhs.my_data == rhs.my_data;
}

template <typename T, typename U, typename POCMA, typename C>
bool operator!=( const ArenaAllocator<T, POCMA, C>& lhs, const ArenaAllocator<U, POCMA, C>& rhs ) {
    return !(lhs == rhs);
}

template <typename BaseAllocatorType>
class LocalCountingAllocator : public BaseAllocatorType {
    using base_type = BaseAllocatorType;
    using base_traits = tbb::detail::allocator_traits<base_type>;
    using counter_type = std::atomic<std::size_t>;
public:
    using value_type = typename base_type::value_type;

    std::size_t max_items;
    counter_type items_allocated;
    counter_type items_freed;
    counter_type items_constructed;
    counter_type items_destroyed;
    counter_type allocations;
    counter_type frees;

    void set_counters( std::size_t it_allocated, std::size_t it_freed,
                       std::size_t it_constructed, std::size_t it_destroyed,
                       std::size_t allocs, std::size_t fres ) {
        items_allocated = it_allocated; // TODO: may be store
        items_freed = it_freed;
        items_constructed = it_constructed;
        items_destroyed = it_destroyed;
        allocations = allocs;
        frees = fres;
    }

    template <typename Allocator>
    void set_counters( const Allocator& alloc ) {
        set_counters(alloc.items_allocated, alloc.items_freed, alloc.items_constructed,
                     alloc.items_destroyed, alloc.allocations, alloc.frees);
    }

    void clear_counters() {
        set_counters(0, 0, 0, 0, 0, 0);
    }

    template <typename U>
    struct rebind {
        using other = LocalCountingAllocator<typename base_traits::template rebind_alloc<U>>;
    };

    LocalCountingAllocator() : max_items{0} { clear_counters(); }

    LocalCountingAllocator( const LocalCountingAllocator& other )
        : base_type(other), max_items{other.max_items} { set_counters(other); }

    template <typename U>
    LocalCountingAllocator( const LocalCountingAllocator<U>& other )
        : base_type(other), max_items{other.max_items} { set_counters(other); }

    LocalCountingAllocator& operator=( const LocalCountingAllocator& other ) {
        base_type::operator=(other);
        max_items = other.max_items;
        set_counters(other);
        return *this;
    }

    value_type* allocate( std::size_t n ) {
        if (max_items != 0 && items_allocated + n >= max_items) {
            TBB_TEST_THROW(std::bad_alloc());
        }
        value_type* ptr = static_cast<base_type*>(this)->allocate(n);
        ++allocations;
        items_allocated += n;
        return ptr;
    }

    void deallocate( value_type* ptr, std::size_t n ) {
        ++frees;
        items_freed += n;
        static_cast<base_type*>(this)->deallocate(ptr, n);
    }

    template <typename U, typename... Args>
    void construct( U* ptr, Args&&... args ) {
        base_traits::construct(*this, ptr, std::forward<Args>(args)...);
        ++items_constructed;
    }

    template <typename U>
    void destroy( U* ptr ) {
        base_traits::destroy(*this, ptr);
        ++items_destroyed;
    }

    void set_limits( std::size_t max ) {
        max_items = max;
    }
}; // class LocalCountingAllocator

struct AllocatorCounters {
    using counter_type = std::atomic<std::size_t>;

    counter_type items_allocated;
    counter_type items_freed;
    counter_type items_constructed;
    counter_type items_destroyed;
    counter_type allocations;
    counter_type frees;

    AllocatorCounters() = default;

    AllocatorCounters( std::size_t it_allocated, std::size_t it_freed, std::size_t it_constructed,
                       std::size_t it_destroyed, std::size_t allocs, std::size_t fres )
        : items_allocated(it_allocated), items_freed(it_freed),
          items_constructed(it_constructed), items_destroyed(it_destroyed),
          allocations(allocs), frees(fres) {}

    AllocatorCounters( const AllocatorCounters& other )
        : items_allocated(other.items_allocated.load()),
          items_freed(other.items_allocated.load()),
          items_constructed(other.items_constructed.load()),
          items_destroyed(other.items_destroyed.load()),
          allocations(other.allocations.load()),
          frees(other.allocations.load()) {}

    AllocatorCounters& operator=( const AllocatorCounters& other ) {
        items_allocated.store(other.items_allocated.load());
        items_freed.store(other.items_freed.load());
        items_constructed.store(other.items_constructed.load());
        items_destroyed.store(other.items_destroyed.load());
        allocations.store(other.allocations.load());
        frees.store(other.frees.load());
        return *this;
    }

    friend bool operator==( const AllocatorCounters& lhs, const AllocatorCounters& rhs ) {
        return lhs.items_allocated == rhs.items_allocated &&
               lhs.items_freed == rhs.items_freed &&
               lhs.items_constructed == rhs.items_constructed &&
               lhs.items_destroyed == rhs.items_destroyed &&
               lhs.allocations == rhs.allocations &&
               lhs.frees == rhs.frees;
    }
}; // struct AllocatorCounters

template <typename BaseAllocatorType>
class StaticCountingAllocator : public BaseAllocatorType {
    using base_type = BaseAllocatorType;
    using base_traits = tbb::detail::allocator_traits<BaseAllocatorType>;
    using counter_type = std::atomic<std::size_t>;
public:
    using value_type = typename base_type::value_type;
    using pointer = value_type*;
    using counters_type = AllocatorCounters;

    static std::size_t max_items;
    static counter_type items_allocated;
    static counter_type items_freed;
    static counter_type items_constructed;
    static counter_type items_destroyed;
    static counter_type allocations;
    static counter_type frees;
    static bool throwing;

    template <typename U>
    struct rebind {
        using other = StaticCountingAllocator<typename base_traits::template rebind_alloc<U>>;
    };

    StaticCountingAllocator() = default;

    template <typename U>
    StaticCountingAllocator( const StaticCountingAllocator<U>& other ) : base_type(other) {}

    value_type* allocate( std::size_t n ) {
        if (max_items != 0 && items_allocated + n >= max_items) {
            if (throwing) {
                TBB_TEST_THROW(std::bad_alloc{});
            }
            return nullptr;
        }
        value_type* ptr = static_cast<base_type*>(this)->allocate(n);
        ++allocations;
        items_allocated += n;
        return ptr;
    }

    void deallocate(const pointer ptr, const std::size_t n){
        ++frees;
        items_freed += n;
        static_cast<base_type*>(this)->deallocate(ptr, n);
    }

    template <typename U, typename... Args>
    void construct( U* ptr, Args&&... args ) {
        ++items_constructed;
        base_traits::construct(*this, ptr, std::forward<Args>(args)...);
    }

    template <typename U>
    void destroy( U* ptr ) {
        ++items_destroyed;
        base_traits::destroy(*this, ptr);
    }

    static AllocatorCounters counters() {
        return {items_allocated, items_freed, items_constructed, items_destroyed, allocations, frees};
    }

    static void init_counters() {
        items_allocated = 0;
        items_freed = 0;
        items_constructed = 0;
        items_destroyed = 0;
        allocations = 0;
        frees = 0;
    }

    static void set_limits( std::size_t max = 0, bool do_throw = true ) {
        max_items = max;
        throwing = do_throw;
    }
}; // class StaticCountingAllocator

template <typename T>
std::size_t StaticCountingAllocator<T>::max_items;
template <typename T>
std::atomic<std::size_t> StaticCountingAllocator<T>::items_allocated;
template <typename T>
std::atomic<std::size_t> StaticCountingAllocator<T>::items_freed;
template <typename T>
std::atomic<std::size_t> StaticCountingAllocator<T>::items_constructed;
template <typename T>
std::atomic<std::size_t> StaticCountingAllocator<T>::items_destroyed;
template <typename T>
std::atomic<std::size_t> StaticCountingAllocator<T>::allocations;
template <typename T>
std::atomic<std::size_t> StaticCountingAllocator<T>::frees;
template <typename T>
bool StaticCountingAllocator<T>::throwing;

struct StaticSharedCountingAllocatorBase {
    using counter_type = std::atomic<std::size_t>;
    using counters_type = AllocatorCounters;
    static std::size_t max_items;
    static counter_type items_allocated;
    static counter_type items_freed;
    static counter_type items_constructed;
    static counter_type items_destroyed;
    static counter_type allocations;
    static counter_type frees;
    static bool throwing;

    static counters_type counters() {
        return { items_allocated.load(), items_freed.load(), items_constructed.load(),
                 items_destroyed.load(), allocations.load(), frees.load() };
    }

    static void init_counters() {
        items_allocated = 0;
        items_freed = 0;
        items_constructed = 0;
        items_destroyed = 0;
        allocations = 0;
        frees = 0;
    }

    static void set_limits( std::size_t max = 0, bool do_throw = true ) {
        max_items = max;
        throwing = do_throw;
    }
}; // class StaticSharedCountingAllocatorBase

std::size_t StaticSharedCountingAllocatorBase::max_items;
std::atomic<std::size_t> StaticSharedCountingAllocatorBase::items_constructed;
std::atomic<std::size_t> StaticSharedCountingAllocatorBase::items_destroyed;
std::atomic<std::size_t> StaticSharedCountingAllocatorBase::items_allocated;
std::atomic<std::size_t> StaticSharedCountingAllocatorBase::items_freed;
std::atomic<std::size_t> StaticSharedCountingAllocatorBase::allocations;
std::atomic<std::size_t> StaticSharedCountingAllocatorBase::frees;
bool StaticSharedCountingAllocatorBase::throwing;

template <typename BaseAllocatorType>
class StaticSharedCountingAllocator
    : public StaticSharedCountingAllocatorBase, public BaseAllocatorType
{
    using base_type = StaticSharedCountingAllocatorBase;
    using alloc_base_type = BaseAllocatorType;
    using base_traits = tbb::detail::allocator_traits<BaseAllocatorType>;
public:
    using value_type = typename alloc_base_type::value_type;
    using counters_type = AllocatorCounters;

    template <typename U>
    struct rebind {
        using other = StaticSharedCountingAllocator<typename base_traits::template rebind_alloc<U>>;
    };

    StaticSharedCountingAllocator() = default;
    StaticSharedCountingAllocator( const StaticSharedCountingAllocator& ) = default;
    StaticSharedCountingAllocator& operator=( const StaticSharedCountingAllocator& ) = default;

    template <typename U>
    StaticSharedCountingAllocator( const StaticSharedCountingAllocator<U>& other) : alloc_base_type(other) {}

    // Constructor from the base allocator with any type
    template <typename Alloc>
    StaticSharedCountingAllocator( const Alloc& src ) noexcept
        : alloc_base_type(src) {}

    value_type* allocate( std::size_t n ) {
        if (base_type::max_items != 0 &&
            base_type::items_allocated + n >= base_type::max_items) {
            if (base_type::throwing) {
                TBB_TEST_THROW(std::bad_alloc());
            }
            return nullptr;
        }
        ++base_type::allocations;
        base_type::items_allocated += n;
        return static_cast<alloc_base_type*>(this)->allocate(n);
    }

    void deallocate( value_type* ptr, std::size_t n ) {
        ++base_type::frees;
        base_type::items_freed += n;
        static_cast<alloc_base_type*>(this)->deallocate(ptr, n);
    }

    template <typename U, typename... Args>
    void construct( U* ptr, Args&&... args ) {
        base_traits::construct(*this, ptr, std::forward<Args>(args)...);
        ++base_type::items_constructed;
    }

    template <typename U>
    void destroy( U* ptr ) {
        base_traits::destroy(*this, ptr);
        ++base_type::items_destroyed;
    }
}; // class StaticSharedCountingAllocator

template <typename Allocator>
class AllocatorAwareData {
public:
    static bool assert_on_constructions;
    using allocator_type = Allocator;

    AllocatorAwareData( const allocator_type& allocator = allocator_type() )
        : my_allocator(allocator), my_value(0) {}

    AllocatorAwareData( int v, const allocator_type& allocator = allocator_type() )
        : my_allocator(allocator), my_value(v) {}

    AllocatorAwareData( const AllocatorAwareData& rhs )
        : my_allocator(rhs.my_allocator), my_value(rhs.my_value)
    {
        REQUIRE_MESSAGE(!assert_on_constructions, "Allocator should propagate to the data during copy construction");
    }

    AllocatorAwareData( AllocatorAwareData&& rhs)
        : my_allocator(rhs.my_allocator), my_value(rhs.my_value)
    {
        REQUIRE_MESSAGE(!assert_on_constructions, "Allocator should propagate to the data during move construction");
    }

    AllocatorAwareData( const AllocatorAwareData& rhs, const allocator_type& allocator )
        : my_allocator(allocator), my_value(rhs.my_value) {}

    AllocatorAwareData( AllocatorAwareData&& rhs, const allocator_type& allocator )
        : my_allocator(allocator), my_value(rhs.my_value) {}

    AllocatorAwareData& operator=( const AllocatorAwareData& other ) {
        my_value = other.my_value;
        return *this;
    }

    int value() const { return my_value; }

    static void activate() { assert_on_constructions = true; }
    static void deactivate() { assert_on_constructions = false; }
private:
    allocator_type my_allocator;
    int my_value;
}; // class AllocatorAwareData

template <typename Allocator>
bool AllocatorAwareData<Allocator>::assert_on_constructions = false;

template <typename Allocator>
bool operator==( const AllocatorAwareData<Allocator>& lhs, const AllocatorAwareData<Allocator>& rhs ) {
    return lhs.value() == rhs.value();
}

template <typename Allocator>
bool operator<( const AllocatorAwareData<Allocator>& lhs, const AllocatorAwareData<Allocator>& rhs ) {
    return lhs.value() < rhs.value();
}

namespace std {
template <typename Allocator>
struct hash<AllocatorAwareData<Allocator>> {
    std::size_t operator()(const AllocatorAwareData<Allocator>& obj) const {
        return std::hash<int>()(obj.value());
    }
};
}

template <typename Allocator, typename POCMA = std::false_type, typename POCCA = std::false_type,
          typename POCS = std::false_type>
struct PropagatingAllocator : Allocator {
    using base_allocator_traits = std::allocator_traits<Allocator>;
    using propagate_on_container_copy_assignment = POCCA;
    using propagate_on_container_move_assignment = POCMA;
    using propagate_on_container_swap = POCS;
    bool* propagated_on_copy_assignment;
    bool* propagated_on_move_assignment;
    bool* propagated_on_swap;
    bool* selected_on_copy_construction;

    template <typename U>
    struct rebind {
        using other = PropagatingAllocator<typename base_allocator_traits::template rebind_alloc<U>,
                                           POCMA, POCCA, POCS>;
    };

    PropagatingAllocator()
        : propagated_on_copy_assignment(nullptr),
          propagated_on_move_assignment(nullptr),
          propagated_on_swap(nullptr),
          selected_on_copy_construction(nullptr) {}

    PropagatingAllocator( bool& poca, bool& poma, bool& pos, bool& soc )
        : propagated_on_copy_assignment(&poca),
          propagated_on_move_assignment(&poma),
          propagated_on_swap(&pos),
          selected_on_copy_construction(&soc) {}

    PropagatingAllocator( const PropagatingAllocator& other )
        : Allocator(other),
          propagated_on_copy_assignment(other.propagated_on_copy_assignment),
          propagated_on_move_assignment(other.propagated_on_move_assignment),
          propagated_on_swap(other.propagated_on_swap),
          selected_on_copy_construction(other.selected_on_copy_construction) {}

    template <typename Allocator2>
    PropagatingAllocator( const PropagatingAllocator<Allocator2, POCMA, POCCA, POCS>& other )
        : Allocator(other),
          propagated_on_copy_assignment(other.propagated_on_copy_assignment),
          propagated_on_move_assignment(other.propagated_on_move_assignment),
          propagated_on_swap(other.propagated_on_swap),
          selected_on_copy_construction(other.selected_on_copy_construction) {}

    PropagatingAllocator& operator=( const PropagatingAllocator& ) {
        REQUIRE_MESSAGE(POCCA::value, "Allocator should not copy assign if POCCA is false");
        if (propagated_on_copy_assignment)
            *propagated_on_copy_assignment = true;
        return *this;
    }

    PropagatingAllocator& operator=( PropagatingAllocator&& ) {
        REQUIRE_MESSAGE(POCMA::value, "Allocator should not move assign if POCMA is false");
        if (propagated_on_move_assignment)
            *propagated_on_move_assignment = true;
        return *this;
    }

    PropagatingAllocator select_on_container_copy_construction() const {
        if (selected_on_copy_construction)
            *selected_on_copy_construction = true;
        return *this;
    }
}; // struct PropagatingAllocator

template <typename Allocator, typename POCMA, typename POCCA, typename POCS>
void swap( PropagatingAllocator<Allocator, POCMA, POCCA, POCS>& lhs,
           PropagatingAllocator<Allocator, POCMA, POCCA, POCS>& )
{
    REQUIRE_MESSAGE(POCS::value, "Allocator should not swap if POCS is false");
    if (lhs.propagated_on_swap)
        *lhs.propagated_on_swap = true;
}

template <typename T>
using AlwaysPropagatingAllocator = PropagatingAllocator<std::allocator<T>, /*POCMA = */std::true_type,
                                                        /*POCCA = */std::true_type, /*POCS = */std::true_type>;
template <typename T>
using NeverPropagatingAllocator = PropagatingAllocator<std::allocator<T>>;
template <typename T>
using PocmaAllocator = PropagatingAllocator<std::allocator<T>, /*POCMA = */std::true_type>;
template <typename T>
using PoccaAllocator = PropagatingAllocator<std::allocator<T>, /*POCMA = */std::false_type, /*POCCA = */std::true_type>;
template <typename T>
using PocsAllocator = PropagatingAllocator<std::allocator<T>, /*POCMA = */std::false_type, /*POCCA = */std::false_type,
                                           /*POCS = */std::true_type>;

template <typename T>
class AlwaysEqualAllocator : public std::allocator<T> {
    using base_allocator = std::allocator<T>;
public:
    using is_always_equal = std::true_type;
    using value_type = typename base_allocator::value_type;
    using propagate_on_container_move_assignment = std::false_type;

    template <typename U>
    struct rebind {
        using other = AlwaysEqualAllocator<U>;
    };

    AlwaysEqualAllocator() = default;

    AlwaysEqualAllocator( const AlwaysEqualAllocator& ) = default;

    template <typename U>
    AlwaysEqualAllocator( const AlwaysEqualAllocator<U>& other )
        : base_allocator(other) {}
}; // class AlwaysEqualAllocator

template <typename T>
class NotAlwaysEqualAllocator : public std::allocator<T> {
    using base_allocator = std::allocator<T>;
public:
    using is_always_equal = std::false_type;
    using value_type = typename base_allocator::value_type;
    using propagate_on_container_swap = std::false_type;

    template <typename U>
    struct rebind {
        using other = NotAlwaysEqualAllocator<U>;
    };

    NotAlwaysEqualAllocator() = default;

    NotAlwaysEqualAllocator( const NotAlwaysEqualAllocator& ) = default;

    template <typename U>
    NotAlwaysEqualAllocator( const NotAlwaysEqualAllocator<U>& other )
        : base_allocator(other) {}
};

template <typename T>
bool operator==( const AlwaysEqualAllocator<T>&, const AlwaysEqualAllocator<T>& ) {
#ifndef __TBB_TEST_SKIP_IS_ALWAYS_EQUAL_CHECK
    REQUIRE_MESSAGE(false, "operator== should not be called if is_always_equal is true");
#endif
    return true;
}

#endif // __TBB_test_common_custom_allocators_H
