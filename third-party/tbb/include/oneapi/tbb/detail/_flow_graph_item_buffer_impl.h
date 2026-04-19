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

#ifndef __TBB__flow_graph_item_buffer_impl_H
#define __TBB__flow_graph_item_buffer_impl_H

#ifndef __TBB_flow_graph_H
#error Do not #include this internal file directly; use public TBB headers instead.
#endif

#include "_aligned_space.h"

// in namespace tbb::flow::interfaceX (included in _flow_graph_node_impl.h)

//! Expandable buffer of items.  The possible operations are push, pop,
//* tests for empty and so forth.  No mutual exclusion is built in.
//* objects are constructed into and explicitly-destroyed.  get_my_item gives
// a read-only reference to the item in the buffer.  set_my_item may be called
// with either an empty or occupied slot.

template <typename T, typename A=cache_aligned_allocator<T> >
class item_buffer {
public:
    typedef T item_type;
    enum buffer_item_state { no_item=0, has_item=1, reserved_item=2 };
protected:
    struct aligned_space_item {
        item_type item;
        buffer_item_state state;
#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
        message_metainfo metainfo;
#endif
    };
    typedef size_t size_type;
    typedef aligned_space<aligned_space_item> buffer_item_type;
    typedef typename allocator_traits<A>::template rebind_alloc<buffer_item_type> allocator_type;
    buffer_item_type *my_array;
    size_type my_array_size;
    static const size_type initial_buffer_size = 4;
    size_type my_head;
    size_type my_tail;

    bool buffer_empty() const { return my_head == my_tail; }

    aligned_space_item &element(size_type i) {
        __TBB_ASSERT(!(size_type(&(my_array[i&(my_array_size-1)].begin()->state))%alignment_of<buffer_item_state>::value), nullptr);
        __TBB_ASSERT(!(size_type(&(my_array[i&(my_array_size-1)].begin()->item))%alignment_of<item_type>::value), nullptr);
#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
        __TBB_ASSERT(!(size_type(&(my_array[i&(my_array_size-1)].begin()->metainfo))%alignment_of<message_metainfo>::value), nullptr);
#endif
        return *my_array[i & (my_array_size - 1) ].begin();
    }

    const aligned_space_item &element(size_type i) const {
        __TBB_ASSERT(!(size_type(&(my_array[i&(my_array_size-1)].begin()->state))%alignment_of<buffer_item_state>::value), nullptr);
        __TBB_ASSERT(!(size_type(&(my_array[i&(my_array_size-1)].begin()->item))%alignment_of<item_type>::value), nullptr);
#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
        __TBB_ASSERT(!(size_type(&(my_array[i&(my_array_size-1)].begin()->metainfo))%alignment_of<message_metainfo>::value), nullptr);
#endif
        return *my_array[i & (my_array_size-1)].begin();
    }

    bool my_item_valid(size_type i) const { return (i < my_tail) && (i >= my_head) && (element(i).state != no_item); }
#if TBB_USE_ASSERT
    bool my_item_reserved(size_type i) const { return element(i).state == reserved_item; }
#endif

    // object management in buffer
    const item_type &get_my_item(size_t i) const {
        __TBB_ASSERT(my_item_valid(i),"attempt to get invalid item");
        return element(i).item;
    }

#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
    message_metainfo& get_my_metainfo(size_t i) {
        __TBB_ASSERT(my_item_valid(i), "attempt to get invalid item");
        return element(i).metainfo;
    }
#endif

    // may be called with an empty slot or a slot that has already been constructed into.
    void set_my_item(size_t i, const item_type &o
                     __TBB_FLOW_GRAPH_METAINFO_ARG(const message_metainfo& metainfo))
    {
        if(element(i).state != no_item) {
            destroy_item(i);
        }
        new(&(element(i).item)) item_type(o);
        element(i).state = has_item;
#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
        new(&element(i).metainfo) message_metainfo(metainfo);

        for (auto& waiter : metainfo.waiters()) {
            waiter->reserve(1);
        }
#endif
    }

#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
    void set_my_item(size_t i, const item_type& o, message_metainfo&& metainfo) {
        if(element(i).state != no_item) {
            destroy_item(i);
        }

        new(&(element(i).item)) item_type(o);
        new(&element(i).metainfo) message_metainfo(std::move(metainfo));
        // Skipping the reservation on metainfo.waiters since the ownership
        // is moving from metainfo to the cache
        element(i).state = has_item;
    }
#endif

    // destructively-fetch an object from the buffer
#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
    void fetch_item(size_t i, item_type& o, message_metainfo& metainfo) {
        __TBB_ASSERT(my_item_valid(i), "Trying to fetch an empty slot");
        o = get_my_item(i);  // could have std::move assign semantics
        metainfo = std::move(get_my_metainfo(i));
        destroy_item(i);
    }
#else
    void fetch_item(size_t i, item_type &o) {
        __TBB_ASSERT(my_item_valid(i), "Trying to fetch an empty slot");
        o = get_my_item(i);  // could have std::move assign semantics
        destroy_item(i);
    }
#endif

