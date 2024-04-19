# oneAPI Threading Building Blocks
[![Apache License Version 2.0](https://img.shields.io/badge/license-Apache_2.0-green.svg)](LICENSE.txt) [![oneTBB CI](https://github.com/oneapi-src/oneTBB/actions/workflows/ci.yml/badge.svg)](https://github.com/oneapi-src/oneTBB/actions/workflows/ci.yml?query=branch%3Amaster)

oneTBB is a flexible C++ library that simplifies the work of adding parallelism
to complex applications, even if you are not a threading expert.  

The library lets you easily write parallel programs that take full advantage of the multi-core performance. Such programs are portable, 
composable and have a future-proof scalability. oneTBB provides you with functions, interfaces, and classes to parallelize and scale the code.
All you have to do is to use the templates. 

The library differs from typical threading packages in the following ways:
* oneTBB enables you to specify logical parallelism instead of threads.
* oneTBB targets threading for performance.
* oneTBB is compatible with other threading packages.
* oneTBB emphasizes scalable, data parallel programming.
* oneTBB relies on generic programming.


Refer to oneTBB [examples](examples) and [samples](https://github.com/oneapi-src/oneAPI-samples/tree/master/Libraries/oneTBB) to see how you can use the library.

oneTBB is a part of [oneAPI](https://oneapi.io). The current branch implements version 1.1 of oneAPI Specification.

> **_NOTE:_** Threading Building Blocks (TBB) is now called oneAPI Threading Building Blocks (oneTBB) to highlight that the tool is a part of the oneAPI ecosystem.

## Release Information
Here are [Release Notes](RELEASE_NOTES.md) and [System Requirements](SYSTEM_REQUIREMENTS.md).

## Documentation
* [oneTBB Specification](https://spec.oneapi.com/versions/latest/elements/oneTBB/source/nested-index.html)
* [oneTBB Developer Guide and Reference](https://oneapi-src.github.io/oneTBB)
* [Migrating from TBB to oneTBB](https://oneapi-src.github.io/oneTBB/main/tbb_userguide/Migration_Guide.html)
* [README for the CMake build system](cmake/README.md)
* [oneTBB Testing Approach](https://oneapi-src.github.io/oneTBB/main/intro/testing_approach.html)
* [Basic support for the Bazel build system](Bazel.md)
* [oneTBB Discussions](https://github.com/oneapi-src/oneTBB/discussions)
* [WASM Support](WASM_Support.md)

## Installation 
See [Installation from Sources](INSTALL.md) to learn how to install oneTBB. 

## Support
Please report issues and suggestions via [GitHub issues](https://github.com/oneapi-src/oneTBB/issues). See our [documentation](./CONTRIBUTING.md##Issues) to learn how to work with them.

## How to Contribute
We welcome community contributions, so check our [Contributing Guidelines](CONTRIBUTING.md)
to learn more.

## License
oneAPI Threading Building Blocks is licensed under [Apache License, Version 2.0](LICENSE.txt).
By its terms, contributions submitted to the project are also done under that license.


## Engineering team contacts
* [Email us.](mailto:inteltbbdevelopers@intel.com)

------------------------------------------------------------------------
\* All names and brands may be claimed as the property of others.
