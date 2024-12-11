# Bazel* build support

The main build system of oneTBB is CMake*.
[Bazel*](https://bazel.build/) support is community-based.
The Bazel configuration may not include recommended compiler and/or linker flags used in the official CMake configuration.

---
**NOTE**

Bazel is not recommended for use by oneTBB maintainers. Thus, it is not used internally. 

---


The Bazel oneTBB build is currently only intended for a subset of oneTBB that suffices restricted use cases.
Pull requests to improve the Bazel build experience are welcome.

The standard Bazel approach to handling third-party libraries is static linking. It is the best practice within the Bazel ecosystem.

## Using oneTBB as a dependency

### Traditional WORKSPACE approach

This example demonstrates how to use oneTBB as a dependency within a Bazel project.

The following file structure is assumed:

```
example
├── .bazelrc
├── BUILD.bazel
├── main.cpp
└── WORKSPACE.bazel
```

_WORKSPACE.bazel_:
```python
load("@platforms//tools/build_defs/repo:git.bzl", "git_repository")

git_repository(
    name = "oneTBB",
    branch = "master",
    remote = "https://github.com/oneapi-src/oneTBB/",
)
```

In the *WORKSPACE* file, the oneTBB GitHub* repository is fetched. 

_BUILD.bazel_:

```python
cc_binary(
    name = "Demo",
    srcs = ["main.cpp"],
    deps = ["@oneTBB//:tbb"],
)
```

The *BUILD* file defines a binary named `Demo` that has a dependency to oneTBB.

_main.cpp_:

```c++
#include "oneapi/tbb/version.h"

#include <iostream>

int main() {
    std::cout << "Hello from oneTBB "
              << TBB_VERSION_MAJOR << "."
              << TBB_VERSION_MINOR << "."
              << TBB_VERSION_PATCH
              << "!" << std::endl;

    return 0;
}
```

The expected output of this program is the current version of oneTBB.

Switch to the folder with the files created earlier and run the binary with `bazel run //:Demo`.

### Bzlmod

If you use Bzlmod, you can fetch oneTBB with the [Bazel Central Registry](https://registry.bazel.build/).

Add the following line to your `MODULE.bazel` file:

```bazel
bazel_dep(name = "onetbb", version = "2021.13.0")
```

## Build oneTBB using Bazel

Run ```bazel build //...``` in the oneTBB root directory.

## Compiler support

The Bazel build uses the compiler flag `-mwaitpkg` in non-Windows* builds.
This flag is supported by the GNU* Compiler Collection (GCC) version 9.3, Clang* 12, and newer versions of those tools.


---
**NOTE**

To use the Bazel build with earlier versions of GCC, remove `-mwaitpkg` flag as it leads to errors during compilation.

---
