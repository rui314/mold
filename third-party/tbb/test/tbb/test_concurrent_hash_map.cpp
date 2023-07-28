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

#if _MSC_VER
#if __INTEL_COMPILER
    #pragma warning(disable : 2586) // decorated name length exceeded, name was truncated
#else
    // Workaround for vs2015 and warning name was longer than the compiler limit (4096).
    #pragma warning (disable: 4503)
#endif
#endif

#define TBB_DEFINE_STD_HASH_SPECIALIZATIONS 1
#define TBB_PREVIEW_CONCURRENT_HASH_MAP_EXTENSIONS 1
#include <common/test.h>
#include <common/utils.h>
#include <common/range_based_for_support.h>
#include <common/custom_allocators.h>
#include <common/containers_common.h>
#include <common/concepts_common.h>
#include <tbb/concurrent_hash_map.h>
#include <tbb/parallel_for.h>
#include <common/concurrent_associative_common.h>
#include <vector>
#include <list>
#include <algorithm>
#include <functional>
#include <scoped_allocator>
#include <mutex>

//! \file test_concurrent_hash_map.cpp
//! \brief Test for [containers.concurrent_hash_map containers.tbb_hash_compare] specification

void TestRangeBasedFor(){
    using namespace range_based_for_support_tests;

    INFO("testing range based for loop compatibility \n");
    using ch_map = tbb::concurrent_hash_map<int,int>;
    ch_map a_ch_map;

    const int sequence_length = 100;
    for (int i = 1; i <= sequence_length; ++i){
        a_ch_map.insert(ch_map::value_type(i,i));
    }

    REQUIRE_MESSAGE((range_based_for_accumulate(a_ch_map, pair_second_summer(), 0) == gauss_summ_of_int_sequence(sequence_length)),
        "incorrect accumulated value generated via range based for ?");
}

// The helper to run a test only when a default construction is present.
template <bool default_construction_present> struct do_default_construction_test {
    template<typename FuncType> void operator() ( FuncType func ) const { func(); }
};

template <> struct do_default_construction_test<false> {
    template<typename FuncType> void operator()( FuncType ) const {}
};

template <typename Table>
class test_insert_by_key {
    using value_type = typename Table::value_type;
    Table &my_c;
    const value_type &my_value;
public:
    test_insert_by_key( Table &c, const value_type &value ) : my_c(c), my_value(value) {}
    void operator()() const {
        {
            typename Table::accessor a;
            CHECK(my_c.insert( a, my_value.first ));
            CHECK(utils::IsEqual()(a->first, my_value.first));
            a->second = my_value.second;
        }
        {
            typename Table::const_accessor ca;
            CHECK(!my_c.insert( ca, my_value.first ));
            CHECK(utils::IsEqual()(ca->first, my_value.first));
            CHECK(utils::IsEqual()(ca->second, my_value.second));
        }
    }
};

template <typename Table, typename Iterator, typename Range = typename Table::range_type>
class test_range {
    using value_type = typename Table::value_type;
    Table &my_c;
    const std::list<value_type> &my_lst;
    std::vector<detail::atomic_type<bool>>& my_marks;
public:
    test_range( Table &c, const std::list<value_type> &lst, std::vector<detail::atomic_type<bool>> &marks ) : my_c(c), my_lst(lst), my_marks(marks) {
        for (std::size_t i = 0; i < my_marks.size(); ++i) {
            my_marks[i].store(false, std::memory_order_relaxed);
        }
    }

    void operator()( const Range &r ) const { do_test_range( r.begin(), r.end() ); }
    void do_test_range( Iterator i, Iterator j ) const {
        for ( Iterator it = i; it != j; ) {
            Iterator it_prev = it++;
            typename std::list<value_type>::const_iterator it2 = std::search( my_lst.begin(), my_lst.end(), it_prev, it, utils::IsEqual() );
            CHECK(it2 != my_lst.end());
            typename std::list<value_type>::difference_type dist = std::distance( my_lst.begin(), it2 );
            CHECK(!my_marks[dist]);
            my_marks[dist].store(true);
        }
    }
};