    // move an existing item from one slot to another.  The moved-to slot must be unoccupied,
    // the moved-from slot must exist and not be reserved.  The after, from will be empty,
    // to will be occupied but not reserved
    void move_item(size_t to, size_t from) {
        __TBB_ASSERT(!my_item_valid(to), "Trying to move to a non-empty slot");
        __TBB_ASSERT(my_item_valid(from), "Trying to move from an empty slot");
        // could have std::move semantics
        set_my_item(to, get_my_item(from) __TBB_FLOW_GRAPH_METAINFO_ARG(get_my_metainfo(from)));
        destroy_item(from);
    }

    // put an item in an empty slot.  Return true if successful, else false
#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
    template <typename Metainfo>
    bool place_item(size_t here, const item_type &me, Metainfo&& metainfo) {
#if !TBB_DEPRECATED_SEQUENCER_DUPLICATES
        if(my_item_valid(here)) return false;
#endif
        set_my_item(here, me, std::forward<Metainfo>(metainfo));
        return true;
    }
#else
    bool place_item(size_t here, const item_type &me) {
#if !TBB_DEPRECATED_SEQUENCER_DUPLICATES
        if(my_item_valid(here)) return false;
#endif
        set_my_item(here, me);
        return true;
    }
#endif

    // could be implemented with std::move semantics
    void swap_items(size_t i, size_t j) {
        __TBB_ASSERT(my_item_valid(i) && my_item_valid(j), "attempt to swap invalid item(s)");
        item_type temp = get_my_item(i);
#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
        message_metainfo temp_metainfo = get_my_metainfo(i);
        set_my_item(i, get_my_item(j), get_my_metainfo(j));
        set_my_item(j, temp, temp_metainfo);
#else
        set_my_item(i, get_my_item(j));
        set_my_item(j, temp);
#endif
    }

    void destroy_item(size_type i) {
        __TBB_ASSERT(my_item_valid(i), "destruction of invalid item");

        auto& e = element(i);
        e.item.~item_type();
        e.state = no_item;

#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
        for (auto& msg_waiter : e.metainfo.waiters()) {
            msg_waiter->release(1);
        }

        e.metainfo.~message_metainfo();
#endif
    }

    // returns the front element
    const item_type& front() const
    {
        __TBB_ASSERT(my_item_valid(my_head), "attempt to fetch head non-item");
        return get_my_item(my_head);
    }

#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
    const message_metainfo& front_metainfo() const
    {
        __TBB_ASSERT(my_item_valid(my_head), "attempt to fetch head non-item");
        return element(my_head).metainfo;
    }
#endif

    // returns  the back element
    const item_type& back() const
    {
        __TBB_ASSERT(my_item_valid(my_tail - 1), "attempt to fetch head non-item");
        return get_my_item(my_tail - 1);
    }

#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
    const message_metainfo& back_metainfo() const {
        __TBB_ASSERT(my_item_valid(my_tail - 1), "attempt to fetch head non-item");
        return element(my_tail - 1).metainfo;
    }
#endif

    // following methods are for reservation of the front of a buffer.
    void reserve_item(size_type i) {
        __TBB_ASSERT(my_item_valid(i) && !my_item_reserved(i), "item cannot be reserved");
        element(i).state = reserved_item;
    }

    void release_item(size_type i) {
        __TBB_ASSERT(my_item_reserved(i), "item is not reserved");
        element(i).state = has_item;
    }

    void destroy_front() { destroy_item(my_head); ++my_head; }
    void destroy_back() { destroy_item(my_tail-1); --my_tail; }

    // we have to be able to test against a new tail value without changing my_tail
    // grow_array doesn't work if we change my_tail when the old array is too small
    size_type size(size_t new_tail = 0) { return (new_tail ? new_tail : my_tail) - my_head; }
    size_type capacity() { return my_array_size; }
    // sequencer_node does not use this method, so we don't
    // need a version that passes in the new_tail value.
    bool buffer_full() { return size() >= capacity(); }

