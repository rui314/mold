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

#ifndef __TBB_test_common_containers_common_H
#define __TBB_test_common_containers_common_H

#include "config.h"
#include "custom_allocators.h"

template <typename ContainerType>
void test_allocator_traits() {
    using allocator_type = typename ContainerType::allocator_type;
    using allocator_traits_type = std::allocator_traits<allocator_type>;
    using pocca_type = typename allocator_traits_type::propagate_on_container_copy_assignment;
    using pocma_type = typename allocator_traits_type::propagate_on_container_move_assignment;
    using pocs_type = typename allocator_traits_type::propagate_on_container_swap;

    bool propagated_on_copy = false;
    bool propagated_on_move = false;
    bool propagated_on_swap = false;
    bool selected_on_copy = false;

    allocator_type alloc(propagated_on_copy, propagated_on_move, propagated_on_swap, selected_on_copy);

    ContainerType c1(alloc), c2(c1);
    REQUIRE_MESSAGE(selected_on_copy, "select_on_container_copy_construction function was not called");

    c1 = c2;
    REQUIRE_MESSAGE(propagated_on_copy == pocca_type::value, "Unexpected allocator propagation on copy assignment");

    c2 = std::move(c1);
    REQUIRE_MESSAGE(propagated_on_move == pocma_type::value, "Unexpected allocator propagation on move assignment");

    c1.swap(c2);
    REQUIRE_MESSAGE(propagated_on_swap == pocs_type::value, "Unexpected allocator propagation on swap");

    propagated_on_move = false;
    propagated_on_swap = false;
    using std::swap;
    swap(c1, c2);
    REQUIRE_MESSAGE(propagated_on_move == false, "Unexpected allocator propagation on non-member swap");
    REQUIRE_MESSAGE(propagated_on_swap == pocs_type::value, "Unexpected allocator propagation on non-member swap");
}

struct NonMovableObject {
    NonMovableObject() = default;
    NonMovableObject( const NonMovableObject& ) = delete;
    NonMovableObject( NonMovableObject&& ) = delete;
    NonMovableObject& operator=( const NonMovableObject& ) = delete;
    NonMovableObject& operator=( NonMovableObject&& ) = delete;
};

namespace std {
template <>
struct hash<NonMovableObject> {
    std::size_t operator()( const NonMovableObject& ) { return 1; }
};
}

template <typename ContainerType>
void test_allocator_traits_with_non_movable_value_type() {
    // Check that if pocma is true, container allows move assignment without per-element move
    using allocator_type = typename ContainerType::allocator_type;
    using allocator_traits_type = std::allocator_traits<allocator_type>;
    using pocma_type = typename allocator_traits_type::propagate_on_container_move_assignment;
    REQUIRE_MESSAGE(pocma_type::value, "Allocator POCMA should be true for this test");
    allocator_type alloc;
    ContainerType container1(alloc), container2(alloc);
    container1 = std::move(container2);
}

template <typename ContainerType>
void test_is_always_equal() {
    using allocator_type = typename ContainerType::allocator_type;
    allocator_type alloc;

    ContainerType container1(alloc), container2(std::move(container1), alloc);

    container1 = std::move(container2);

    container1.swap(container2);

    using std::swap;
    swap(container1, container2);
}

template <typename ContainerTraits>
void test_allocator_traits_support() {
    using container_value_type = typename ContainerTraits::template container_value_type<int>;

    using always_propagating_allocator_type = AlwaysPropagatingAllocator<container_value_type>;
    using never_propagating_allocator_type = NeverPropagatingAllocator<container_value_type>;
    using pocma_allocator_type = PocmaAllocator<container_value_type>;
    using pocca_allocator_type = PoccaAllocator<container_value_type>;
    using pocs_allocator_type = PocsAllocator<container_value_type>;
    using always_equal_allocator_type = AlwaysEqualAllocator<container_value_type>;

    using always_container_type = typename ContainerTraits::template container_type<int, always_propagating_allocator_type>;
    using never_container_type = typename ContainerTraits::template container_type<int, never_propagating_allocator_type>;
    using pocma_container_type = typename ContainerTraits::template container_type<int, pocma_allocator_type>;
    using pocca_container_type = typename ContainerTraits::template container_type<int, pocca_allocator_type>;
    using pocs_container_type = typename ContainerTraits::template container_type<int, pocs_allocator_type>;
    using always_equal_container_type = typename ContainerTraits::template container_type<int, always_equal_allocator_type>;

    test_allocator_traits<always_container_type>();
    test_allocator_traits<never_container_type>();
    test_allocator_traits<pocma_container_type>();
    test_allocator_traits<pocca_container_type>();
    test_allocator_traits<pocs_container_type>();

    using container_non_movable_value_type = typename ContainerTraits::template container_value_type<NonMovableObject>;
    using pocma_allocator_non_movable_value_type = PocmaAllocator<container_non_movable_value_type>;
    using pocma_container_non_movable_value_type = typename ContainerTraits::template
                                                   container_type<NonMovableObject, pocma_allocator_non_movable_value_type>;
    test_allocator_traits_with_non_movable_value_type<pocma_container_non_movable_value_type>();
    test_is_always_equal<always_equal_container_type>();
}

#if TBB_USE_EXCEPTIONS
struct ThrowOnCopy {
    static int error_code() { return 8; };
    static bool is_active;
    ThrowOnCopy() = default;
    ThrowOnCopy( const ThrowOnCopy& ) {
        if (is_active) {
            throw error_code();
        }
    }
    static void activate() { is_active = true; }
    static void deactivate() { is_active = false; }

    bool operator<( const ThrowOnCopy& ) const { return true; }
    bool operator==( const ThrowOnCopy& ) const { return true; }
}; // struct ThrowOnCopy

bool ThrowOnCopy::is_active = false;

#endif

namespace std {
template <typename T>
struct hash<std::reference_wrapper<T>> {
    std::size_t operator()( const std::reference_wrapper<T>& wr ) const {
        using type = typename std::remove_const<typename std::remove_const<T>::type>::type;
        return std::hash<type>()(wr.get());
    }
};

template <typename T>
struct hash<std::weak_ptr<T>> {
    std::size_t operator()( const std::weak_ptr<T>& wr ) const {
        return std::hash<T>()(*wr.lock().get());
    }
};

#if TBB_USE_EXCEPTIONS
template <>
struct hash<ThrowOnCopy> {
    std::size_t operator()( const ThrowOnCopy& ) const {
        return 1;
    }
};
#endif

template <typename T>
struct equal_to<std::weak_ptr<T>> {
    std::size_t operator()( const std::weak_ptr<T>& rhs, const std::weak_ptr<T>& lhs ) const {
        return *rhs.lock().get() == *lhs.lock().get();
    }
};

} // namespace std

#endif // __TBB_test_common_containers_common_H