template <bool default_construction_present, typename Table>
class check_value {
    using const_iterator = typename Table::const_iterator;
    using iterator = typename Table::iterator;
    using size_type = typename Table::size_type;
    Table &my_c;
public:
    check_value( Table &c ) : my_c(c) {}
    void operator()(const typename Table::value_type &value ) {
        const Table &const_c = my_c;
        CHECK(my_c.count( value.first ) == 1);
        { // tests with a const accessor.
            typename Table::const_accessor ca;
            // find
            CHECK(my_c.find( ca, value.first ));
            CHECK(!ca.empty() );
            CHECK(utils::IsEqual()(ca->first, value.first));
            CHECK(utils::IsEqual()(ca->second, value.second));
            // erase
            CHECK(my_c.erase( ca ));
            CHECK(my_c.count( value.first ) == 0);
            // insert (pair)
            CHECK(my_c.insert( ca, value ));
            CHECK(utils::IsEqual()(ca->first, value.first));
            CHECK(utils::IsEqual()(ca->second, value.second));
        } { // tests with a non-const accessor.
            typename Table::accessor a;
            // find
            CHECK(my_c.find( a, value.first ));
            CHECK(!a.empty() );
            CHECK(utils::IsEqual()(a->first, value.first));
            CHECK(utils::IsEqual()(a->second, value.second));
            // erase
            CHECK(my_c.erase( a ));
            CHECK(my_c.count( value.first ) == 0);
            // insert
            CHECK(my_c.insert( a, value ));
            CHECK(utils::IsEqual()(a->first, value.first));
            CHECK(utils::IsEqual()(a->second, value.second));
        }
        // erase by key
        CHECK(my_c.erase( value.first ));
        CHECK(my_c.count( value.first ) == 0);
        do_default_construction_test<default_construction_present>()(test_insert_by_key<Table>( my_c, value ));
        // insert by value
        CHECK(my_c.insert( value ) != default_construction_present);
        // equal_range
        std::pair<iterator,iterator> r1 = my_c.equal_range( value.first );
        iterator r1_first_prev = r1.first++;
        CHECK((utils::IsEqual()( *r1_first_prev, value ) && utils::IsEqual()( r1.first, r1.second )));
        std::pair<const_iterator,const_iterator> r2 = const_c.equal_range( value.first );
        const_iterator r2_first_prev = r2.first++;
        CHECK((utils::IsEqual()( *r2_first_prev, value ) && utils::IsEqual()( r2.first, r2.second )));
    }
};

template <typename Value, typename U = Value>
struct CompareTables {
    template <typename T>
    static bool IsEqual( const T& t1, const T& t2 ) {
        return (t1 == t2) && !(t1 != t2);
    }
};

template <typename U>
struct CompareTables< std::pair<const std::weak_ptr<U>, std::weak_ptr<U> > > {
    template <typename T>
    static bool IsEqual( const T&, const T& ) {
        /* do nothing for std::weak_ptr */
        return true;
    }
};

template <bool default_construction_present, typename Table>
void Examine( Table c, const std::list<typename Table::value_type> &lst) {
    using const_table = const Table;
    using const_iterator = typename Table::const_iterator;
    using iterator = typename Table::iterator;
    using value_type = typename Table::value_type;
    using size_type = typename Table::size_type;

    CHECK(!c.empty());
    CHECK(c.size() == lst.size());
    CHECK(c.max_size() >= c.size());

    const check_value<default_construction_present, Table> cv(c);
    std::for_each( lst.begin(), lst.end(), cv );

    std::vector<detail::atomic_type<bool>> marks( lst.size() );

    test_range<Table,iterator>( c, lst, marks ).do_test_range( c.begin(), c.end() );
    CHECK(std::find( marks.begin(), marks.end(), false ) == marks.end());

    test_range<const_table,const_iterator>( c, lst, marks ).do_test_range( c.begin(), c.end() );
    CHECK(std::find( marks.begin(), marks.end(), false ) == marks.end());

    using range_type = typename Table::range_type;
    tbb::parallel_for( c.range(), test_range<Table,typename range_type::iterator,range_type>( c, lst, marks ) );
    CHECK(std::find( marks.begin(), marks.end(), false ) == marks.end());

    const_table const_c = c;
    CHECK(CompareTables<value_type>::IsEqual( c, const_c ));

    const size_type new_bucket_count = 2*c.bucket_count();
    c.rehash( new_bucket_count );
    CHECK(c.bucket_count() >= new_bucket_count);

    Table c2;
    typename std::list<value_type>::const_iterator begin5 = lst.begin();
    std::advance( begin5, 5 );
    c2.insert( lst.begin(), begin5 );
    std::for_each( lst.begin(), begin5, check_value<default_construction_present, Table>( c2 ) );

    c2.swap( c );
    CHECK(CompareTables<value_type>::IsEqual( c2, const_c ));
    CHECK(c.size() == 5);
    std::for_each( lst.begin(), lst.end(), check_value<default_construction_present,Table>(c2) );

    swap( c, c2 );
    CHECK(CompareTables<value_type>::IsEqual( c, const_c ));
    CHECK(c2.size() == 5);

    c2.clear();
    CHECK(CompareTables<value_type>::IsEqual( c2, Table() ));

    typename Table::allocator_type a = c.get_allocator();
    value_type *ptr = a.allocate(1);
    CHECK(ptr);
    a.deallocate( ptr, 1 );
}

