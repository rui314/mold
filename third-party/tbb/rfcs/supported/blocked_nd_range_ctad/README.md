# Enabling CTAD for blocked_nd_range

## Introduction

`oneapi::tbb::blocked_nd_range` class was introduced as a representation for recursively divisible N-dimensional range for oneTBB parallel algorithms.
This document describes extending its API with the deduction guides since C++17 that allows dropping the explicit template arguments specification while
creating the object if they can be determined using the arguments provided:

```cpp
oneapi::tbb::blocked_range<int> range1(0, 100);
oneapi::tbb::blocked_range<int> range2(0, 200);
oneapi::tbb::blocked_range<int> range3(0, 300);

// Since 3 ranges of type int are provided, the type of nd_range
// can be deduced as oneapi::tbb::blocked_nd_range<int, 3>
oneapi::tbb::blocked_nd_range nd_range(range1, range2, range3);
```

## Design description

### Supported constructors

The `oneapi::tbb::blocked_nd_range` supports the following set of constructors:

```cpp
template <typename T, unsigned int N>\
class blocked_nd_range {
public:
    using value_type;
    using dim_range_type = blocked_range<value_type>;
    using size_type = typename dim_range_type::size_type;

    blocked_nd_range(const dim_range_type& dim0, /*exactly N arguments of type const dim_range_type&*/); // [1]
    blocked_nd_range(const value_type (&dim_size)[N], size_type grainsize = 1);                          // [2]
    blocked_nd_range(blocked_nd_range& r, split);                                                        // [3]
    blocked_nd_range(blocked_nd_range& r, proportional_split proportion);                                // [4]
};
```

The constructor `[1]` is intended to create an n-dimensional interval by providing N one-dimensional ranges. Each element represents the
dimension of the N-dimensional interval being constructed.
It also allows to construct these one-dimensional intervals in-place from braced-inclosed initializer lists:

```cpp
// Passing blocked_range objects itself
tbb::blocked_range<int> dim_range(0, 100);

tbb::blocked_nd_range<int, 2> nd_range_1(dim_range, tbb::blocked_range<int>(0, 200));

// Constructing in-place from braced-init-lists
tbb::blocked_nd_range<int, 2> nd_range_2({0, 100}, {0, 200, 5});

// Combined approach
tbb::blocked_nd_range<int, 2> nd_range_3(dim_range, {0, 200, 5});
```

The constructor `[2]` is intended to create an interval by providing a C-array each element of which represents a size of the corresponding
dimension of the interval being created. This constructor also allows to pass braced-init-list instead of the array from stack:

```cpp
// Passing array object itself
int sizes[3] = {100, 200, 300};

// Constructing the 3-dim range [0, 100), [0, 200), [0, 300)
tbb::blocked_nd_range<int, 3> nd_range_1(sizes);

// Using the grainsize - each dim range will have grainsize 5
tbb::blocked_nd_range<int, 3> nd_range_2(sizes, 5);

// Passing the braced-init-list
tbb::blocked_nd_range<int, 3> nd_range_3({100, 200, 300});
```

In case of passing the template arguments explicitly, using a braced-init-list in both constructors `[1]` and `[2]` does not introduce any 
ambiguity since if the number of braced-init lists provided is always equal to the number of dimensions of the range for constructor `[1]` and
the number of elements in the braced-init list equal to the number of dimensions for constructor `[2]`. 

Constructors `[3]` and `[4]` are intended to split the range into two parts. They are part of _Range_ Named Requirements and used internally in the
implementation of oneTBB parallel algorithms.

### Deduction guides

The following explicit deduction guides for ``blocked_nd_range`` class are supported by oneTBB implementation:

```cpp
// [g1]
template <typename Value, typename... Values>
blocked_nd_range(blocked_range<Value>, blocked_range<Values>...)
-> blocked_nd_range<Value, 1 + sizeof...(Values)>;
```

This deduction guide corresponds to the constructor `[1]` for the case of passing _N_ `blocked_range` objects itself.
It only participates in overload resolution if all of the types in `Values` are same as `Value`.

To cover the case when blocked_ranges are passed as braced-init-lists, a deduction guide is added that takes a set of C-array objects.


There are currently two options how to define the deduction guide (or a function) taking the braced-init-list
of any type- C-array and `std::initializer_list`. The issue with `std::initializer_list` is that it does not allow
tracking the size in compile-time. 

```cpp
// [g2]
template <typename Value, unsigned int... Ns>
blocked_nd_range(const Value (&...)[Ns])
-> blocked_nd_range<Value, sizeof...(Ns)>;
```

