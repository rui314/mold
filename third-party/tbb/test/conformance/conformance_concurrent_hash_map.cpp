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

#include <common/test.h>
#include <common/utils.h>
#include "common/utils_report.h"
#include <common/spin_barrier.h>
#include <common/state_trackable.h>
#include <common/container_move_support.h>
#include <common/containers_common.h>
#include <common/initializer_list_support.h>
#include <common/vector_types.h>
#include <common/test_comparisons.h>
#include "oneapi/tbb/concurrent_hash_map.h"
#include "oneapi/tbb/global_control.h"
#include "oneapi/tbb/parallel_for.h"

//! \file conformance_concurrent_hash_map.cpp
//! \brief Test for [containers.concurrent_hash_map containers.tbb_hash_compare] specification

/** Has tightly controlled interface so that we can verify
    that concurrent_hash_map uses only the required interface. */
class MyException : public std::bad_alloc {
public:
    virtual const char *what() const throw() override { return "out of items limit"; }
    virtual ~MyException() throw() {}
};

/** Has tightly controlled interface so that we can verify
    that concurrent_hash_map uses only the required interface. */
class MyKey {
private:
    int key;
    friend class MyHashCompare;
    friend class YourHashCompare;
public:
    MyKey() = default;
    MyKey( const MyKey& ) = default;
    void operator=( const MyKey&  ) = delete;
    static MyKey make( int i ) {
        MyKey result;
        result.key = i;
        return result;
    }

    int value_of() const { return key; }
};

std::atomic<long> MyDataCount;
long MyDataCountLimit = 0;

class MyData {
protected:
    friend class MyData2;
    int data;
    enum state_t {
        LIVE=0x1234,
        DEAD=0x5678
    } my_state;
    void operator=( const MyData& );    // Deny access
public:
    MyData(int i = 0) {
        my_state = LIVE;
        data = i;
        if (MyDataCountLimit && MyDataCount + 1 >= MyDataCountLimit) {
            TBB_TEST_THROW(MyException{});
        }
        ++MyDataCount;
    }

    MyData( const MyData& other ) {
        CHECK_FAST(other.my_state==LIVE);
        my_state = LIVE;
        data = other.data;
        if(MyDataCountLimit && MyDataCount + 1 >= MyDataCountLimit) {
            TBB_TEST_THROW(MyException{});
        }
        ++MyDataCount;
    }

    ~MyData() {
        --MyDataCount;
        my_state = DEAD;
    }

    static MyData make( int i ) {
        MyData result;
        result.data = i;
        return result;
    }

    int value_of() const {
        CHECK_FAST(my_state==LIVE);
        return data;
    }

    void set_value( int i ) {
        CHECK_FAST(my_state==LIVE);
        data = i;
    }

    bool operator==( const MyData& other ) const {
        CHECK_FAST(other.my_state==LIVE);
        CHECK_FAST(my_state==LIVE);
        return data == other.data;
    }
};

class MyData2 : public MyData {
public:
    MyData2( ) {}

    MyData2( const MyData2& other ) : MyData() {
        CHECK_FAST(other.my_state==LIVE);
        CHECK_FAST(my_state==LIVE);
        data = other.data;
    }

    MyData2( const MyData& other ) {
        CHECK_FAST(other.my_state==LIVE);
        CHECK_FAST(my_state==LIVE);
        data = other.data;
    }

    void operator=( const MyData& other ) {
        CHECK_FAST(other.my_state==LIVE);
        CHECK_FAST(my_state==LIVE);
        data = other.data;
    }

    void operator=( const MyData2& other ) {
        CHECK_FAST(other.my_state==LIVE);
        CHECK_FAST(my_state==LIVE);
        data = other.data;
    }

    bool operator==( const MyData2& other ) const {
        CHECK_FAST(other.my_state==LIVE);
        CHECK_FAST(my_state==LIVE);
        return data == other.data;
    }
};

class MyHashCompare {
public:
    bool equal( const MyKey& j, const MyKey& k ) const {
        return j.key==k.key;
    }

    std::size_t hash( const MyKey& k ) const {
        return k.key;
    }
};

class YourHashCompare {
public:
    bool equal( const MyKey& j, const MyKey& k ) const {
        return j.key==k.key;
    }

    std::size_t hash( const MyKey& ) const {
        return 1;
    }
};

using test_allocator_type = StaticSharedCountingAllocator<std::allocator<std::pair<const MyKey, MyData>>>;
using test_table_type = oneapi::tbb::concurrent_hash_map<MyKey, MyData, MyHashCompare, test_allocator_type>;
using other_test_table_type = oneapi::tbb::concurrent_hash_map<MyKey, MyData2, MyHashCompare>;

template <template <typename...> class ContainerType>
void test_member_types() {
    using container_type = ContainerType<int, int>;
    static_assert(std::is_same<typename container_type::allocator_type, oneapi::tbb::tbb_allocator<std::pair<const int, int>>>::value,
                  "Incorrect default template allocator");

    static_assert(std::is_same<typename container_type::key_type, int>::value,
                  "Incorrect container key_type member type");
    static_assert(std::is_same<typename container_type::value_type, std::pair<const int, int>>::value,
                  "Incorrect container value_type member type");

    static_assert(std::is_unsigned<typename container_type::size_type>::value,
                  "Incorrect container size_type member type");
    static_assert(std::is_signed<typename container_type::difference_type>::value,
                  "Incorrect container difference_type member type");

    using value_type = typename container_type::value_type;
    static_assert(std::is_same<typename container_type::reference, value_type&>::value,
                  "Incorrect container reference member type");
    static_assert(std::is_same<typename container_type::const_reference, const value_type&>::value,
                  "Incorrect container const_reference member type");
    using allocator_type = typename container_type::allocator_type;
    static_assert(std::is_same<typename container_type::pointer, typename std::allocator_traits<allocator_type>::pointer>::value,
                  "Incorrect container pointer member type");
    static_assert(std::is_same<typename container_type::const_pointer, typename std::allocator_traits<allocator_type>::const_pointer>::value,
                  "Incorrect container const_pointer member type");

    static_assert(utils::is_forward_iterator<typename container_type::iterator>::value,
                  "Incorrect container iterator member type");
    static_assert(!std::is_const<typename container_type::iterator::value_type>::value,
                  "Incorrect container iterator member type");
    static_assert(utils::is_forward_iterator<typename container_type::const_iterator>::value,
                  "Incorrect container const_iterator member type");
    static_assert(std::is_const<typename container_type::const_iterator::value_type>::value,
                  "Incorrect container iterator member type");
}

template<typename test_table_type>
void FillTable( test_table_type& x, int n ) {
    for( int i=1; i<=n; ++i ) {
        MyKey key( MyKey::make(-i) ); // hash values must not be specified in direct order
        typename test_table_type::accessor a;
        bool b = x.insert(a,key);
        CHECK_FAST(b);
        a->second.set_value( i*i );
    }
}

template<typename test_table_type>
static void CheckTable( const test_table_type& x, int n ) {
    REQUIRE_MESSAGE( x.size()==size_t(n), "table is different size than expected" );
    CHECK(x.empty()==(n==0));
    CHECK(x.size()<=x.max_size());
    for( int i=1; i<=n; ++i ) {
        MyKey key( MyKey::make(-i) );
        typename test_table_type::const_accessor a;
        bool b = x.find(a,key);
        CHECK_FAST(b);
        CHECK_FAST(a->second.value_of()==i*i);
    }
    int count = 0;
    int key_sum = 0;
    for( typename test_table_type::const_iterator i(x.begin()); i!=x.end(); ++i ) {
        ++count;
        key_sum += -i->first.value_of();
    }
    CHECK(count==n);
    CHECK(key_sum==n*(n+1)/2);
}