template <typename T>
struct debug_hash_compare : public tbb::detail::d1::tbb_hash_compare<T> {};

template <bool default_construction_present, typename Value>
void TypeTester( const std::list<Value> &lst ) {
    using first_type = typename Value::first_type;
    using key_type = typename std::remove_const<first_type>::type;
    using second_type = typename Value::second_type;
    using ch_map = tbb::concurrent_hash_map<key_type, second_type>;
    debug_hash_compare<key_type> compare{};
    // Construct an empty hash map.
    ch_map c1;
    c1.insert( lst.begin(), lst.end() );
    Examine<default_construction_present>( c1, lst );

    // Constructor from initializer_list.
    typename std::list<Value>::const_iterator it = lst.begin();
    std::initializer_list<Value> il = { *it++, *it++, *it++ };
    ch_map c2( il );
    c2.insert( it, lst.end() );
    Examine<default_construction_present>( c2, lst );

    // Constructor from initializer_list and compare object
    ch_map c3( il, compare);
    c3.insert( it, lst.end() );
    Examine<default_construction_present>( c3, lst );

    // Constructor from initializer_list, compare object and allocator
    ch_map c4( il, compare, typename ch_map::allocator_type());
    c4.insert( it, lst.end());
    Examine<default_construction_present>( c4, lst );

    // Copying constructor.
    ch_map c5(c1);
    Examine<default_construction_present>( c5, lst );
    // Construct with non-default allocator
    using ch_map_debug_alloc = tbb::concurrent_hash_map<key_type, second_type,
                                                        tbb::detail::d1::tbb_hash_compare<key_type>,
                                                        LocalCountingAllocator<std::allocator<Value>>>;
    ch_map_debug_alloc c6;
    c6.insert( lst.begin(), lst.end() );
    Examine<default_construction_present>( c6, lst );
    // Copying constructor
    ch_map_debug_alloc c7(c6);
    Examine<default_construction_present>( c7, lst );
    // Construction empty table with n preallocated buckets.
    ch_map c8( lst.size() );
    c8.insert( lst.begin(), lst.end() );
    Examine<default_construction_present>( c8, lst );
    ch_map_debug_alloc c9( lst.size() );
    c9.insert( lst.begin(), lst.end() );
    Examine<default_construction_present>( c9, lst );
    // Construction with copying iteration range.
    ch_map c10_1( c1.begin(), c1.end() ), c10_2(c1.cbegin(), c1.cend());
    Examine<default_construction_present>( c10_1, lst );
    Examine<default_construction_present>( c10_2, lst );
    // Construction with copying iteration range and given allocator instance.
    LocalCountingAllocator<std::allocator<Value>> allocator;
    ch_map_debug_alloc c11( lst.begin(), lst.end(), allocator );
    Examine<default_construction_present>( c11, lst );

    using ch_map_debug_hash = tbb::concurrent_hash_map<key_type, second_type,
                                                       debug_hash_compare<key_type>,
                                                       typename ch_map::allocator_type>;

    // Constructor with two iterators and hash_compare
    ch_map_debug_hash c12(c1.begin(), c1.end(), compare);
    Examine<default_construction_present>( c12, lst );

    ch_map_debug_hash c13(c1.begin(), c1.end(), compare, typename ch_map::allocator_type());
    Examine<default_construction_present>( c13, lst );
}

