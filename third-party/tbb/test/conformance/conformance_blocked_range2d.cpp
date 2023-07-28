/*
    Copyright (c) 2005-2022 Intel Corporation

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

#include "common/test.h"
#include "common/utils.h"
#include "common/utils_assert.h"
#include "common/utils_concurrency_limit.h"

#include "oneapi/tbb/blocked_range2d.h"
#include "oneapi/tbb/parallel_for.h"
#include "oneapi/tbb/global_control.h"

//! \file conformance_blocked_range2d.cpp
//! \brief Test for [algorithms.blocked_range2d] specification

template<typename Tag>
class AbstractValueType {
    AbstractValueType() {}
    int value;
public:
    template<typename OtherTag>
    friend AbstractValueType<OtherTag> MakeAbstractValueType( int i );

    template<typename OtherTag>
    friend int GetValueOf( const AbstractValueType<OtherTag>& v ) ;
};

template<typename Tag>
AbstractValueType<Tag> MakeAbstractValueType( int i ) {
    AbstractValueType<Tag> x;
    x.value = i;
    return x;
}

template<typename Tag>
int GetValueOf( const AbstractValueType<Tag>& v ) {return v.value;}

template<typename Tag>
bool operator<( const AbstractValueType<Tag>& u, const AbstractValueType<Tag>& v ) {
    return GetValueOf(u)<GetValueOf(v);
}

template<typename Tag>
std::size_t operator-( const AbstractValueType<Tag>& u, const AbstractValueType<Tag>& v ) {
    return GetValueOf(u)-GetValueOf(v);
}

template<typename Tag>
AbstractValueType<Tag> operator+( const AbstractValueType<Tag>& u, std::size_t offset ) {
    return MakeAbstractValueType<Tag>(GetValueOf(u)+int(offset));
}

struct RowTag {};
struct ColTag {};

static void SerialTest() {
    typedef AbstractValueType<RowTag> row_type;
    typedef AbstractValueType<ColTag> col_type;
    typedef oneapi::tbb::blocked_range2d<row_type,col_type> range_type;
    for( int row_x=-10; row_x<10; ++row_x ) {
        for( int row_y=row_x; row_y<10; ++row_y ) {
            row_type row_i = MakeAbstractValueType<RowTag>(row_x);
            row_type row_j = MakeAbstractValueType<RowTag>(row_y);
            for( int row_grain=1; row_grain<10; ++row_grain ) {
                for( int col_x=-10; col_x<10; ++col_x ) {
                    for( int col_y=col_x; col_y<10; ++col_y ) {
                        col_type col_i = MakeAbstractValueType<ColTag>(col_x);
                        col_type col_j = MakeAbstractValueType<ColTag>(col_y);
                        for( int col_grain=1; col_grain<10; ++col_grain ) {
                            range_type r( row_i, row_j, row_grain, col_i, col_j, col_grain );
                            utils::AssertSameType( r.is_divisible(), true );
                            utils::AssertSameType( r.empty(), true );
                            utils::AssertSameType( static_cast<range_type::row_range_type::const_iterator*>(nullptr), static_cast<row_type*>(nullptr) );
                            utils::AssertSameType( static_cast<range_type::col_range_type::const_iterator*>(nullptr), static_cast<col_type*>(nullptr) );
                            utils::AssertSameType( r.rows(), oneapi::tbb::blocked_range<row_type>( row_i, row_j, 1 ));
                            utils::AssertSameType( r.cols(), oneapi::tbb::blocked_range<col_type>( col_i, col_j, 1 ));
                            REQUIRE( r.empty()==(row_x==row_y||col_x==col_y) );
                            REQUIRE( r.is_divisible()==(row_y-row_x>row_grain||col_y-col_x>col_grain) );
                            if( r.is_divisible() ) {
                                range_type r2(r,oneapi::tbb::split());
                                if( GetValueOf(r2.rows().begin())==GetValueOf(r.rows().begin()) ) {
                                    REQUIRE( GetValueOf(r2.rows().end())==GetValueOf(r.rows().end()) );
                                    REQUIRE( GetValueOf(r2.cols().begin())==GetValueOf(r.cols().end()) );
                                } else {
                                    REQUIRE( GetValueOf(r2.cols().end())==GetValueOf(r.cols().end()) );
                                    REQUIRE( GetValueOf(r2.rows().begin())==GetValueOf(r.rows().end()) );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

const int N = 1<<10;

unsigned char Array[N][N];

struct Striker {
   // Note: we use <int> here instead of <long> in order to test for problems similar to Quad 407676
    void operator()( const oneapi::tbb::blocked_range2d<int>& r ) const {
        for( oneapi::tbb::blocked_range<int>::const_iterator i=r.rows().begin(); i!=r.rows().end(); ++i )
            for( oneapi::tbb::blocked_range<int>::const_iterator j=r.cols().begin(); j!=r.cols().end(); ++j )
                ++Array[i][j];
    }
};

void ParallelTest() {
    for( int i=0; i<N; i=i<3 ? i+1 : i*3 ) {
        for( int j=0; j<N; j=j<3 ? j+1 : j*3 ) {
            const oneapi::tbb::blocked_range2d<int> r( 0, i, 7, 0, j, 5 );
            oneapi::tbb::parallel_for( r, Striker() );
            for( int k=0; k<N; ++k ) {
                for( int l=0; l<N; ++l ) {
                    if( Array[k][l] != (k<i && l<j) ) REQUIRE(false);
                    Array[k][l] = 0;
                }
            }
        }
    }
}

//! Testing blocked_range2d interface
//! \brief \ref interface \ref requirement
TEST_CASE("Serial test") {
    SerialTest();
}

//! Testing blocked_range2d interface with parallel_for
//! \brief \ref requirement
TEST_CASE("Parallel test") {
    for ( auto concurrency_level : utils::concurrency_range() ) {
        oneapi::tbb::global_control control(oneapi::tbb::global_control::max_allowed_parallelism, concurrency_level);
        ParallelTest();
    }
}

//! Testing blocked_range2d with proportional splitting
//! \brief \ref interface \ref requirement
TEST_CASE("blocked_range2d proportional splitting") {
    oneapi::tbb::blocked_range2d<int> original(0, 100, 0, 100);
    oneapi::tbb::blocked_range2d<int> first(original);
    oneapi::tbb::proportional_split ps(3, 1);
    oneapi::tbb::blocked_range2d<int> second(first, ps);

    int expected_first_end = static_cast<int>(
        original.rows().begin() + ps.left() * (original.rows().end() - original.rows().begin()) / (ps.left() + ps.right())
    );
    if (first.rows().size() == second.rows().size()) {
        // Splitting was made by cols
        utils::check_range_bounds_after_splitting(original.cols(), first.cols(), second.cols(), expected_first_end);
    } else {
        // Splitting was made by rows
        utils::check_range_bounds_after_splitting(original.rows(), first.rows(), second.rows(), expected_first_end);
    }
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
//! Testing blocked_range2d deduction guides
//! \brief \ref interface
TEST_CASE("Deduction guides") {
    std::vector<const unsigned long *> v;
    std::vector<double> v2;

    // check blocked_range2d(RowValue, RowValue, size_t, ColValue, ColValue, size_t)
    oneapi::tbb::blocked_range2d r1(v.begin(), v.end(), 2, v2.begin(), v2.end(), 2);
    static_assert(std::is_same<decltype(r1), oneapi::tbb::blocked_range2d<decltype(v)::iterator, decltype(v2)::iterator>>::value);

    // check blocked_range2d(blocked_range2d &)
    oneapi::tbb::blocked_range2d r2(r1);
    static_assert(std::is_same<decltype(r2), decltype(r1)>::value);

    // check blocked_range2d(blocked_range2d &&)
    oneapi::tbb::blocked_range2d r3(std::move(r1));
    static_assert(std::is_same<decltype(r3), decltype(r1)>::value);
}
#endif