void TestCopy() {
    INFO("testing copy\n");
    test_table_type t1;
    for( int i=0; i<10000; i=(i<100 ? i+1 : i*3) ) {
        MyDataCount = 0;

        FillTable(t1,i);
        // Do not call CheckTable(t1,i) before copying, it enforces rehashing

        test_table_type t2(t1);
        // Check that copy constructor did not mangle source table.
        CheckTable(t1,i);
        swap(t1, t2);
        CheckTable(t1,i);
        CHECK(!(t1 != t2));

        // Clear original table
        t2.clear();
        swap(t2, t1);
        CheckTable(t1,0);

        // Verify that copy of t1 is correct, even after t1 is cleared.
        CheckTable(t2,i);
        t2.clear();
        t1.swap( t2 );
        CheckTable(t1,0);
        CheckTable(t2,0);
        REQUIRE_MESSAGE( MyDataCount==0, "data leak?" );
    }
}

void TestRehash() {
    INFO("testing rehashing\n");
    test_table_type w;
    w.insert( std::make_pair(MyKey::make(-5), MyData()) );
    w.rehash(); // without this, assertion will fail
    test_table_type::iterator it = w.begin();
    int i = 0; // check for non-rehashed buckets
    for( ; it != w.end(); i++ )
        w.count( (it++)->first );
    CHECK(i == 1);
    for( i=0; i<1000; i=(i<29 ? i+1 : i*2) ) {
        for( int j=std::max(256+i, i*2); j<10000; j*=3 ) {
            test_table_type v;
            FillTable( v, i );
            CHECK(int(v.size()) == i);
            CHECK(int(v.bucket_count()) <= j);
            v.rehash( j );
            CHECK(int(v.bucket_count()) >= j);
            CheckTable( v, i );
        }
    }
}

void TestAssignment() {
    INFO("testing assignment\n");
    oneapi::tbb::concurrent_hash_map<int, int> test_map({{1, 2}, {2, 4}});
    test_map.operator=(test_map); // suppress self assign warning
    CHECK(!test_map.empty());

    for( int i=0; i<1000; i=(i<30 ? i+1 : i*5) ) {
        for( int j=0; j<1000; j=(j<30 ? j+1 : j*7) ) {
            test_table_type t1;
            test_table_type t2;
            FillTable(t1,i);
            FillTable(t2,j);
            CHECK((t1 == t2) == (i == j));
            CheckTable(t2,j);

            test_table_type& tref = t2=t1;
            CHECK(&tref==&t2);
            CHECK(t1 == t2);
            CheckTable(t1,i);
            CheckTable(t2,i);

            t1.clear();
            CheckTable(t1,0);
            CheckTable(t2,i);
            REQUIRE_MESSAGE( MyDataCount==i, "data leak?" );

            t2.clear();
            CheckTable(t1,0);
            CheckTable(t2,0);
            REQUIRE_MESSAGE( MyDataCount==0, "data leak?" );
        }
    }
}

template<typename Iterator, typename T>
void TestIteratorTraits() {
    T x;
    typename Iterator::reference xr = x;
    typename Iterator::pointer xp = &x;
    CHECK(&xr==xp);
}

template<typename Iterator1, typename Iterator2>
void TestIteratorAssignment( Iterator2 j ) {
    Iterator1 i(j), k;
    CHECK(i==j);
    CHECK(!(i!=j));
    k = j;
    CHECK(k==j);
    CHECK(!(k!=j));
}

template<typename Range1, typename Range2>
void TestRangeAssignment( Range2 r2 ) {
    Range1 r1(r2); r1 = r2;
}

void TestIteratorsAndRanges() {
    INFO("testing iterators compliance\n");
    TestIteratorTraits<test_table_type::iterator,test_table_type::value_type>();
    TestIteratorTraits<test_table_type::const_iterator,const test_table_type::value_type>();

    test_table_type v;
    CHECK(v.range().grainsize() == 1);
    test_table_type const &u = v;

    TestIteratorAssignment<test_table_type::const_iterator>( u.begin() );
    TestIteratorAssignment<test_table_type::const_iterator>( v.begin() );
    TestIteratorAssignment<test_table_type::iterator>( v.begin() );
    // doesn't compile as expected: TestIteratorAssignment<typename V::iterator>( u.begin() );

    // check for non-existing
    CHECK(v.equal_range(MyKey::make(-1)) == std::make_pair(v.end(), v.end()));
    CHECK(u.equal_range(MyKey::make(-1)) == std::make_pair(u.end(), u.end()));

    INFO("testing ranges compliance\n");
    TestRangeAssignment<test_table_type::const_range_type>( u.range() );
    TestRangeAssignment<test_table_type::range_type>( v.range() );
    // doesn't compile as expected: TestRangeAssignment<typename V::range_type>( u.range() );

    INFO("testing construction and insertion from iterators range\n");
    FillTable( v, 1000 );
    other_test_table_type t(v.begin(), v.end());
    v.rehash();
    CheckTable(t, 1000);
    t.insert(v.begin(), v.end()); // do nothing
    CheckTable(t, 1000);
    t.clear();
    t.insert(v.begin(), v.end()); // restore
    CheckTable(t, 1000);

    INFO("testing comparison\n");
    using test_allocator_type2 = StaticSharedCountingAllocator<std::allocator<std::pair<const MyKey, MyData2>>>;
    using YourTable1 = oneapi::tbb::concurrent_hash_map<MyKey,MyData2,YourHashCompare, test_allocator_type2>;
    using YourTable2 = oneapi::tbb::concurrent_hash_map<MyKey,MyData2,YourHashCompare>;
    YourTable1 t1;
    FillTable( t1, 10 );
    CheckTable(t1, 10 );
    YourTable2 t2(t1.begin(), t1.end());
    MyKey key( MyKey::make(-5) ); MyData2 data;
    CHECK(t2.erase(key));
    YourTable2::accessor a;
    CHECK(t2.insert(a, key));
    data.set_value(0);   a->second = data;
    CHECK(t1 != t2);
    data.set_value(5*5); a->second = data;
    CHECK(t1 == t2);
}

struct test_insert {
    template<typename container_type, typename element_type>
    static void test( std::initializer_list<element_type> il, container_type const& expected ) {
        container_type vd;
        vd.insert( il );
        REQUIRE_MESSAGE( vd == expected, "inserting with an initializer list failed" );
    }
};

struct ctor_test {
 template<typename container_type, typename element_type>
    static void test( std::initializer_list<element_type> il, container_type const& expected ) {
        container_type vd(il, tbb::tbb_allocator<std::pair<element_type, element_type>>{});
        REQUIRE_MESSAGE( vd == expected, "inserting with an initializer list failed" );
    }
};

void TestInitList(){
    using namespace initializer_list_support_tests;
    INFO("testing initializer_list methods \n");

    using ch_map_type = oneapi::tbb::concurrent_hash_map<int,int>;
    std::initializer_list<ch_map_type::value_type> pairs_il = {{1,1},{2,2},{3,3},{4,4},{5,5}};

    test_initializer_list_support_without_assign<ch_map_type, test_insert>( pairs_il );
    test_initializer_list_support_without_assign<ch_map_type, test_insert>( {} );
    test_initializer_list_support_without_assign<ch_map_type, ctor_test>(pairs_il);
}

template <typename base_alloc_type>
class only_node_counting_allocator : public StaticSharedCountingAllocator<base_alloc_type> {
    using base_type = StaticSharedCountingAllocator<base_alloc_type>;
    using base_traits = oneapi::tbb::detail::allocator_traits<base_alloc_type>;
public:
    template<typename U>
    struct rebind {
        using other = only_node_counting_allocator<typename base_traits::template rebind_alloc<U>>;
    };