void TestSpecificTypes() {
    const int NUMBER = 10;

    using int_int_t = std::pair<const int, int>;
    std::list<int_int_t> arrIntInt;
    for ( int i=0; i<NUMBER; ++i ) arrIntInt.push_back( int_int_t(i, NUMBER-i) );
    TypeTester</*default_construction_present = */true>( arrIntInt );

    using ref_int_t = std::pair<const std::reference_wrapper<const int>, int>;
    std::list<ref_int_t> arrRefInt;
    for ( std::list<int_int_t>::iterator it = arrIntInt.begin(); it != arrIntInt.end(); ++it )
        arrRefInt.push_back( ref_int_t( it->first, it->second ) );
    TypeTester</*default_construction_present = */true>( arrRefInt );

    using int_ref_t = std::pair< const int, std::reference_wrapper<int> >;
    std::list<int_ref_t> arrIntRef;
    for ( std::list<int_int_t>::iterator it = arrIntInt.begin(); it != arrIntInt.end(); ++it )
        arrIntRef.push_back( int_ref_t( it->first, it->second ) );
    TypeTester</*default_construction_present = */false>( arrIntRef );

    using shr_shr_t = std::pair< const std::shared_ptr<int>, std::shared_ptr<int> >;
    std::list<shr_shr_t> arrShrShr;
    for ( int i=0; i<NUMBER; ++i ) {
        const int NUMBER_minus_i = NUMBER - i;
        arrShrShr.push_back( shr_shr_t( std::make_shared<int>(i), std::make_shared<int>(NUMBER_minus_i) ) );
    }
    TypeTester< /*default_construction_present = */true>( arrShrShr );

    using wk_wk_t = std::pair< const std::weak_ptr<int>, std::weak_ptr<int> >;
    std::list< wk_wk_t > arrWkWk;
    std::copy( arrShrShr.begin(), arrShrShr.end(), std::back_inserter(arrWkWk) );
    TypeTester< /*default_construction_present = */true>( arrWkWk );

    // Check working with deprecated hashers
    using pair_key_type = std::pair<int, int>;
    using pair_int_t = std::pair<const pair_key_type, int>;
    std::list<pair_int_t> arr_pair_int;
    for (int i = 0; i < NUMBER; ++i) {
        arr_pair_int.push_back(pair_int_t(pair_key_type{i, i}, i));
    }
    TypeTester</*default_construction_present = */true>(arr_pair_int);

    using tbb_string_key_type = std::basic_string<char, std::char_traits<char>, tbb::tbb_allocator<char>>;
    using pair_tbb_string_int_t = std::pair<const tbb_string_key_type, int>;
    std::list<pair_tbb_string_int_t> arr_pair_string_int;
    for (int i = 0; i < NUMBER; ++i) {
        tbb_string_key_type key(i, char(i));
        arr_pair_string_int.push_back(pair_tbb_string_int_t(key, i));
    }
    TypeTester</*default_construction_present = */true>(arr_pair_string_int);
}

struct custom_hash_compare {
    template<typename Allocator>
    size_t hash(const AllocatorAwareData<Allocator>& key) const {
        return my_hash_compare.hash(key.value());
    }

    template<typename Allocator>
    bool equal(const AllocatorAwareData<Allocator>& key1, const AllocatorAwareData<Allocator>& key2) const {
        return my_hash_compare.equal(key1.value(), key2.value());
    }

private:
    tbb::tbb_hash_compare<int> my_hash_compare;
};

void TestScopedAllocator() {
    using allocator_data_type = AllocatorAwareData<std::scoped_allocator_adaptor<tbb::tbb_allocator<int>>>;
    using allocator_type = std::scoped_allocator_adaptor<tbb::tbb_allocator<std::pair<const allocator_data_type, allocator_data_type>>>;
    using hash_map_type = tbb::concurrent_hash_map<allocator_data_type, allocator_data_type,
                                                   custom_hash_compare, allocator_type>;

    allocator_type allocator;
    allocator_data_type key1(1, allocator), key2(2, allocator);
    allocator_data_type data1(1, allocator), data2(data1, allocator);
    hash_map_type map1(allocator), map2(allocator);

    hash_map_type::value_type v1(key1, data1), v2(key2, data2);

    auto init_list = { v1, v2 };

    allocator_data_type::assert_on_constructions = true;
    map1.emplace(key1, data1);
    map2.emplace(key2, std::move(data2));

    map1.clear();
    map2.clear();

    map1.insert(v1);
    map2.insert(std::move(v2));

    map1.clear();
    map2.clear();

    map1.insert(init_list);

    map1.clear();
    map2.clear();

    hash_map_type::accessor a;
    map2.insert(a, allocator_data_type(3));
    a.release();

    map1 = map2;
    map2 = std::move(map1);

    hash_map_type map3(allocator);
    map3.rehash(1000);
    map3 = map2;
}

