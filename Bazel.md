# Bazel build support

The main build system of oneTBB is CMake.
Bazel support is community-based.
The maintainers do not use Bazel internally.
The Bazel configuration may not include recommended compiler and linker flags used in official CMake configuration.

The Bazel build of oneTBB currently only aims for a subset of oneTBB that suffices restricted use cases of the usage of oneTBB.
Pull requests to improve the Bazel build experience are welcomed.

The standard approach of how Bazel handles third-party libraries is static linking. 
Even this is not recommended by the oneTBB maintainers this is chosen since this is considered as the best practice in the Bazel ecosystem.

## Example usage

1. [Install Bazel](https://docs.bazel.build/versions/main/install.html).

2. Create the following files in one folder (this folder will be considered as the workspace folder):

_WORKSPACE.bazel_:
```
load("@bazel_tools//tools/build_defs/repo:git.bzl", "git_repository")

git_repository(
    name = "oneTBB",
    branch = "master",
    remote = "https://github.com/oneapi-src/oneTBB/",
)
```

_BUILD_:
```
cc_binary(
    name = "Demo",
    srcs = ["main.cpp"],
    deps = ["@oneTBB//:tbb"],
)
```

_main.cpp_:
```
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

3. Switch to the folder where you create the files.

4. Execute the command `bazel run //:Demo`.
As an expected output, you should see the oneTBB version.
