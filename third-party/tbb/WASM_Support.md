<!--
******************************************************************************
* 
* Licensed under the Apache License, Version 2.0 (the "License");
* you may not use this file except in compliance with the License.
* You may obtain a copy of the License at
*
*     http://www.apache.org/licenses/LICENSE-2.0
*
* Unless required by applicable law or agreed to in writing, software
* distributed under the License is distributed on an "AS IS" BASIS,
* WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
* See the License for the specific language governing permissions and
* limitations under the License.
*******************************************************************************/-->

# WASM Support

oneTBB extends its capabilities by offering robust support for ``WASM`` (see ``Limitation`` sections).

``WASM`` stands for WebAssembly, a low-level binary format for executing code in web browsers. 
It is designed to be a portable target for compilers and efficient to parse and execute. 

Using oneTBB with WASM, you can take full advantage of parallelism and concurrency while working on web-based applications, interactive websites, and a variety of other WASM-compatible platforms.

oneTBB offers WASM support through the integration with [Emscripten*](https://emscripten.org/docs/introducing_emscripten/index.html), a powerful toolchain for compiling C and C++ code into WASM-compatible runtimes. 

## Build

**Prerequisites:** Download and install Emscripten*. See the [instructions](https://emscripten.org/docs/getting_started/downloads.html). 

To build the system, run:

```
mkdir build && cd build
emcmake cmake .. -DCMAKE_CXX_COMPILER=em++ -DCMAKE_C_COMPILER=emcc -DTBB_STRICT=OFF -DCMAKE_CXX_FLAGS=-Wno-unused-command-line-argument -DTBB_DISABLE_HWLOC_AUTOMATIC_SEARCH=ON -DBUILD_SHARED_LIBS=ON -DTBB_EXAMPLES=ON -DTBB_TEST=ON
```
To compile oneTBB without ``pthreads``, set the flag ``-DEMSCRIPTEN_WITHOUT_PTHREAD=true`` in the command above. By default, oneTBB uses the ``pthreads``.
```
cmake --build . <options>
cmake --install . <options>
```
Where:

* ``emcmake`` - a tool that sets up the environment for Emscripten*. 
* ``-DCMAKE_CXX_COMPILER=em++`` - specifies the C++ compiler as Emscripten* C++ compiler. 
* ``-DCMAKE_C_COMPILER=emcc`` - specifies the C compiler as Emscripten* C compiler.


> **_NOTE:_** See [CMake documentation](https://github.com/oneapi-src/oneTBB/blob/master/cmake/README.md) to learn about other options. 


## Run Test

To run tests, use:

```
ctest
```

# Limitations

You can successfully build your application with oneTBB using WASM, but you may not achieve optimal performance immediately. This is due to the limitation with nested Web Workers: a Web Worker cannot schedule another worker without help from a browser thread. This can lead to unexpected performance outcomes, such as the application running in serial.
Find more information in the [issue](https://github.com/emscripten-core/emscripten/discussions/21963) in the Emscripten repository.
To workaround this issue, try one of the following ways:
1. **Recommended Solution: Use the ``-sPROXY_TO_PTHREAD`` Flag**. 
This flag splits the initial thread into a browser thread and a main thread (proxied by a Web Worker), effectively resolving the issue as the browser thread is always present in the event loop and can participate in Web Workers scheduling. Refer to the [Emscripten documentation](https://emscripten.org/docs/porting/pthreads.html) for more details about ``-sPROXY_TO_PTHREAD`` since using this flag may require refactoring the code.
2. **Alternative Solution: Warm Up the oneTBB Thread Pool**
Initialize the oneTBB thread pool before making the first call to oneTBB. This approach forces the browser thread to participate in Web Workers scheduling.
```cpp
    int num_threads = tbb::this_task_arena::max_concurrency();
    std::atomic<int> barrier{num_threads};
    tbb::parallel_for(0, num_threads, [&barrier] (int) {
        barrier--;
        while (barrier > 0) {
            // Send browser thread to event loop
            std::this_thread::yield();
        }
    }, tbb::static_partitioner{});
```
> **_NOTE:_** Be aware that it might cause delays on the browser side.
