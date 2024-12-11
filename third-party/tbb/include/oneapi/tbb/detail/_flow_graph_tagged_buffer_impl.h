/*
    Copyright (c) 2005-2024 Intel Corporation

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

// a hash table buffer that can expand, and can support as many deletions as
// additions, list-based, with elements of list held in array (for destruction
// management), multiplicative hashing (like ets).  No synchronization built-in.
//

#ifndef __TBB__flow_graph_hash_buffer_impl_H
#define __TBB__flow_graph_hash_buffer_impl_H

#ifndef __TBB_flow_graph_H
#error Do not #include this internal file directly; use public TBB headers instead.
#endif

// included in namespace tbb::flow::interfaceX::internal

// elements in the table are a simple list; we need pointer to next element to
// traverse the chain

template <typename Key, typename ValueType>
struct hash_buffer_element : public aligned_pair<ValueType, void*> {
    using key_type = Key;
    using value_type = ValueType;

    value_type* get_value_ptr() { return reinterpret_cast<value_type*>(this->first); }
    hash_buffer_element* get_next() { return reinterpret_cast<hash_buffer_element*>(this->second); }
    void set_next(hash_buffer_element* new_next) { this->second = reinterpret_cast<void*>(new_next); }

    void create_element(const value_type& v) {
        ::new(this->first) value_type(v);
    }

    void create_element(hash_buffer_element&& other) {
        ::new(this->first) value_type(std::move(*other.get_value_ptr()));
    }

    void destroy_element() {
        get_value_ptr()->~value_type();
    }
};

#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
template <typename Key, typename ValueType>
struct metainfo_hash_buffer_element : public aligned_triple<ValueType, void*, message_metainfo> {
    using key_type = Key;
    using value_type = ValueType;

    value_type* get_value_ptr() { return reinterpret_cast<value_type*>(this->first); }
    metainfo_hash_buffer_element* get_next() {
        return reinterpret_cast<metainfo_hash_buffer_element*>(this->second);
    }
    void set_next(metainfo_hash_buffer_element* new_next) { this->second = reinterpret_cast<void*>(new_next); }
    message_metainfo& get_metainfo() { return this->third; }

    void create_element(const value_type& v, const message_metainfo& metainfo) {
        __TBB_ASSERT(this->third.empty(), nullptr);
        ::new(this->first) value_type(v);
        this->third = metainfo;

        for (auto waiter : metainfo.waiters()) {
            waiter->reserve(1);
        }
    }

    void create_element(metainfo_hash_buffer_element&& other) {
        __TBB_ASSERT(this->third.empty(), nullptr);
        ::new(this->first) value_type(std::move(*other.get_value_ptr()));
        this->third = std::move(other.get_metainfo());
    }

    void destroy_element() {
        get_value_ptr()->~value_type();

        for (auto waiter : get_metainfo().waiters()) {
            waiter->release(1);
        }
        get_metainfo() = message_metainfo{};
    }
};
#endif

template
    <
     typename ElementType,
     typename ValueToKey,  // abstract method that returns "const Key" or "const Key&" given ValueType
     typename HashCompare, // has hash and equal
     typename Allocator=tbb::cache_aligned_allocator<ElementType>
    >
class hash_buffer_impl : public HashCompare {
public:
    static const size_t INITIAL_SIZE = 8;  // initial size of the hash pointer table
    typedef typename ElementType::key_type key_type;
    typedef typename ElementType::value_type value_type;
    typedef ElementType element_type;
    typedef value_type *pointer_type;
    typedef element_type *list_array_type;  // array we manage manually
    typedef list_array_type *pointer_array_type;
    typedef typename std::allocator_traits<Allocator>::template rebind_alloc<list_array_type> pointer_array_allocator_type;
    typedef typename std::allocator_traits<Allocator>::template rebind_alloc<element_type> elements_array_allocator;
    typedef typename std::decay<key_type>::type Knoref;

private:
    ValueToKey *my_key;
    size_t my_size;
    size_t nelements;
    pointer_array_type pointer_array;    // pointer_array[my_size]
    list_array_type elements_array;      // elements_array[my_size / 2]
    element_type* free_list;

    size_t mask() { return my_size - 1; }

    void set_up_free_list( element_type **p_free_list, list_array_type la, size_t sz) {
        for(size_t i=0; i < sz - 1; ++i ) {  // construct free list
            la[i].set_next(&(la[i + 1]));
        }
        la[sz - 1].set_next(nullptr);
        *p_free_list = (element_type *)&(la[0]);
    }

    // cleanup for exceptions
    struct DoCleanup {
        pointer_array_type *my_pa;
        list_array_type *my_elements;
        size_t my_size;

        DoCleanup(pointer_array_type &pa, list_array_type &my_els, size_t sz) :
            my_pa(&pa), my_elements(&my_els), my_size(sz) {  }
        ~DoCleanup() {
            if(my_pa) {
                size_t dont_care = 0;
                internal_free_buffer(*my_pa, *my_elements, my_size, dont_care);
            }
        }
    };

    // exception-safety requires we do all the potentially-throwing operations first
    void grow_array() {
        size_t new_size = my_size*2;
        size_t new_nelements = nelements;  // internal_free_buffer zeroes this
        list_array_type new_elements_array = nullptr;
        pointer_array_type new_pointer_array = nullptr;
        list_array_type new_free_list = nullptr;
        {
            DoCleanup my_cleanup(new_pointer_array, new_elements_array, new_size);
            new_elements_array = elements_array_allocator().allocate(my_size);
#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
            for (std::size_t i = 0; i < my_size; ++i) {
                ::new(new_elements_array + i) element_type();
            }
#endif
            new_pointer_array = pointer_array_allocator_type().allocate(new_size);
            for(size_t i=0; i < new_size; ++i) new_pointer_array[i] = nullptr;
            set_up_free_list(&new_free_list, new_elements_array, my_size );

            for(size_t i=0; i < my_size; ++i) {
                for( element_type* op = pointer_array[i]; op; op = (element_type *)(op->get_next())) {
                    internal_insert_with_key(new_pointer_array, new_size, new_free_list, std::move(*op));
                }
            }
            my_cleanup.my_pa = nullptr;
            my_cleanup.my_elements = nullptr;
        }

        internal_free_buffer(pointer_array, elements_array, my_size, nelements);
        free_list = new_free_list;
        pointer_array = new_pointer_array;
        elements_array = new_elements_array;
        my_size = new_size;
        nelements = new_nelements;
    }

    // v should have perfect forwarding if std::move implemented.
    // we use this method to move elements in grow_array, so can't use class fields
    template <typename Value, typename... Args>
    const value_type& get_value_from_pack(const Value& value, const Args&...) {
        return value;
    }

    template <typename Element>
    const value_type& get_value_from_pack(Element&& element) {
        return *(element.get_value_ptr());
    }

    template <typename... Args>
    void internal_insert_with_key( element_type **p_pointer_array, size_t p_sz, list_array_type &p_free_list,
                                   Args&&... args) {
        size_t l_mask = p_sz-1;
        __TBB_ASSERT(my_key, "Error: value-to-key functor not provided");
        size_t h = this->hash(tbb::detail::invoke(*my_key, get_value_from_pack(args...))) & l_mask;
        __TBB_ASSERT(p_free_list, "Error: free list not set up.");
        element_type* my_elem = p_free_list; p_free_list = (element_type *)(p_free_list->get_next());
        my_elem->create_element(std::forward<Args>(args)...);
        my_elem->set_next(p_pointer_array[h]);
        p_pointer_array[h] = my_elem;
    }

    void internal_initialize_buffer() {
        pointer_array = pointer_array_allocator_type().allocate(my_size);
        for(size_t i = 0; i < my_size; ++i) pointer_array[i] = nullptr;
        elements_array = elements_array_allocator().allocate(my_size / 2);
#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
        for (std::size_t i = 0; i < my_size / 2; ++i) {
            ::new(elements_array + i) element_type();
        }
#endif
        set_up_free_list(&free_list, elements_array, my_size / 2);
    }

    // made static so an enclosed class can use to properly dispose of the internals
    static void internal_free_buffer( pointer_array_type &pa, list_array_type &el, size_t &sz, size_t &ne ) {
        if(pa) {
            for(size_t i = 0; i < sz; ++i ) {
                element_type *p_next;
                for( element_type *p = pa[i]; p; p = p_next) {
                    p_next = p->get_next();
                    p->destroy_element();
                }
            }
            pointer_array_allocator_type().deallocate(pa, sz);
            pa = nullptr;
        }
        // Separate test (if allocation of pa throws, el may be allocated.
        // but no elements will be constructed.)
        if(el) {
#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
            for (std::size_t i = 0; i < sz / 2; ++i) {
                (el + i)->~element_type();
            }
#endif
            elements_array_allocator().deallocate(el, sz / 2);
            el = nullptr;
        }
        sz = INITIAL_SIZE;
        ne = 0;
    }

public:
    hash_buffer_impl() : my_key(nullptr), my_size(INITIAL_SIZE), nelements(0) {
        internal_initialize_buffer();
    }

    ~hash_buffer_impl() {
        internal_free_buffer(pointer_array, elements_array, my_size, nelements);
        delete my_key;
        my_key = nullptr;
    }
    hash_buffer_impl(const hash_buffer_impl&) = delete;
    hash_buffer_impl& operator=(const hash_buffer_impl&) = delete;

    void reset() {
        internal_free_buffer(pointer_array, elements_array, my_size, nelements);
        internal_initialize_buffer();
    }

    // Take ownership of func object allocated with new.
    // This method is only used internally, so can't be misused by user.
    void set_key_func(ValueToKey *vtk) { my_key = vtk; }
    // pointer is used to clone()
    ValueToKey* get_key_func() { return my_key; }

    template <typename... Args>
    bool insert_with_key(const value_type &v, Args&&... args) {
        element_type* p = nullptr;
        __TBB_ASSERT(my_key, "Error: value-to-key functor not provided");
        if(find_element_ref_with_key(tbb::detail::invoke(*my_key, v), p)) {
            p->destroy_element();
            p->create_element(v, std::forward<Args>(args)...);
            return false;
        }
        ++nelements;
        if(nelements*2 > my_size) grow_array();
        internal_insert_with_key(pointer_array, my_size, free_list, v, std::forward<Args>(args)...);
        return true;
    }

    bool find_element_ref_with_key(const Knoref& k, element_type*& v) {
        size_t i = this->hash(k) & mask();
        for(element_type* p = pointer_array[i]; p; p = (element_type *)(p->get_next())) {
            __TBB_ASSERT(my_key, "Error: value-to-key functor not provided");
            if(this->equal(tbb::detail::invoke(*my_key, *p->get_value_ptr()), k)) {
                v = p;
                return true;
            }
        }
        return false;
    }

    // returns true and sets v to array element if found, else returns false.
    bool find_ref_with_key(const Knoref& k, pointer_type &v) {
        element_type* element_ptr = nullptr;
        bool res = find_element_ref_with_key(k, element_ptr);
        v = element_ptr->get_value_ptr();
        return res;
    }

    bool find_with_key( const Knoref& k, value_type &v) {
        value_type *p;
        if(find_ref_with_key(k, p)) {
            v = *p;
            return true;
        }
        else
            return false;
    }

    void delete_with_key(const Knoref& k) {
        size_t h = this->hash(k) & mask();
        element_type* prev = nullptr;
        for(element_type* p = pointer_array[h]; p; prev = p, p = (element_type *)(p->get_next())) {
            value_type *vp = p->get_value_ptr();
            __TBB_ASSERT(my_key, "Error: value-to-key functor not provided");
            if(this->equal(tbb::detail::invoke(*my_key, *vp), k)) {
                p->destroy_element();
                if(prev) prev->set_next(p->get_next());
                else pointer_array[h] = (element_type *)(p->get_next());
                p->set_next(free_list);
                free_list = p;
                --nelements;
                return;
            }
        }
        __TBB_ASSERT(false, "key not found for delete");
    }
};

template
    <
     typename Key,         // type of key within ValueType
     typename ValueType,
     typename ValueToKey,  // abstract method that returns "const Key" or "const Key&" given ValueType
     typename HashCompare, // has hash and equal
     typename Allocator=tbb::cache_aligned_allocator<hash_buffer_element<Key, ValueType>>
    >
using hash_buffer = hash_buffer_impl<hash_buffer_element<Key, ValueType>,
                                     ValueToKey, HashCompare, Allocator>;

#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
template
    <
     typename Key,         // type of key within ValueType
     typename ValueType,
     typename ValueToKey,  // abstract method that returns "const Key" or "const Key&" given ValueType
     typename HashCompare, // has hash and equal
     typename Allocator=tbb::cache_aligned_allocator<metainfo_hash_buffer_element<Key, ValueType>>
    >
struct metainfo_hash_buffer : public hash_buffer_impl<metainfo_hash_buffer_element<Key, ValueType>,
                                               ValueToKey, HashCompare, Allocator>
{
private:
    using base_type = hash_buffer_impl<metainfo_hash_buffer_element<Key, ValueType>,
                                       ValueToKey, HashCompare, Allocator>;
public:
    bool find_with_key(const typename base_type::Knoref& k,
                       typename base_type::value_type& v, message_metainfo& metainfo)
    {
        typename base_type::element_type* p = nullptr;
        bool result = this->find_element_ref_with_key(k, p);
        if (result) {
            v = *(p->get_value_ptr());
            metainfo = p->get_metainfo();
        }
        return result;
    }
};
#endif // __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
#endif // __TBB__flow_graph_hash_buffer_impl_H