    only_node_counting_allocator() : base_type() {}
    only_node_counting_allocator(const only_node_counting_allocator& a) : base_type(a) {}

    template<typename U>
    only_node_counting_allocator(const only_node_counting_allocator<U>& a) : base_type(a) {}

    typename base_type::value_type* allocate(const std::size_t n) {
        if ( n > 1) {
            return base_alloc_type::allocate(n);
        } else {
            return base_type::allocate(n);
        }
    }
};

#if TBB_USE_EXCEPTIONS
void TestExceptions() {
    using allocator_type = only_node_counting_allocator<oneapi::tbb::tbb_allocator<std::pair<const MyKey, MyData2>>>;
    using throwing_table = oneapi::tbb::concurrent_hash_map<MyKey, MyData2, MyHashCompare, allocator_type>;
    enum methods {
        zero_method = 0,
        ctor_copy, op_assign, op_insert,
        all_methods
    };

    INFO("testing exception-safety guarantees\n");
    throwing_table src;
    FillTable( src, 1000 );
    CHECK(MyDataCount==1000);

    try {
        for(int t = 0; t < 2; t++) // exception type
        for(int m = zero_method+1; m < all_methods; m++)
        {
            allocator_type a;
            allocator_type::init_counters();
            if(t) MyDataCountLimit = 101;
            else a.set_limits(101);
            throwing_table victim(a);
            MyDataCount = 0;

            try {
                switch(m) {
                case ctor_copy: {
                        throwing_table acopy(src, a);
                    } break;
                case op_assign: {
                        victim = src;
                    } break;
                case op_insert: {
                        // Insertion in cpp11 don't make copy constructions
                        // during the insertion, so we need to decrement limit
                        // to throw an exception in the right place and to prevent
                        // successful insertion of one unexpected item
                        if (MyDataCountLimit)
                            --MyDataCountLimit;
                        FillTable( victim, 1000 );
                    } break;
                default:;
                }
                REQUIRE_MESSAGE(false, "should throw an exception");
            } catch(std::bad_alloc &e) {
                MyDataCountLimit = 0;
                size_t size = victim.size();
                switch(m) {
                case op_assign:
                    REQUIRE_MESSAGE( MyDataCount==100, "data leak?" );
                    CHECK(size>=100);
                    utils_fallthrough;
                case ctor_copy:
                    CheckTable(src, 1000);
                    break;
                case op_insert:
                    CHECK(size==size_t(100-t));
                    REQUIRE_MESSAGE( MyDataCount==100-t, "data leak?" );
                    CheckTable(victim, 100-t);
                    break;

                default:; // nothing to check here
                }
                INFO("Exception "<< m << " : " << e.what() << "- ok ()");
            }
            catch ( ... ) {
                REQUIRE_MESSAGE( false, "Unrecognized exception" );
            }
        }
    } catch(...) {
        REQUIRE_MESSAGE(false, "unexpected exception");
    }
    src.clear(); MyDataCount = 0;
    allocator_type::max_items = 0;
}
#endif

struct default_container_traits {
    template <typename container_type, typename iterator_type>
    static container_type& construct_container(typename std::aligned_storage<sizeof(container_type)>::type& storage, iterator_type begin, iterator_type end){
        container_type* ptr = reinterpret_cast<container_type*>(&storage);
        new (ptr) container_type(begin, end);
        return *ptr;
    }

    template <typename container_type, typename iterator_type, typename allocator_type>
    static container_type& construct_container(typename std::aligned_storage<sizeof(container_type)>::type& storage, iterator_type begin, iterator_type end, allocator_type const& a){
        container_type* ptr = reinterpret_cast<container_type*>(&storage);
        new (ptr) container_type(begin, end, a);
        return *ptr;
    }
};

struct hash_map_traits : default_container_traits {
    enum{ expected_number_of_items_to_allocate_for_steal_move = 0 };

    template<typename T>
    struct hash_compare {
        bool equal( const T& lhs, const T& rhs ) const {
            return lhs==rhs;
        }
        size_t hash( const T& k ) const {
            return my_hash_func(k);
        }
        std::hash<T> my_hash_func;
    };

    template <typename T, typename Allocator>
    using container_type = oneapi::tbb::concurrent_hash_map<T, T, hash_compare<T>, Allocator>;

    template <typename T>
    using container_value_type = std::pair<const T, T>;

    template<typename element_type, typename allocator_type>
    struct apply {
        using type = oneapi::tbb::concurrent_hash_map<element_type, element_type, hash_compare<element_type>, allocator_type>;
    };

    using init_iterator_type = move_support_tests::FooPairIterator;
    template <typename hash_map_type, typename iterator>
    static bool equal(hash_map_type const& c, iterator begin, iterator end){
        bool equal_sizes = ( static_cast<size_t>(std::distance(begin, end)) == c.size() );
        if (!equal_sizes)
            return false;

        for (iterator it = begin; it != end; ++it ){
            if (c.count( (*it).first) == 0){
                return false;
            }
        }
        return true;
    }
};

using DataStateTrackedTable = oneapi::tbb::concurrent_hash_map<MyKey, move_support_tests::Foo, MyHashCompare>;

struct RvalueInsert {
    static void apply( DataStateTrackedTable& table, int i ) {
        DataStateTrackedTable::accessor a;
        int next = i + 1;
        CHECK_FAST_MESSAGE((table.insert( a, std::make_pair(MyKey::make(i), move_support_tests::Foo(next)))),
            "already present while should not ?" );
        CHECK_FAST((*a).second == next);
        CHECK_FAST((*a).second.state == StateTrackableBase::MoveInitialized);
    }
};

struct Emplace {
    template <typename Accessor>
    static void apply_impl( DataStateTrackedTable& table, int i) {
        Accessor a;
        CHECK_FAST_MESSAGE((table.emplace( a, MyKey::make(i), (i + 1))),
                "already present while should not ?" );
        CHECK_FAST((*a).second == i + 1);
        CHECK_FAST((*a).second.state == StateTrackableBase::DirectInitialized);
    }

    static void apply( DataStateTrackedTable& table, int i ) {
        // TODO: investigate ability to rewrite apply methods with use apply_imp method.
        if (i % 2) {
            apply_impl<DataStateTrackedTable::accessor>(table, i);
        } else {
            apply_impl<DataStateTrackedTable::const_accessor>(table, i);
        }
    }
};

template<typename Op, typename test_table_type>
class TableOperation {
    test_table_type& my_table;
public:
    void operator()( const oneapi::tbb::blocked_range<int>& range ) const {
        for( int i=range.begin(); i!=range.end(); ++i )
            Op::apply(my_table,i);
    }
    TableOperation( test_table_type& table ) : my_table(table) {}
};

bool UseKey( size_t i ) {
    return (i&3)!=3;
}

struct Insert {
    static void apply( test_table_type& table, int i ) {
        if( UseKey(i) ) {
            if( i&4 ) {
                test_table_type::accessor a;
                table.insert( a, MyKey::make(i) );
                if( i&1 )
                    (*a).second.set_value(i*i);
                else
                    a->second.set_value(i*i);
            } else
                if( i&1 ) {
                    test_table_type::accessor a;
                    table.insert( a, std::make_pair(MyKey::make(i), MyData(i*i)) );
                    CHECK_FAST((*a).second.value_of()==i*i);
                } else {
                    test_table_type::const_accessor ca;
                    table.insert( ca, std::make_pair(MyKey::make(i), MyData(i*i)) );
                    CHECK_FAST(ca->second.value_of()==i*i);
                }
        }
    }
};