    //! Grows the internal array.
    void grow_my_array( size_t minimum_size ) {
        // test that we haven't made the structure inconsistent.
        __TBB_ASSERT(capacity() >= my_tail - my_head, "total items exceed capacity");
        size_type new_size = my_array_size ? 2*my_array_size : initial_buffer_size;
        while( new_size<minimum_size )
            new_size*=2;

        buffer_item_type* new_array = allocator_type().allocate(new_size);

        // initialize validity to "no"
        for( size_type i=0; i<new_size; ++i ) { new_array[i].begin()->state = no_item; }

        for( size_type i=my_head; i<my_tail; ++i) {
            if(my_item_valid(i)) {  // sequencer_node may have empty slots
                // placement-new copy-construct; could be std::move
                char *new_space = (char *)&(new_array[i&(new_size-1)].begin()->item);
                (void)new(new_space) item_type(get_my_item(i));
                new_array[i&(new_size-1)].begin()->state = element(i).state;
#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
                char* meta_space = (char *)&(new_array[i&(new_size-1)].begin()->metainfo);
                ::new(meta_space) message_metainfo(std::move(element(i).metainfo));
#endif
            }
        }

        clean_up_buffer(/*reset_pointers*/false);

        my_array = new_array;
        my_array_size = new_size;
    }

    bool push_back(item_type& v
                   __TBB_FLOW_GRAPH_METAINFO_ARG(const message_metainfo& metainfo))
    {
        if (buffer_full()) {
            grow_my_array(size() + 1);
        }
        set_my_item(my_tail, v __TBB_FLOW_GRAPH_METAINFO_ARG(metainfo));
        ++my_tail;
        return true;
    }

    bool pop_back(item_type& v
                  __TBB_FLOW_GRAPH_METAINFO_ARG(message_metainfo& metainfo))
    {
        if (!my_item_valid(my_tail - 1)) {
            return false;
        }
        auto& e = element(my_tail - 1);
        v = e.item;
#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
        metainfo = std::move(e.metainfo);
#endif

        destroy_back();
        return true;
    }

    bool pop_front(item_type& v
                   __TBB_FLOW_GRAPH_METAINFO_ARG(message_metainfo& metainfo))
    {
        if (!my_item_valid(my_head)) {
            return false;
        }
        auto& e = element(my_head);
        v = e.item;
#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
        metainfo = std::move(e.metainfo);
#endif

        destroy_front();
        return true;
    }

#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
    bool pop_back(item_type& v) {
        message_metainfo metainfo;
        return pop_back(v, metainfo);
    }

    bool pop_front(item_type& v) {
        message_metainfo metainfo;
        return pop_front(v, metainfo);
    }
#endif

    // This is used both for reset and for grow_my_array.  In the case of grow_my_array
    // we want to retain the values of the head and tail.
    void clean_up_buffer(bool reset_pointers) {
        if (my_array) {
            for( size_type i=my_head; i<my_tail; ++i ) {
                if(my_item_valid(i))
                    destroy_item(i);
            }
            allocator_type().deallocate(my_array,my_array_size);
        }
        my_array = nullptr;
        if(reset_pointers) {
            my_head = my_tail = my_array_size = 0;
        }
    }

public:
    //! Constructor
    item_buffer( ) : my_array(nullptr), my_array_size(0),
                     my_head(0), my_tail(0) {
        grow_my_array(initial_buffer_size);
    }

    ~item_buffer() {
        clean_up_buffer(/*reset_pointers*/true);
    }

    void reset() { clean_up_buffer(/*reset_pointers*/true); grow_my_array(initial_buffer_size); }

};

//! item_buffer with reservable front-end.  NOTE: if reserving, do not
//* complete operation with pop_front(); use consume_front().
//* No synchronization built-in.
template<typename T, typename A=cache_aligned_allocator<T> >
class reservable_item_buffer : public item_buffer<T, A> {
protected:
    using item_buffer<T, A>::my_item_valid;
    using item_buffer<T, A>::my_head;

public:
    reservable_item_buffer() : item_buffer<T, A>(), my_reserved(false) {}
    void reset() {my_reserved = false; item_buffer<T,A>::reset(); }
protected:

    bool reserve_front(T &v) {
        if(my_reserved || !my_item_valid(this->my_head)) return false;
        my_reserved = true;
        // reserving the head
        v = this->front();
        this->reserve_item(this->my_head);
        return true;
    }

#if __TBB_PREVIEW_FLOW_GRAPH_TRY_PUT_AND_WAIT
    bool reserve_front(T& v, message_metainfo& metainfo) {
        if (my_reserved || !my_item_valid(this->my_head)) return false;
        my_reserved = true;
        // reserving the head
        v = this->front();
        metainfo = this->front_metainfo();
        this->reserve_item(this->my_head);
        return true;
    }
#endif

    void consume_front() {
        __TBB_ASSERT(my_reserved, "Attempt to consume a non-reserved item");
        this->destroy_front();
        my_reserved = false;
    }

    void release_front() {
        __TBB_ASSERT(my_reserved, "Attempt to release a non-reserved item");
        this->release_item(this->my_head);
        my_reserved = false;
    }

    bool my_reserved;
};

#endif // __TBB__flow_graph_item_buffer_impl_H
