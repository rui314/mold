# oneTBB Versioning Policies

There are several types of "versioning" used by the oneTBB project.

1. The *oneTBB specification version*
2. The *official oneTBB library version*
3. The *library interface version*
4. *API versioning* used to maintain backwards compatibility

The oneTBB project uses semantic versioning (https://semver.org/) and as a oneAPI Library,
the oneTBB project makes commitments around compatibility as described
[here](https://www.intel.com/content/www/us/en/docs/oneapi/programming-guide/2024-1/oneapi-library-compatibility.html).
With semantic versioning, a MAJOR version increment generally means an incompatible API change,
while a MINOR version increment means added functionality that is implemented in a backwards compatible manner.

## The oneTBB Specification Version

The oneTBB project is governed by the [UXL foundation](https://uxlfoundation.org/) and has
[an open specification](https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/nested-index).
Each release of the specification has a version number.

[As described in the specification](https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/configuration/version_information),
the value of the `ONETBB_SPEC_VERSION` macro in `oneapi/tbb/version.h` is  latest specification of oneTBB fully
supported by the implementation.

## The oneTBB Library Version

The oneTBB library version can be thought of as the name of the release. If you want to find a specific
release in the repository (https://github.com/uxlfoundation/oneTBB), the tag you would look for corresponds
to the oneTBB library version.

[As described in the specification](https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/configuration/version_information),
the macros `TBB_VERSION_MAJOR`, `TBB_VERSION_MINOR`, `TBB_VERSION_STRING` in `oneapi/tbb/version.h`
provide the library version of the implementation that is being compiled against. The function
`const char* TBB_runtime_version();` can be used to query the library version of the oneTBB binary library that is
loaded at runtime.

## The oneTBB Interface Version

The oneTBB interface version is the public contract about the interface to the library including the
functions exported by the binary library and the types defined in the headers. The oneTBB library is designed
for backwards compatibility. It works well in the scenarios where components compile/link against a TBB version
X.Y. And then at runtime, the application loads a library version X.Z, where X.Z >= X.Y.

A minor version may add new functionality in the headers or in the binary library, including new entry points to the
binary library. An application compiled against an older minor version could not have used this new functionality
and therefore can safely use the newer binary library.

[As described in the specification](https://oneapi-spec.uxlfoundation.org/specifications/oneapi/latest/elements/onetbb/source/configuration/version_information),
the macros `TBB_INTERFACE_VERSION_MAJOR`, `TBB_INTERFACE_VERSION_MINOR`, `TBB_INTERFACE_VERSION` in `oneapi/tbb/version.h`
can be used to determine the interface version of the implementation that is being compiled against. The function
`int TBB_runtime_interface_version();` can be used to query the interface version of the oneTBB binary library that is
loaded at runtime.

### Shared Library Practices for Linux

On Linux we use a symlink system for binary libraries in the oneTBB distribution, and also explicitly set a soname.
The actual shared library is libtbb.so.X.Y but it is built with a –soname that uses only the major version number,
libtbb.so.X.  In addition, there is a symlink from libtbb.so.X -> libtbb.so.X.Y and a symlink from
libtbb.so -> libtbb.so.X.  Therefore, whether you build against libtbb.so, libtbb.so.X or libtbb.so.X.Y, your
application will have a dependency on libtbb.so.X.

A common development process is to compile and link against a library version X.Y. This creates
a dependency on libtbb.so.X. The packaged project then redistributes a TBB shared library that 
is X.Z, where X.Z >= X.Y.

In the (unusual) case where the package does not redistribute a TBB library and instead depends on a version already 
available on the install platform, it is safest to build against the oldest version you expect to encounter; in the
extreme this would be X.1. Of course, building against X.1 means no additional functionalility added in any later minor 
release can be used during the development of the application.

### Shared Library Practices for Windows

On Windows, the TBB binary library has only the major version number, tbb12.dll.

## API versioning used to maintain backwards compatibility

This section is targeted at oneTBB contributors and will likely not be of interest to users of the
oneTBB library.

As mentioned previously, minor release may add new functionality, including modified definitions of classes 
in the headers or new entry points into the binary library.  To safely add this new functionality in a way 
that avoids conflicts, oneTBB has namespaces for class definitions in the headers (that start with the 
letter "d") and namespaces for symbols exported by the binary runtime library (that start with the 
letter "r").

Below is an example of
[a function exported](https://github.com/uxlfoundation/oneTBB/blob/45c2298727d09556a523d6aeaec84ef23872eccf/src/tbb/parallel_pipeline.cpp#L446)
by the oneTBB runtime library:

    namespace tbb {
    namespace detail {
    namespace r1 {
        void __TBB_EXPORTED_FUNC parallel_pipeline(d1::task_group_context& cxt,
                                                   std::size_t max_token,
                                                   const d1::filter_node& fn);
    }
    }
    }

This function is in the runtime namespace `r1` and takes `d1` versions of `task_group_context` and 
`filter_node` classes as arguments. Backward incompatible changes added to the library can be added 
to namespaces with incremented version numbers.

Rules for the `d` namespace include:

- If the layout of public class X is changed incompatibly, the “d” namespace number should be incremented.
- If there are existing entry points using the current version of class X, the old version of X should 
be kept in the previous “d” namespace.

## API and ABI changes by release

For a detailed table of API and ABI changes across all oneTBB releases, see:
[API and ABI Changes by Release](https://uxlfoundation.github.io/oneTBB/main/intro/api_abi_changes.html)