struct Find {
    static void apply( test_table_type& table, int i ) {
        test_table_type::accessor a;
        const test_table_type::accessor& ca = a;
        bool b = table.find( a, MyKey::make(i) );
        CHECK_FAST(b==!a.empty());
        if( b ) {
            if( !UseKey(i) )
                REPORT("Line %d: unexpected key %d present\n",__LINE__,i);
            CHECK_FAST(ca->second.value_of()==i*i);
            CHECK_FAST((*ca).second.value_of()==i*i);
            if( i&1 )
                ca->second.set_value( ~ca->second.value_of() );
            else
                (*ca).second.set_value( ~ca->second.value_of() );
        } else {
            if( UseKey(i) )
                REPORT("Line %d: key %d missing\n",__LINE__,i);
        }
    }
};

struct FindConst {
    static void apply( const test_table_type& table, int i ) {
        test_table_type::const_accessor a;
        const test_table_type::const_accessor& ca = a;
        bool b = table.find( a, MyKey::make(i) );
        CHECK_FAST(b==(table.count(MyKey::make(i))>0));
        CHECK_FAST(b==!a.empty());
        CHECK_FAST(b==UseKey(i));
        if( b ) {
            CHECK_FAST(ca->second.value_of()==~(i*i));
            CHECK_FAST((*ca).second.value_of()==~(i*i));
        }
    }
};

struct InsertInitList {
    static void apply( test_table_type& table, int i ) {
        if ( UseKey( i ) ) {
            // TODO: investigate why the following sequence causes an additional allocation sometimes:
            // table.insert( test_table_type::value_type( MyKey::make( i ), i*i ) );
            // table.insert( test_table_type::value_type( MyKey::make( i ), i*i+1 ) );
            std::initializer_list<test_table_type::value_type> il = {
                test_table_type::value_type( MyKey::make( i ), i*i )
                /*, test_table_type::value_type( MyKey::make( i ), i*i+1 ) */
                                                                    };
            table.insert( il );
        }
    }
};

template<typename Op, typename TableType>
void DoConcurrentOperations( TableType& table, int n, const char* what, std::size_t nthread ) {
    INFO("testing " << what << " with " << nthread << " threads");
    oneapi::tbb::parallel_for(oneapi::tbb::blocked_range<int>(0, n ,100), TableOperation<Op, TableType>(table));
}

//! Test traversing the table with an iterator.
void TraverseTable( test_table_type& table, size_t n, size_t expected_size ) {
    INFO("testing traversal\n");
    size_t actual_size = table.size();
    CHECK(actual_size==expected_size);
    size_t count = 0;
    bool* array = new bool[n];
    memset( array, 0, n*sizeof(bool) );
    const test_table_type& const_table = table;
    test_table_type::const_iterator ci = const_table.begin();
    for( test_table_type::iterator i = table.begin(); i!=table.end(); ++i ) {
        // Check iterator
        int k = i->first.value_of();
        CHECK_FAST(UseKey(k));
        CHECK_FAST((*i).first.value_of()==k);
        CHECK_FAST_MESSAGE((0<=k && size_t(k)<n), "out of bounds key" );
        CHECK_FAST_MESSAGE( !array[k], "duplicate key" );
        array[k] = true;
        ++count;

        // Check lower/upper bounds
        std::pair<test_table_type::iterator, test_table_type::iterator> er = table.equal_range(i->first);
        std::pair<test_table_type::const_iterator, test_table_type::const_iterator> cer = const_table.equal_range(i->first);
        CHECK_FAST((cer.first == er.first && cer.second == er.second));
        CHECK_FAST(cer.first == i);
        CHECK_FAST(std::distance(cer.first, cer.second) == 1);

        // Check const_iterator
        test_table_type::const_iterator cic = ci++;
        CHECK_FAST(cic->first.value_of()==k);
        CHECK_FAST((*cic).first.value_of()==k);
    }
    CHECK(ci==const_table.end());
    delete[] array;
    if (count != expected_size) {
        INFO("Line " << __LINE__ << ": count=" << count << " but should be " << expected_size);
    }
}

std::atomic<int> EraseCount;

struct Erase {
    static void apply( test_table_type& table, int i ) {
        bool b;
        if(i&4) {
            if(i&8) {
                test_table_type::const_accessor a;
                b = table.find( a, MyKey::make(i) ) && table.erase( a );
            } else {
                test_table_type::accessor a;
                b = table.find( a, MyKey::make(i) ) && table.erase( a );
            }
        } else
            b = table.erase( MyKey::make(i) );
        if( b ) ++EraseCount;
        CHECK_FAST(table.count(MyKey::make(i)) == 0);
    }
};

using YourTable = oneapi::tbb::concurrent_hash_map<MyKey, MyData, YourHashCompare>;
static const int IE_SIZE = 2;
std::atomic<YourTable::size_type> InsertEraseCount[IE_SIZE];

struct InsertErase  {
    static void apply( YourTable& table, int i ) {
        if ( i%3 ) {
            int key = i%IE_SIZE;
            if ( table.insert( std::make_pair(MyKey::make(key), MyData2()) ) )
                ++InsertEraseCount[key];
        } else {
            int key = i%IE_SIZE;
            if( i&1 ) {
                YourTable::accessor res;
                if(table.find( res, MyKey::make(key) ) && table.erase( res ) )
                    --InsertEraseCount[key];
            } else {
                YourTable::const_accessor res;
                if(table.find( res, MyKey::make(key) ) && table.erase( res ) )
                    --InsertEraseCount[key];
            }
        }
    }
};

struct InnerInsert {
    static void apply( YourTable& table, int i ) {
        YourTable::accessor a1, a2;
        if(i&1) utils::yield();
        table.insert( a1, MyKey::make(1) );
        utils::yield();
        table.insert( a2, MyKey::make(1 + (1<<30)) ); // the same chain
        table.erase( a2 ); // if erase by key it would lead to deadlock for single thread
    }
};

struct FakeExclusive {
    utils::SpinBarrier& barrier;
    YourTable& table;
    FakeExclusive(utils::SpinBarrier& b, YourTable&t) : barrier(b), table(t) {}
    void operator()( std::size_t i ) const {
        if(i) {
            YourTable::const_accessor real_ca;
            // const accessor on non-const table acquired as reader (shared)
            CHECK(table.find(real_ca,MyKey::make(1)));
            barrier.wait(); // item can be erased
            std::this_thread::sleep_for(std::chrono::milliseconds(10)); // let it enter the erase
            real_ca->second.value_of(); // check the state while holding accessor
        } else {
            YourTable::accessor fake_ca;
            const YourTable &const_table = table;
            // non-const accessor on const table acquired as reader (shared)
            CHECK(const_table.find(fake_ca,MyKey::make(1)));
            barrier.wait(); // readers acquired
            // can mistakenly remove the item while other readers still refers to it
            table.erase( fake_ca );
        }
    }
};

using AtomicByte = std::atomic<unsigned char>;

template<typename RangeType>
struct ParallelTraverseBody {
    const size_t n;
    AtomicByte* const array;
    ParallelTraverseBody( AtomicByte array_[], size_t n_ ) :
        n(n_),
        array(array_)
    {}
    void operator()( const RangeType& range ) const {
        for( typename RangeType::iterator i = range.begin(); i!=range.end(); ++i ) {
            int k = i->first.value_of();
            CHECK_FAST((0<=k && size_t(k)<n));
            ++array[k];
        }
    }
};

void Check( AtomicByte array[], size_t n, size_t expected_size ) {
    if( expected_size )
        for( size_t k=0; k<n; ++k ) {
            if( array[k] != int(UseKey(k)) ) {
                REPORT("array[%d]=%d != %d=UseKey(%d)\n",
                       int(k), int(array[k]), int(UseKey(k)), int(k));
                CHECK(false);
            }
        }
}