// A test for undocumented member function internal_fast_find
// which is declared protected in concurrent_hash_map for internal TBB use
void TestInternalFastFind() {
    typedef tbb::concurrent_hash_map<int, int> basic_chmap_type;
    typedef basic_chmap_type::const_pointer const_pointer;

    class chmap : public basic_chmap_type {
    public:
        chmap() : basic_chmap_type() {}

        using basic_chmap_type::internal_fast_find;
    };

    chmap m;
    int sz = 100;

    for (int i = 0; i != sz; ++i) {
        m.insert(std::make_pair(i, i * i));
    }
    REQUIRE_MESSAGE(m.size() == 100, "Incorrect concurrent_hash_map size");

    for (int i = 0; i != sz; ++i) {
        const_pointer res = m.internal_fast_find(i);
        REQUIRE_MESSAGE(res != nullptr, "Incorrect internal_fast_find return value for existing key");
        basic_chmap_type::value_type val = *res;
        REQUIRE_MESSAGE(val.first == i, "Incorrect key in internal_fast_find return value");
        REQUIRE_MESSAGE(val.second == i * i, "Incorrect mapped in internal_fast_find return value");
    }

    for (int i = sz; i != 2 * sz; ++i) {
        const_pointer res = m.internal_fast_find(i);
        REQUIRE_MESSAGE(res == nullptr, "Incorrect internal_fast_find return value for not existing key");
    }
}

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
    using container_type = tbb::concurrent_hash_map<T, T, hash_compare<T>, Allocator>;

    template <typename T>
    using container_value_type = std::pair<const T, T>;

    template<typename element_type, typename allocator_type>
    struct apply {
        using type = tbb::concurrent_hash_map<element_type, element_type, hash_compare<element_type>, allocator_type>;
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

template <bool SimulateReacquiring>
class MinimalisticMutex {
public:
    static constexpr bool is_rw_mutex = true;
    static constexpr bool is_recursive_mutex = false;
    static constexpr bool is_fair_mutex = false;

    class scoped_lock {
    public:
        constexpr scoped_lock() noexcept : my_mutex_ptr(nullptr) {}

        scoped_lock( MinimalisticMutex& m, bool = true ) : my_mutex_ptr(&m) {
            my_mutex_ptr->my_mutex.lock();
        }

        scoped_lock( const scoped_lock& ) = delete;
        scoped_lock& operator=( const scoped_lock& ) = delete;

        ~scoped_lock() {
            if (my_mutex_ptr) release();
        }

        void acquire( MinimalisticMutex& m, bool = true ) {
            CHECK(my_mutex_ptr == nullptr);
            my_mutex_ptr = &m;
            my_mutex_ptr->my_mutex.lock();
        }

        bool try_acquire( MinimalisticMutex& m, bool = true ) {
            if (m.my_mutex.try_lock()) {
                my_mutex_ptr = &m;
                return true;
            }
            return false;
        }

        void release() {
            CHECK(my_mutex_ptr != nullptr);
            my_mutex_ptr->my_mutex.unlock();
            my_mutex_ptr = nullptr;
        }

        bool upgrade_to_writer() const {
            // upgrade_to_writer should return false if the mutex simulates
            // reaquiring the lock on upgrade operation
            return !SimulateReacquiring;
        }

        bool downgrade_to_reader() const {
            // downgrade_to_reader should return false if the mutex simulates
            // reaquiring the lock on upgrade operation
            return !SimulateReacquiring;
        }

        bool is_writer() const {
            CHECK(my_mutex_ptr != nullptr);
            return true; // Always a writer
        }

    private:
        MinimalisticMutex* my_mutex_ptr;
    }; // class scoped_lock
private:
    std::mutex my_mutex;
}; // class MinimalisticMutex

template <bool SimulateReacquiring>
void test_with_minimalistic_mutex() {
    using mutex_type = MinimalisticMutex<SimulateReacquiring>;
    using chmap_type = tbb::concurrent_hash_map<int, int, tbb::tbb_hash_compare<int>,
                                                tbb::tbb_allocator<std::pair<const int, int>>,
                                                mutex_type>;

    chmap_type chmap;

    // Insert pre-existing elements
    for (int i = 0; i < 100; ++i) {
        bool result = chmap.emplace(i, i);
        CHECK(result);
    }

    // Insert elements to erase
    for (int i = 10000; i < 10005; ++i) {
        bool result = chmap.emplace(i, i);
        CHECK(result);
    }

    auto thread_body = [&]( const tbb::blocked_range<std::size_t>& range ) {
        for (std::size_t item = range.begin(); item != range.end(); ++item) {
            switch(item % 4) {
                case 0 :
                    // Insert new elements
                    for (int i = 100; i < 200; ++i) {
                        typename chmap_type::const_accessor acc;
                        chmap.emplace(acc, i, i);
                        CHECK(acc->first == i);
                        CHECK(acc->second == i);
                    }
                    break;
                case 1 :
                    // Insert pre-existing elements
                    for (int i = 0; i < 100; ++i) {
                        typename chmap_type::const_accessor acc;
                        bool result = chmap.emplace(acc, i, i * 10000);
                        CHECK(!result);
                        CHECK(acc->first == i);
                        CHECK(acc->second == i);
                    }
                    break;
                case 2 :
                    // Find pre-existing elements
                    for (int i = 0; i < 100; ++i) {
                        typename chmap_type::const_accessor acc;
                        bool result = chmap.find(acc, i);
                        CHECK(result);
                        CHECK(acc->first == i);
                        CHECK(acc->second == i);
                    }
                    break;
                case 3 :
                    // Erase pre-existing elements
                    for (int i = 10000; i < 10005; ++i) {
                        chmap.erase(i);
                    }
                break;
            }
        }
    }; // thread_body

    tbb::blocked_range<std::size_t> br(0, 1000, 8);

    tbb::parallel_for(br, thread_body);

    // Check pre-existing and new elements
    for (int i = 0; i < 200; ++i) {
        typename chmap_type::const_accessor acc;
        bool result = chmap.find(acc, i);
        REQUIRE_MESSAGE(result, "Some element was unexpectedly removed or not inserted");
        REQUIRE_MESSAGE(acc->first == i, "Incorrect key");
        REQUIRE_MESSAGE(acc->second == i, "Incorrect value");
    }

    // Check elements for erasure
    for (int i = 10000; i < 10005; ++i) {
        typename chmap_type::const_accessor acc;
        bool result = chmap.find(acc, i);
        REQUIRE_MESSAGE(!result, "Some element was not removed");
    }
}

void test_mutex_customization() {
    test_with_minimalistic_mutex</*SimulateReacquiring = */false>();
    test_with_minimalistic_mutex</*SimulateReacquiring = */true>();
}

struct SimpleTransparentHashCompare {
    using is_transparent = void;

    template <typename T>
    std::size_t hash(const T&) const { return 0; }

    template <typename T, typename U>
    bool equal(const T& key1, const U& key2) const { return key1 == key2; }
};

template <typename Accessor>
struct IsWriterAccessor : public Accessor {
    using Accessor::is_writer;
};

template <typename Map, typename Accessor>
void test_chmap_access_mode(bool expect_write) {
    static_assert(std::is_same<int, typename Map::key_type>::value, "Incorrect test setup");
    Map map;
    Accessor acc;

    // Test homogeneous insert
    bool result = map.insert(acc, 1);
    CHECK(result);
    CHECK_MESSAGE(acc.is_writer() == expect_write, "Incorrect access into the map from homogeneous insert");

    // Test heterogeneous insert
    result = map.insert(acc, 2L);
    CHECK(result);
    CHECK_MESSAGE(acc.is_writer() == expect_write, "Incorrect access into the map from heterogeneous insert");

    // Test lvalue insert
    typename Map::value_type value{3, 3};
    result = map.insert(acc, value);
    CHECK(result);
    CHECK_MESSAGE(acc.is_writer() == expect_write, "Incorrect access into the map from lvalue insert");

    // Test rvalue insert
    result = map.insert(acc, typename Map::value_type{4, 4});
    CHECK(result);
    CHECK_MESSAGE(acc.is_writer() == expect_write, "Incorrect access into the map from rvalue insert");

    // Test homogeneous find
    result = map.find(acc, 1);
    CHECK(result);
    CHECK_MESSAGE(acc.is_writer() == expect_write, "Incorrect access into the map from homogeneous find");

    // Test heterogeneous find
    result = map.find(acc, 2L);
    CHECK(result);
    CHECK_MESSAGE(acc.is_writer() == expect_write, "Incorrect access into the map from heterogeneous find");
}

//! Test of insert operation
//! \brief \ref error_guessing
TEST_CASE("testing range based for support"){
    TestRangeBasedFor();
}

//! Test concurrent_hash_map with specific key/mapped types
//! \brief \ref regression \ref error_guessing
TEST_CASE("testing concurrent_hash_map with specific key/mapped types") {
    TestSpecificTypes();
}

//! Test work with scoped allocator
//! \brief \ref regression
TEST_CASE("testing work with scoped allocator") {
    TestScopedAllocator();
}

//! Test internal fast find for concurrent_hash_map
//! \brief \ref regression
TEST_CASE("testing internal fast find for concurrent_hash_map") {
    TestInternalFastFind();
}

//! Test constructor with move iterators
//! \brief \ref error_guessing
TEST_CASE("testing constructor with move iterators"){
    move_support_tests::test_constructor_with_move_iterators<hash_map_traits>();
}

#if TBB_USE_EXCEPTIONS
//! Test exception in constructors
//! \brief \ref regression \ref error_guessing
TEST_CASE("Test exception in constructors") {
    using allocator_type = StaticSharedCountingAllocator<std::allocator<std::pair<const int, int>>>;
    using map_type = tbb::concurrent_hash_map<int, int, tbb::tbb_hash_compare<int>, allocator_type>;

    auto init_list = {std::pair<const int, int>(1, 42), std::pair<const int, int>(2, 42), std::pair<const int, int>(3, 42),
        std::pair<const int, int>(4, 42), std::pair<const int, int>(5, 42), std::pair<const int, int>(6, 42)};
    map_type map(init_list);

    allocator_type::set_limits(1);
    REQUIRE_THROWS_AS( [&] {
        map_type map1(map);
        utils::suppress_unused_warning(map1);
    }(), const std::bad_alloc);

    REQUIRE_THROWS_AS( [&] {
        map_type map2(init_list.begin(), init_list.end());
        utils::suppress_unused_warning(map2);
    }(), const std::bad_alloc);

    tbb::tbb_hash_compare<int> test_hash;

    REQUIRE_THROWS_AS( [&] {
        map_type map3(init_list.begin(), init_list.end(), test_hash);
        utils::suppress_unused_warning(map3);
    }(), const std::bad_alloc);

    REQUIRE_THROWS_AS( [&] {
        map_type map4(init_list, test_hash);
        utils::suppress_unused_warning(map4);
    }(), const std::bad_alloc);

    REQUIRE_THROWS_AS( [&] {
        map_type map5(init_list);
        utils::suppress_unused_warning(map5);
    }(), const std::bad_alloc);

    allocator_type::set_limits(0);
    map_type big_map{};
    for (int i = 0; i < 1000; ++i) {
        big_map.insert(std::pair<const int, int>(i, 42));
    }

    allocator_type::init_counters();
    allocator_type::set_limits(300);
    REQUIRE_THROWS_AS( [&] {
        map_type map6(big_map);
        utils::suppress_unused_warning(map6);
    }(), const std::bad_alloc);
}
#endif // TBB_USE_EXCEPTIONS

//! \brief \ref error_guessing
TEST_CASE("swap with NotAlwaysEqualAllocator allocators") {
    using allocator_type = NotAlwaysEqualAllocator<std::pair<const int, int>>;
    using map_type = tbb::concurrent_hash_map<int, int, tbb::tbb_hash_compare<int>, allocator_type>;

    map_type map1{};
    map_type map2({{42, 42}, {24, 42}});
    map_type map3(map2);

    swap(map1, map2);

    CHECK(map2.empty());
    CHECK(map1 == map3);
}

//! \brief \ref error_guessing
TEST_CASE("test concurrent_hash_map mutex customization") {
    test_mutex_customization();
}

// A test for an issue when const_accessor passed to find provides write access into the map after the lookup
//! \brief \ref regression
TEST_CASE("test concurrent_hash_map accessors issue") {
    using map_type = tbb::concurrent_hash_map<int, int, SimpleTransparentHashCompare>;
    using accessor = IsWriterAccessor<typename map_type::accessor>;
    using const_accessor = IsWriterAccessor<typename map_type::const_accessor>;

    test_chmap_access_mode<map_type, accessor>(/*expect_write = */true);
    test_chmap_access_mode<map_type, const_accessor>(/*expect_write = */false);
}

#if __TBB_CPP20_CONCEPTS_PRESENT
template <bool ExpectSatisfies, typename Key, typename Mapped, typename... HCTypes>
    requires (... && (utils::well_formed_instantiation<tbb::concurrent_hash_map, Key, Mapped, HCTypes> == ExpectSatisfies))
void test_chmap_hash_compare_constraints() {}

//! \brief \ref error_guessing
TEST_CASE("tbb::concurrent_hash_map hash_compare constraints") {
    using key_type = int;
    using mapped_type = int;
    using namespace test_concepts::hash_compare;

    test_chmap_hash_compare_constraints</*Expected = */true, /*key = */key_type, /*mapped = */mapped_type,
                                        Correct<key_type>, tbb::tbb_hash_compare<key_type>>();

    test_chmap_hash_compare_constraints</*Expected = */false, /*key = */key_type, /*mapped = */mapped_type,
                                        NonCopyable<key_type>, NonDestructible<key_type>,
                                        NoHash<key_type>, HashNonConst<key_type>, WrongInputHash<key_type>, WrongReturnHash<key_type>,
                                        NoEqual<key_type>, EqualNonConst<key_type>,
                                        WrongFirstInputEqual<key_type>, WrongSecondInputEqual<key_type>, WrongReturnEqual<key_type>>();
}

template <bool ExpectSatisfies, typename Key, typename Mapped, typename... RWMutexes>
    requires (... && (utils::well_formed_instantiation<tbb::concurrent_hash_map, Key, Mapped,
                                                tbb::tbb_hash_compare<Key>, tbb::tbb_allocator<std::pair<const Key, Mapped>>, RWMutexes> == ExpectSatisfies))
void test_chmap_mutex_constraints() {}

//! \brief \ref error_guessing
TEST_CASE("tbb::concurrent_hash_map rw_mutex constraints") {
    using key_type = int;
    using mapped_type = int;
    using namespace test_concepts::rw_mutex;

    test_chmap_mutex_constraints</*Expected = */true, key_type, mapped_type,
                                 Correct>();

    test_chmap_mutex_constraints</*Expected = */false, key_type, mapped_type,
                                 NoScopedLock, ScopedLockNoDefaultCtor, ScopedLockNoMutexCtor,
                                 ScopedLockNoDtor, ScopedLockNoAcquire, ScopedLockWrongFirstInputAcquire, ScopedLockWrongSecondInputAcquire, ScopedLockNoTryAcquire,
                                 ScopedLockWrongFirstInputTryAcquire, ScopedLockWrongSecondInputTryAcquire, ScopedLockWrongReturnTryAcquire, ScopedLockNoRelease,
                                 ScopedLockNoUpgrade, ScopedLockWrongReturnUpgrade, ScopedLockNoDowngrade, ScopedLockWrongReturnDowngrade,
                                 ScopedLockNoIsWriter, ScopedLockIsWriterNonConst, ScopedLockWrongReturnIsWriter>();
}

//! \brief \ref error_guessing
TEST_CASE("container_range concept for tbb::concurrent_hash_map ranges") {
    static_assert(test_concepts::container_range<tbb::concurrent_hash_map<int, int>::range_type>);
    static_assert(test_concepts::container_range<tbb::concurrent_hash_map<int, int>::const_range_type>);
}

#endif // __TBB_CPP20_CONCEPTS_PRESENT
