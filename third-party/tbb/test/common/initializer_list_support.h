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

#ifndef __TBB_test_common_initializer_list_support_H
#define __TBB_test_common_initializer_list_support_H

#include "config.h"

#include <initializer_list>
#include <vector>
#include <type_traits>

namespace initializer_list_support_tests {

template <typename ContainerType, typename ElementType>
void test_ctor( std::initializer_list<ElementType> init, const ContainerType& expected ) {
    ContainerType cont(init);
    REQUIRE_MESSAGE(cont == expected, "Initialization via initializer_list failed");
}

template <typename ContainerType, typename ElementType>
void test_assignment_operator( std::initializer_list<ElementType> init, const ContainerType& expected ) {
    ContainerType cont;
    static_assert(std::is_same< decltype(cont = init), ContainerType& >::value == true,
        "ContainerType::operator=(std::intializer_list) must return ContainerType&");

    cont = init;
    REQUIRE_MESSAGE(cont == expected, "Assignment from the initializer_list failed");
}

struct SkippedTest {
    template <typename ContainerType, typename ElementType>
    static void test( std::initializer_list<ElementType>, const ContainerType& ) {}
}; // struct SkippedTest

struct TestAssignMethod {
    template <typename ContainerType, typename ElementType>
    static void test( std::initializer_list<ElementType> init, const ContainerType& expected) {
        ContainerType cont;
        cont.assign(init);
        REQUIRE_MESSAGE(cont == expected, "assign method with the initializer list argument failed");
    }
}; // struct TestAssign

struct TestInsertMethod {
    template <typename ContainerType, typename ElementType>
    static void test( std::initializer_list<ElementType> init, const ContainerType& expected) {
        ContainerType cont;
        cont.insert(init);
        REQUIRE_MESSAGE(cont == expected, "insert method with the initializer list argument failed");
    }
}; // struct TestInsertMethod

template <typename ContainerType, typename TestAssign, typename TestSpecial>
void test_initializer_list_support( std::initializer_list<typename ContainerType::value_type> init ) {
    using element_type = typename ContainerType::value_type;
    std::vector<element_type> test_seq(init);
    ContainerType expected(test_seq.begin(), test_seq.end());

    test_ctor(init, expected);
    test_assignment_operator(init, expected);
    TestAssign::test(init, expected);
    TestSpecial::test(init, expected);
}

template <typename ContainerType, typename TestSpecial = SkippedTest>
void test_initializer_list_support( std::initializer_list<typename ContainerType::value_type> init ) {
    test_initializer_list_support<ContainerType, TestAssignMethod, TestSpecial>(init);
}

template <typename ContainerType, typename TestSpecial = SkippedTest>
void test_initializer_list_support_without_assign( std::initializer_list<typename ContainerType::value_type> init ) {
    test_initializer_list_support<ContainerType, SkippedTest, TestSpecial>(init);
}

} // namespace initializer_list_support_tests

#endif // __TBB_test_common_initializer_list_support_H