//! Test traversing the table with a parallel range
void ParallelTraverseTable( test_table_type& table, size_t n, size_t expected_size ) {
    INFO("testing parallel traversal\n");
    CHECK(table.size()==expected_size);
    AtomicByte* array = new AtomicByte[n];

    memset( static_cast<void*>(array), 0, n*sizeof(AtomicByte) );
    test_table_type::range_type r = table.range(10);
    oneapi::tbb::parallel_for( r, ParallelTraverseBody<test_table_type::range_type>( array, n ));
    Check( array, n, expected_size );

    const test_table_type& const_table = table;
    memset( static_cast<void*>(array), 0, n*sizeof(AtomicByte) );
    test_table_type::const_range_type cr = const_table.range(10);
    oneapi::tbb::parallel_for( cr, ParallelTraverseBody<test_table_type::const_range_type>( array, n ));
    Check( array, n, expected_size );

    delete[] array;
}

void TestInsertFindErase( std::size_t nthread ) {
    int n=250000;

    // compute m = number of unique keys
    int m = 0;
    for( int i=0; i<n; ++i )
        m += UseKey(i);
    {
        test_allocator_type alloc;
        test_allocator_type::init_counters();
        CHECK(MyDataCount==0);
        test_table_type table(alloc);
        TraverseTable(table,n,0);
        ParallelTraverseTable(table,n,0);

        for ( int i = 0; i < 2; ++i ) {
            if ( i==0 )
                DoConcurrentOperations<InsertInitList, test_table_type>( table, n, "insert(std::initializer_list)", nthread );
            else
                DoConcurrentOperations<Insert, test_table_type>( table, n, "insert", nthread );
            CHECK(MyDataCount == m);
            TraverseTable( table, n, m );
            ParallelTraverseTable( table, n, m );

            DoConcurrentOperations<Find, test_table_type>( table, n, "find", nthread );
            CHECK(MyDataCount == m);

            DoConcurrentOperations<FindConst, test_table_type>( table, n, "find(const)", nthread );
            CHECK(MyDataCount == m);

            EraseCount = 0;
            DoConcurrentOperations<Erase, test_table_type>( table, n, "erase", nthread );
            CHECK(EraseCount == m);
            CHECK(MyDataCount == 0);
            TraverseTable( table, n, 0 );

            table.clear();
        }

        if( nthread > 1 ) {
            YourTable ie_table;
            for( int i=0; i<IE_SIZE; ++i )
                InsertEraseCount[i] = 0;
            DoConcurrentOperations<InsertErase,YourTable>(ie_table,n/2,"insert_erase",nthread);
            for( int i=0; i<IE_SIZE; ++i )
                CHECK(InsertEraseCount[i]==ie_table.count(MyKey::make(i)));

            DoConcurrentOperations<InnerInsert, YourTable>(ie_table,2000,"inner insert",nthread);
            utils::SpinBarrier barrier(nthread);
            INFO("testing erase on fake exclusive accessor\n");
            utils::NativeParallelFor( nthread, FakeExclusive(barrier, ie_table));
        }
    }
    REQUIRE( test_allocator_type::items_constructed == test_allocator_type::items_destroyed );
    REQUIRE( test_allocator_type::items_allocated == test_allocator_type::items_freed );
    REQUIRE( test_allocator_type::allocations == test_allocator_type::frees );
}

std::atomic<int> Counter;

class AddToTable {
    test_table_type& my_table;
    const std::size_t my_nthread;
    const int my_m;
public:
    AddToTable( test_table_type& table, std::size_t nthread, int m ) : my_table(table), my_nthread(nthread), my_m(m) {}
    void operator()( std::size_t ) const {
        for( int i=0; i<my_m; ++i ) {
            // Busy wait to synchronize threads
            int j = 0;
            while( Counter<i ) {
                if( ++j==1000000 ) {
                    // If Counter<i after a million iterations, then we almost surely have
                    // more logical threads than physical threads, and should yield in
                    // order to let suspended logical threads make progress.
                    j = 0;
                    utils::yield();
                }
            }
            // Now all threads attempt to simultaneously insert a key.
            int k;
            {
                test_table_type::accessor a;
                MyKey key = MyKey::make(i);
                if( my_table.insert( a, key ) )
                    a->second.set_value( 1 );
                else
                    a->second.set_value( a->second.value_of()+1 );
                k = a->second.value_of();
            }
            if( std::size_t(k) == my_nthread )
                Counter=i+1;
        }
    }
};

class RemoveFromTable {
    test_table_type& my_table;
    const int my_m;
public:
    RemoveFromTable( test_table_type& table, int m ) : my_table(table), my_m(m) {}
    void operator()(std::size_t) const {
        for( int i=0; i<my_m; ++i ) {
            bool b;
            if(i&4) {
                if(i&8) {
                    test_table_type::const_accessor a;
                    b = my_table.find( a, MyKey::make(i) ) && my_table.erase( a );
                } else {
                    test_table_type::accessor a;
                    b = my_table.find( a, MyKey::make(i) ) && my_table.erase( a );
                }
            } else
                b = my_table.erase( MyKey::make(i) );
            if( b ) ++EraseCount;
        }
    }
};

void TestConcurrency( std::size_t nthread ) {
    INFO("testing multiple insertions/deletions of same key with " << nthread << " threads");
    test_allocator_type::init_counters();
    {
        CHECK( MyDataCount==0);
        test_table_type table;
        const int m = 1000;
        Counter = 0;
        oneapi::tbb::tick_count t0 = oneapi::tbb::tick_count::now();
        utils::NativeParallelFor( nthread, AddToTable(table,nthread,m) );
        REQUIRE_MESSAGE( MyDataCount==m, "memory leak detected" );

        EraseCount = 0;
        t0 = oneapi::tbb::tick_count::now();
        utils::NativeParallelFor( nthread, RemoveFromTable(table,m) );
        REQUIRE_MESSAGE(MyDataCount==0, "memory leak detected");
        REQUIRE_MESSAGE(EraseCount==m, "return value of erase() is broken");

    }
    REQUIRE( test_allocator_type::items_constructed == test_allocator_type::items_destroyed );
    REQUIRE( test_allocator_type::items_allocated == test_allocator_type::items_freed );
    REQUIRE( test_allocator_type::allocations == test_allocator_type::frees );
    REQUIRE_MESSAGE(MyDataCount==0, "memory leak detected");
}

template<typename Key>
struct non_default_constructible_hash_compare : oneapi::tbb::detail::d1::tbb_hash_compare<Key> {
    non_default_constructible_hash_compare() {
        REQUIRE_MESSAGE(false, "Hash compare object must not default construct during the construction of hash_map with compare argument");
    }

    non_default_constructible_hash_compare(int) {}
};