This deduction guide only participates in overload resolution if
1. the number of C-arrays provided is $>=2$ (`sizeof...(Ns) >= 2`),
2. Each C-array has the size 2 or 3.

The first constraint is intended to disambiguate between `[1]` and `[2]`.
See [separate section](#ambiguity-while-passing-the-single-braced-init-list-of-size-2-or-3) for more details.

The second constraint is intended to accept only the braced-init-lists that can be used to initialize the `blocked_range` object.
Currently it supports the constructor with 2 parameters, taking _begin_ and _end_ of the interval, and the constructor with 3 parameters, taking
additional _grainsize_ parameter.

The important limitation of the deduction guide `[g2]` is that all of the elements in the braced-init-list should be of the same type. 
It would be impossible to use this constructor for initializing the `blocked_range` objects of types that are not convertible to `size_type`
together with the grainsize:

```cpp
std::vector<int> vector;

// OK, deduced as blocked_nd_range<iterator, 1>
blocked_nd_range range1({vector.begin(), vector.end()});

// FAIL, all items in braced-init-lists should be objects of the same type
// It is impossible to provide grainsize as iterator since iterator is not convertible to size_type
blocked_nd_range range({vector.begin(), vector.end(), /*grainsize = */5});
```

For the constructor `[2]`, the following deduction guide is provided:

```cpp
// [g3]
template <typename Value, unsigned int N>
blocked_nd_range(const Value (&)[N])
```

For service constructors `[3]` and `[4]`, the following guides are provided:

```cpp
// [g4]
template <typename Value, unsigned int N>
blocked_nd_range(blocked_nd_range<Value, N>, split)
-> blocked_nd_range<Value, N>;

// [g5]
template <typename Value, unsigned int N>
blocked_nd_range(blocked_nd_range<Value, N>, proportional_split)
-> blocked_nd_range<Value, N>;
```

From the specification perspective, such a deduction guides can be generated as implicit deduction guides, in the same manner as copy and move constructors.
But the current oneTBB implementation, these deduction guides are not generated implicitly, so the explicit guides are required.
Guides `[g4]` and `[g5]` are no part of the spec, only a part of oneTBB implementation.

## Design conclusions

### Ambiguity while passing the single braced-init-list of size 2 or 3

While using the CTAD with `blocked_nd_range`, there is an ambiguity between two approaches while using a single braced-init-list of size 2 or 3:

```cpp
blocked_nd_range range1({10, 20});
blocked_nd_range range2({10, 20, 5});
```

Since the template arguments for `blocked_nd_range` are not specified, there can be two possible resolutions:
1. Be interpreted as one-dimensional range _[10, 20)_ (with grainsize 1 or 5). In this case it should be deduced as `blocked_nd_range<int, 1>` and 
   constructed using the constructor `[1]`.
2. Be interpreted as two (or three) dimensional range _[0, 10)_, _[0, 20)_ (and _[0, 5)_). In this case it should be deduced as `blocked_nd_range<int, 3>` 
   and constructed using the constructor `[2]`. 

The current oneTBB implementation prefers the second option since the multi-dimensional ranges are more practical and for consistency with the behavior described
in [Passing single C-array object of size 2 or 3](#passing-single-c-array-object-of-size-2-or-3) section.

### Passing single C-array object of size 2 or 3

Another interesting issue that should be resolved, is passing the single C-array object of size 2 or 3 to the constructor:

```cpp
int array[2] = {100, 200};
tbb::blocked_nd_range range(array);
```

Since the `blocked_range` is not constructible from C-array and the braced-init-list is not used, the user expects the range to be deduced as
`blocked_nd_range<int, 2>` and the constructor `[2]` to be used.

Current oneTBB implementation supports such a behavior constraining a braced-init-list deduction guide to allow 2 or more lists and hence
by resolving an ambiguity described in
[Ambiguity while passing the single braced-init-list of size 2 or 3](#ambiguity-while-passing-the-single-braced-init-list-of-size-2-or-3)
to always prefer a multi-dimensional range.

### Using the constructor `[1]` with "mixed" arguments

There is a limitation of the deduction guides if the constructor `[1]` is used with both arguments of exact `tbb::blocked_range` type
and the braced-init-lists:

```cpp
tbb::blocked_range<int> dim_range(0, 100);
tbb::blocked_nd_range nd_range(dim_range, {0, 200}, {0, 300}, dim_range);
```

These arguments would not match neither on the `[g1]` not `[g2]` and it is unclear how to define the deduction guide that covers this case.