void TestHashCompareConstructors() {
    using key_type = int;
    using map_type = oneapi::tbb::concurrent_hash_map<key_type, key_type, non_default_constructible_hash_compare<key_type>>;

    non_default_constructible_hash_compare<key_type> compare(0);
    map_type::allocator_type allocator;

    map_type map1(compare);
    map_type map2(compare, allocator);

    map_type map3(1, compare);
    map_type map4(1, compare, allocator);

    std::vector<map_type::value_type> reference_vector;
    map_type map5(reference_vector.begin(), reference_vector.end(), compare);
    map_type map6(reference_vector.begin(), reference_vector.end(), compare, allocator);

    map_type map7({}, compare);
    map_type map8({}, compare, allocator);
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
template <typename T>
struct debug_hash_compare : public oneapi::tbb::detail::d1::tbb_hash_compare<T> {};

template <template <typename...> typename TMap>
void TestDeductionGuides() {
    using Key = int;
    using Value = std::string;

    using ComplexType = std::pair<Key, Value>;
    using ComplexTypeConst = std::pair<const Key, Value>;

    using DefaultCompare = oneapi::tbb::detail::d1::tbb_hash_compare<Key>;
    using Compare = debug_hash_compare<Key>;
    using DefaultAllocator = oneapi::tbb::tbb_allocator<ComplexTypeConst>;
    using Allocator = std::allocator<ComplexTypeConst>;

    std::vector<ComplexType> v;
    auto l = { ComplexTypeConst(1, "one"), ComplexTypeConst(2, "two") };
    Compare compare;
    Allocator allocator;

    // check TMap(InputIterator, InputIterator)
    TMap m1(v.begin(), v.end());
    static_assert(std::is_same<decltype(m1), TMap<Key, Value, DefaultCompare, DefaultAllocator>>::value);

    // check TMap(InputIterator, InputIterator, HashCompare)
    TMap m2(v.begin(), v.end(), compare);
    static_assert(std::is_same<decltype(m2), TMap<Key, Value, Compare>>::value);

    // check TMap(InputIterator, InputIterator, HashCompare, Allocator)
    TMap m3(v.begin(), v.end(), compare, allocator);
    static_assert(std::is_same<decltype(m3), TMap<Key, Value, Compare, Allocator>>::value);

    // check TMap(InputIterator, InputIterator, Allocator)
    TMap m4(v.begin(), v.end(), allocator);
    static_assert(std::is_same<decltype(m4), TMap<Key, Value, DefaultCompare, Allocator>>::value);

    // check TMap(std::initializer_list)
    TMap m5(l);
    static_assert(std::is_same<decltype(m5), TMap<Key, Value, DefaultCompare, DefaultAllocator>>::value);

    // check TMap(std::initializer_list, HashCompare)
    TMap m6(l, compare);
    static_assert(std::is_same<decltype(m6), TMap<Key, Value, Compare, DefaultAllocator>>::value);

    // check TMap(std::initializer_list, HashCompare, Allocator)
    TMap m7(l, compare, allocator);
    static_assert(std::is_same<decltype(m7), TMap<Key, Value, Compare, Allocator>>::value);

    // check TMap(std::initializer_list, Allocator)
    TMap m8(l, allocator);
    static_assert(std::is_same<decltype(m8), TMap<Key, Value, DefaultCompare, Allocator>>::value);

    // check TMap(TMap &)
    TMap m9(m1);
    static_assert(std::is_same<decltype(m9), decltype(m1)>::value);

    // check TMap(TMap &, Allocator)
    TMap m10(m4, allocator);
    static_assert(std::is_same<decltype(m10), decltype(m4)>::value);

    // check TMap(TMap &&)
    TMap m11(std::move(m1));
    static_assert(std::is_same<decltype(m11), decltype(m1)>::value);

    // check TMap(TMap &&, Allocator)
    TMap m12(std::move(m4), allocator);
    static_assert(std::is_same<decltype(m12), decltype(m4)>::value);
}
#endif // __TBB_CPP17_DEDUCTION_GUIDES_PRESENT

template <typename CHMapType>
void test_comparisons_basic() {
    using comparisons_testing::testEqualityComparisons;
    CHMapType c1, c2;
    testEqualityComparisons</*ExpectEqual = */true>(c1, c2);

    c1.emplace(1, 1);
    testEqualityComparisons</*ExpectEqual = */false>(c1, c2);

    c2.emplace(1, 1);
    testEqualityComparisons</*ExpectEqual = */true>(c1, c2);
}

template <typename TwoWayComparableContainerType>
void test_two_way_comparable_chmap() {
    TwoWayComparableContainerType c1, c2;
    c1.emplace(1, 1);
    c2.emplace(1, 1);
    comparisons_testing::TwoWayComparable::reset();
    REQUIRE_MESSAGE(c1 == c2, "Incorrect operator == result");
    comparisons_testing::check_equality_comparison();
    REQUIRE_MESSAGE(!(c1 != c2), "Incorrect operator != result");
    comparisons_testing::check_equality_comparison();
}

void TestCHMapComparisons() {
    using integral_container = oneapi::tbb::concurrent_hash_map<int, int>;
    using two_way_comparable_container = oneapi::tbb::concurrent_hash_map<comparisons_testing::TwoWayComparable,
                                                                          comparisons_testing::TwoWayComparable>;

    test_comparisons_basic<integral_container>();
    test_comparisons_basic<two_way_comparable_container>();
    test_two_way_comparable_chmap<two_way_comparable_container>();
}

template <typename Iterator, typename CHMapType>
void TestCHMapIteratorComparisonsBasic( CHMapType& chmap ) {
    REQUIRE_MESSAGE(!chmap.empty(), "Incorrect test setup");
    using namespace comparisons_testing;
    Iterator it1, it2;
    testEqualityComparisons</*ExpectEqual = */true>(it1, it2);
    it1 = chmap.begin();
    testEqualityComparisons</*ExpectEqual = */false>(it1, it2);
    it2 = chmap.begin();
    testEqualityComparisons</*ExpectEqual = */true>(it1, it2);
    it2 = chmap.end();
    testEqualityComparisons</*ExpectEqual = */false>(it1, it2);
}

void TestCHMapIteratorComparisons() {
    using chmap_type = oneapi::tbb::concurrent_hash_map<int, int>;
    using value_type = typename chmap_type::value_type;
    chmap_type chmap = {value_type{1, 1}, value_type{2, 2}, value_type{3, 3}};
    TestCHMapIteratorComparisonsBasic<typename chmap_type::iterator>(chmap);
    const chmap_type& cchmap = chmap;
    TestCHMapIteratorComparisonsBasic<typename chmap_type::const_iterator>(cchmap);
}

template <bool IsConstructible>
class HeterogeneousKey {
public:
    static std::size_t heterogeneous_keys_count;

    int integer_key() const { return my_key; }

    template <bool I = IsConstructible, typename = typename std::enable_if<I>::type>
    HeterogeneousKey(int key) : my_key(key) { ++heterogeneous_keys_count; }

    HeterogeneousKey(const HeterogeneousKey&) = delete;
    HeterogeneousKey& operator=(const HeterogeneousKey&) = delete;

    static void reset() { heterogeneous_keys_count = 0; }

    struct construct_flag {};

    HeterogeneousKey( construct_flag, int key ) : my_key(key) {}

private:
    int my_key;
}; // class HeterogeneousKey

template <bool IsConstructible>
std::size_t HeterogeneousKey<IsConstructible>::heterogeneous_keys_count = 0;

struct HeterogeneousHashCompare {
    using is_transparent = void;

    template <bool IsConstructible>
    std::size_t hash( const HeterogeneousKey<IsConstructible>& key ) const {
        return my_hash_object(key.integer_key());
    }

    std::size_t hash( const int& key ) const {
        return my_hash_object(key);
    }

    bool equal( const int& key1, const int& key2 ) const {
        return key1 == key2;
    }

    template <bool IsConstructible>
    bool equal( const int& key1, const HeterogeneousKey<IsConstructible>& key2 ) const {
        return key1 == key2.integer_key();
    }

    template <bool IsConstructible>
    bool equal( const HeterogeneousKey<IsConstructible>& key1, const int& key2 ) const {
        return key1.integer_key() == key2;
    }

    template <bool IsConstructible>
    bool equal( const HeterogeneousKey<IsConstructible>& key1, const HeterogeneousKey<IsConstructible>& key2 ) const {
        return key1.integer_key() == key2.integer_key();
    }

    std::hash<int> my_hash_object;
}; // struct HeterogeneousHashCompare

class DefaultConstructibleValue {
public:
    DefaultConstructibleValue() : my_i(default_value) {};

    int value() const { return my_i; }
    static constexpr int default_value = -4242;
private:
    int my_i;
}; // class DefaultConstructibleValue

constexpr int DefaultConstructibleValue::default_value;

void test_heterogeneous_find() {
    using key_type = HeterogeneousKey</*IsConstructible = */false>;
    using chmap_type = oneapi::tbb::concurrent_hash_map<key_type, int, HeterogeneousHashCompare>;

    chmap_type chmap;
    using const_accessor = typename chmap_type::const_accessor;
    using accessor = typename chmap_type::accessor;
    const_accessor cacc;
    accessor acc;

    REQUIRE_MESSAGE(key_type::heterogeneous_keys_count == 0, "Incorrect test setup");

    key_type key(key_type::construct_flag{}, 1);
    bool regular_result = chmap.find(cacc, key);
    bool heterogeneous_result = chmap.find(cacc, int(1));

    REQUIRE(!regular_result);
    REQUIRE_MESSAGE(regular_result == heterogeneous_result,
                    "Incorrect heterogeneous find result with const_accessor (no element)");
    REQUIRE_MESSAGE(key_type::heterogeneous_keys_count == 0, "Temporary key object was created during find call with const_accessor (no element)");

    regular_result = chmap.find(acc, key);
    heterogeneous_result = chmap.find(acc, int(1));

    REQUIRE(!regular_result);
    REQUIRE_MESSAGE(regular_result == heterogeneous_result,
                    "Incorrect heterogeneous find result with accessor (no element)");
    REQUIRE_MESSAGE(key_type::heterogeneous_keys_count == 0, "Temporary key object was created during find call with accessor (no element)");

    bool tmp_result = chmap.emplace(cacc, std::piecewise_construct,
                                    std::forward_as_tuple(key_type::construct_flag{}, 1), std::forward_as_tuple(100));
    REQUIRE(tmp_result);

    regular_result = chmap.find(cacc, key);
    heterogeneous_result = chmap.find(cacc, int(1));

    REQUIRE(regular_result);
    REQUIRE_MESSAGE(regular_result == heterogeneous_result, "Incorrect heterogeneous find result with const_accessor (element exists)");
    REQUIRE_MESSAGE(cacc->first.integer_key() == 1, "Incorrect accessor returned");
    REQUIRE_MESSAGE(key_type::heterogeneous_keys_count == 0, "Temporary key object was created during find call with const_accessor (element exists)");
    cacc.release();

    regular_result = chmap.find(acc, key);
    heterogeneous_result = chmap.find(acc, int(1));

    REQUIRE(regular_result);
    REQUIRE_MESSAGE(regular_result == heterogeneous_result, "Incorrect heterogeneous find result with accessor (element exists)");
    REQUIRE_MESSAGE(acc->first.integer_key() == 1, "Incorrect accessor returned");
    REQUIRE_MESSAGE(key_type::heterogeneous_keys_count == 0, "Temporary key object was created during find call with accessor (element exists)");
    key_type::reset();
}

void test_heterogeneous_count() {
    using key_type = HeterogeneousKey</*IsConstructible = */false>;
    using chmap_type = oneapi::tbb::concurrent_hash_map<key_type, int, HeterogeneousHashCompare>;

    chmap_type chmap;

    REQUIRE_MESSAGE(key_type::heterogeneous_keys_count == 0, "Incorrect test setup");
    key_type key(key_type::construct_flag{}, 1);

    typename chmap_type::size_type regular_count = chmap.count(key);
    typename chmap_type::size_type heterogeneous_count = chmap.count(int(1));

    REQUIRE(regular_count == 0);
    REQUIRE_MESSAGE(regular_count == heterogeneous_count, "Incorrect heterogeneous count result (no element)");
    REQUIRE_MESSAGE(key_type::heterogeneous_keys_count == 0, "Temporary key object was created during count call (no element)");

    chmap.emplace(std::piecewise_construct, std::forward_as_tuple(key_type::construct_flag{}, 1), std::forward_as_tuple(100));

    regular_count = chmap.count(key);
    heterogeneous_count = chmap.count(int(1));

    REQUIRE(regular_count == 1);
    REQUIRE_MESSAGE(regular_count == heterogeneous_count, "Incorrect heterogeneous count result (element exists)");
    REQUIRE_MESSAGE(key_type::heterogeneous_keys_count == 0, "Temporary key object was created during count call (element exists)");
    key_type::reset();
}

void test_heterogeneous_equal_range() {
    using key_type = HeterogeneousKey</*IsConstructible = */false>;
    using chmap_type = oneapi::tbb::concurrent_hash_map<key_type, int, HeterogeneousHashCompare>;

    chmap_type chmap;
    REQUIRE_MESSAGE(key_type::heterogeneous_keys_count == 0, "Incorrect test setup");

    using iterator = typename chmap_type::iterator;
    using const_iterator = typename chmap_type::const_iterator;
    using result = std::pair<iterator, iterator>;
    using const_result = std::pair<const_iterator, const_iterator>;
    key_type key(key_type::construct_flag{}, 1);

    result regular_result = chmap.equal_range(key);
    result heterogeneous_result = chmap.equal_range(int(1));

    REQUIRE(regular_result.first == chmap.end());
    REQUIRE(regular_result.second == chmap.end());
    REQUIRE_MESSAGE(regular_result == heterogeneous_result, "Incorrect heterogeneous equal_range result (non const, no element)");
    REQUIRE_MESSAGE(key_type::heterogeneous_keys_count == 0, "Temporary key object was created during equal_range call (non const, no element)");

    const chmap_type& cchmap = chmap;

    const_result regular_const_result = cchmap.equal_range(key);
    const_result heterogeneous_const_result = cchmap.equal_range(int(1));

    REQUIRE(regular_const_result.first == cchmap.end());
    REQUIRE(regular_const_result.second == cchmap.end());
    REQUIRE_MESSAGE(regular_const_result == heterogeneous_const_result,
                    "Incorrect heterogeneous equal_range result (const, no element)");
    REQUIRE_MESSAGE(key_type::heterogeneous_keys_count == 0, "Temporary key object was created during equal_range call (const, no element)");

    chmap.emplace(std::piecewise_construct, std::forward_as_tuple(key_type::construct_flag{}, 1), std::forward_as_tuple(100));

    regular_result = chmap.equal_range(key);
    heterogeneous_result = chmap.equal_range(int(1));

    REQUIRE(regular_result.first != chmap.end());
    REQUIRE(regular_result.first->first.integer_key() == 1);
    REQUIRE(regular_result.second == chmap.end());
    REQUIRE_MESSAGE(regular_result == heterogeneous_result, "Incorrect heterogeneous equal_range result (non const, element exists)");
    REQUIRE_MESSAGE(key_type::heterogeneous_keys_count == 0, "Temporary key object was created during equal_range call (non const, element exists)");

    regular_const_result = cchmap.equal_range(key);
    heterogeneous_const_result = cchmap.equal_range(int(1));
    REQUIRE_MESSAGE(regular_const_result == heterogeneous_const_result,
                    "Incorrect heterogeneous equal_range result (const, element exists)");
    REQUIRE_MESSAGE(key_type::heterogeneous_keys_count == 0, "Temporary key object was created during equal_range call (const, element exists)");
    key_type::reset();
}

void test_heterogeneous_insert() {
    using key_type = HeterogeneousKey</*IsConstructible = */true>;
    using chmap_type = oneapi::tbb::concurrent_hash_map<key_type, DefaultConstructibleValue, HeterogeneousHashCompare>;

    chmap_type chmap;
    using const_accessor = typename chmap_type::const_accessor;
    using accessor = typename chmap_type::accessor;
    const_accessor cacc;
    accessor acc;

    REQUIRE_MESSAGE(key_type::heterogeneous_keys_count == 0, "Incorrect test setup");

    bool result = chmap.insert(cacc, int(1));

    REQUIRE_MESSAGE(key_type::heterogeneous_keys_count == 1, "Only one heterogeneous key should be created");
    REQUIRE_MESSAGE(result, "Incorrect heterogeneous insert result (const_accessor)");
    REQUIRE_MESSAGE(cacc->first.integer_key() == 1, "Incorrect accessor");
    REQUIRE_MESSAGE(cacc->second.value() == DefaultConstructibleValue::default_value, "Value should be default constructed");

    result = chmap.insert(cacc, int(1));

    REQUIRE_MESSAGE(key_type::heterogeneous_keys_count == 1, "No extra keys should be created");
    REQUIRE_MESSAGE(!result, "Incorrect heterogeneous insert result (const_accessor)");
    REQUIRE_MESSAGE(cacc->first.integer_key() == 1, "Incorrect accessor");
    REQUIRE_MESSAGE(cacc->second.value() == DefaultConstructibleValue::default_value, "Value should be default constructed");

    result = chmap.insert(acc, int(2));

    REQUIRE_MESSAGE(key_type::heterogeneous_keys_count == 2, "Only one extra heterogeneous key should be created");
    REQUIRE_MESSAGE(result, "Incorrect heterogeneous insert result (accessor)");
    REQUIRE_MESSAGE(acc->first.integer_key() == 2, "Incorrect accessor");
    REQUIRE_MESSAGE(acc->second.value() == DefaultConstructibleValue::default_value, "Value should be default constructed");

    result = chmap.insert(acc, int(2));

    REQUIRE_MESSAGE(key_type::heterogeneous_keys_count == 2, "No extra keys should be created");
    REQUIRE_MESSAGE(!result, "Incorrect heterogeneous insert result (accessor)");
    REQUIRE_MESSAGE(acc->first.integer_key() == 2, "Incorrect accessor");
    REQUIRE_MESSAGE(acc->second.value() == DefaultConstructibleValue::default_value, "Value should be default constructed");

    key_type::reset();
}

void test_heterogeneous_erase() {
    using key_type = HeterogeneousKey</*IsConstructible = */false>;
    using chmap_type = oneapi::tbb::concurrent_hash_map<key_type, int, HeterogeneousHashCompare>;

    chmap_type chmap;

    REQUIRE_MESSAGE(key_type::heterogeneous_keys_count == 0, "Incorrect test setup");

    chmap.emplace(std::piecewise_construct, std::forward_as_tuple(key_type::construct_flag{}, 1), std::forward_as_tuple(100));
    chmap.emplace(std::piecewise_construct, std::forward_as_tuple(key_type::construct_flag{}, 2), std::forward_as_tuple(200));

    typename chmap_type::const_accessor cacc;

    REQUIRE(chmap.find(cacc, int(1)));
    REQUIRE(chmap.find(cacc, int(2)));

    cacc.release();

    bool result = chmap.erase(int(1));
    REQUIRE_MESSAGE(result, "Erasure should be successful");
    REQUIRE_MESSAGE(key_type::heterogeneous_keys_count == 0, "No extra keys should be created");
    REQUIRE_MESSAGE(!chmap.find(cacc, int(1)), "Element was not erased");


    result = chmap.erase(int(1));
    REQUIRE_MESSAGE(!result, "Erasure should fail");
    REQUIRE_MESSAGE(key_type::heterogeneous_keys_count == 0, "No extra keys should be created");
    key_type::reset();
}

void test_heterogeneous_lookup() {
    test_heterogeneous_find();
    test_heterogeneous_count();
    test_heterogeneous_equal_range();
}

//! Test consruction with hash_compare
//! \brief \ref interface \ref requirement
TEST_CASE("testing consruction with hash_compare") {
    TestHashCompareConstructors();
}

//! Test concurrent_hash_map member types
//! \brief \ref interface \ref requirement
TEST_CASE("test types"){
    test_member_types<oneapi::tbb::concurrent_hash_map>();
}

//! Test swap and clear operations
//! \brief \ref interface \ref requirement
TEST_CASE("test copy operations") {
    TestCopy();
}

//! Test rehash operation
//! \brief \ref interface \ref requirement
TEST_CASE("test rehash operation") {
    TestRehash();
}

//! Test assignment operation
//! \brief \ref interface \ref requirement
TEST_CASE("test assignment operation") {
    TestAssignment();
}

//! Test iterators and ranges
//! \brief \ref interface \ref requirement
TEST_CASE("test iterators and ranges") {
    TestIteratorsAndRanges();
}

//! Test work with initializer_list
//! \brief \ref interface \ref requirement
TEST_CASE("test work with initializer_list") {
    TestInitList();
}

#if TBB_USE_EXCEPTIONS
//! Test exception safety
//! \brief \ref requirement
TEST_CASE("test exception safety") {
    TestExceptions();
}

//! Test exceptions safety guarantees for move constructor
//! \brief \ref requirement
TEST_CASE("test move support with exceptions") {
    move_support_tests::test_ex_move_ctor_unequal_allocator_memory_failure<hash_map_traits>();
    move_support_tests::test_ex_move_ctor_unequal_allocator_element_ctor_failure<hash_map_traits>();
}
#endif

//! Test move constructor
//! \brief \ref interface \ref requirement
TEST_CASE("testing move constructor"){
    move_support_tests::test_move_constructor<hash_map_traits>();
}

//! Test move assign operator
//! \brief \ref interface \ref requirement
TEST_CASE("testing move assign operator"){
    move_support_tests::test_move_assignment<hash_map_traits>();
}

//! Test insert and empace
//! \brief \ref interface \ref requirement
TEST_CASE("testing concurrent insert and emplace"){
    int n=250000;
    {
        DataStateTrackedTable table;
        DoConcurrentOperations<RvalueInsert, DataStateTrackedTable>( table, n, "rvalue ref insert", 1 );
    }
    {
        DataStateTrackedTable table;
        DoConcurrentOperations<Emplace, DataStateTrackedTable>( table, n, "emplace", 1 );
    }
}

//! Test allocator traits
//! \brief \ref requirement
TEST_CASE("testing allocator traits") {
    test_allocator_traits_support<hash_map_traits>();
}

//! Test concurrent operations
//! \brief \ref requirement
TEST_CASE("testing concurrency"){
    for (std::size_t p = 1; p <= 4; ++p) {
        oneapi::tbb::global_control limit(oneapi::tbb::global_control::max_allowed_parallelism, p);
        TestInsertFindErase(p);
        TestConcurrency(p);
    }
}

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
//! Test deduction guides
//! \brief \ref interface
TEST_CASE("testing deduction guides") {
    TestDeductionGuides<oneapi::tbb::concurrent_hash_map>();
}
#endif // __TBB_CPP17_DEDUCTION_GUIDES_PRESENT

//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_hash_map comparisons") {
    TestCHMapComparisons();
}

//! \brief \ref interface \ref requirement
TEST_CASE("concurrent_hash_map iterator comparisons") {
    TestCHMapIteratorComparisons();
}

//! \brief \ref interface \ref requirement
TEST_CASE("test concurrent_hash_map heterogeneous lookup") {
    test_heterogeneous_lookup();
}

//! \brief \ref interface \ref requirement
TEST_CASE("test concurrent_hash_map heterogeneous insert") {
    test_heterogeneous_insert();
}

//! \brief \ref interface \ref requirement
TEST_CASE("test concurrent_hash_map heterogeneous erase") {
    test_heterogeneous_erase();
}
