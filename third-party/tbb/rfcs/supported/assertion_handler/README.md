# Custom Assertion Handler

## Introduction

In 2021, with the transition from TBB 2020 to the first release of oneTBB, the custom assertion handler feature
was removed and has since been requested several times (e.g., https://github.com/uxlfoundation/oneTBB/issues/1258).

OneTBB currently provides assertion functionality for debugging and error reporting purposes through the
`assertion_failure` function. However, applications using oneTBB may have their own error handling, logging, or
reporting requirements that differ from the default assertion behavior (which prints to stderr and calls
`std::abort`).

The motivation for this proposal includes:

- **Improved integration with application error handling**: Applications often have sophisticated error handling,
  logging frameworks, or crash reporting systems that they want to integrate with oneTBB assertions.
- **Custom recovery mechanisms**: Some applications may want to attempt recovery or cleanup operations before
  terminating, rather than immediately calling `std::abort`.
- **Enhanced debugging capabilities**: Development environments may want to integrate oneTBB assertions with their
  debugging tools, IDE breakpoints, or custom diagnostic systems.
- **Production environment requirements**: Production systems may need to log assertion failures to specific
  locations, send alerts, or perform graceful shutdowns rather than immediate termination.

Currently, when an assertion fails in oneTBB, users have no control over the behavior - the library always prints
to stderr and calls `std::abort`. This rigid behavior can be problematic for applications with specific
requirements for error handling and diagnostics.

## Proposal

We propose adding a custom assertion handler mechanism that allows applications to set their own assertion
handling functions. This proposal is modeled on the assertion handler API that existed in TBB 2020 and is semantically
similar to the standard library's `std::set_terminate` and `std::get_terminate` functions.

The design parallels both:
- **Standard Library Pattern**: `std::set_terminate` allows setting a custom termination handler and returns the
  previous handler, with thread-safe atomic operations for handler storage
- **TBB 2020 Compatibility**: The same `assertion_handler_type` and `set_assertion_handler` function signatures
  that existed in TBB 2020, enabling seamless migration for applications that previously used this functionality

This approach provides both familiarity for developers accustomed to standard library patterns and direct
compatibility for applications migrating from TBB 2020 to oneTBB.

### New Public API

The proposal adds the following new functions to the public API, using the same signatures as TBB 2020 for
compatibility:

#### Header

```cpp
#include <oneapi/tbb/global_control.h>
```

#### Syntax

```cpp
namespace oneapi {
namespace tbb {
namespace ext {
#if !__TBB_DISABLE_SPEC_EXTENSIONS
    //! Type alias for assertion handler function pointer - same as TBB 2020.
    //! The handler should not return. If it eventually returns, the behavior is runtime-undefined.
    using assertion_handler_type = void(*)(const char* location, int line,
                                           const char* expression, const char* comment);

    //! Set assertion handler and return its previous value.
    //! If new_handler is nullptr, resets to the default handler.
    //! Uses the same signature as TBB 2020 for migration compatibility.
    assertion_handler_type set_assertion_handler(assertion_handler_type new_handler) noexcept;

    //! Return the current assertion handler.
    //! New function not present in TBB 2020, following std::get_terminate pattern.
    assertion_handler_type get_assertion_handler() noexcept;
#endif
}}}
```

Applications that used the custom assertion handler in TBB 2020 can migrate to this proposal with minimal changes
by adding names to `namespace tbb`:
```cpp
namespace tbb {
    using oneapi::tbb::ext::set_assertion_handler;
    using oneapi::tbb::ext::assertion_handler_type;
}
```

#### Specification Extension

This API is introduced as an extension to the oneTBB specification, controlled by the
`__TBB_DISABLE_SPEC_EXTENSIONS` macro. By default (macro undefined or defined as 0), the extension will
be enabled: `set_assertion_handler` and `get_assertion_handler` will be declared and exported, and
`assertion_failure` will dispatch to the active handler. Defining `__TBB_DISABLE_SPEC_EXTENSIONS` to a non-zero
value before including oneTBB headers will disable the extension: these declarations will be excluded from
the public API, and the library will always use the default assertion behavior.

The availability of the extension can be checked with the `TBB_EXT_CUSTOM_ASSERTION_HANDLER` macro
after including either the `oneapi/tbb/global_control.h` or the `oneapi/tbb/version.h` header.
The value of the macro should be increased when observable modifications are made to the feature.

### Proposed Implementation Strategy

#### Core Implementation Approach

The implementation will follow the same patterns used by `std::set_terminate` and `std::get_terminate`, while
maintaining compatibility with TBB 2020:

1. **Handler Storage**: Add a global static atomic variable to store the current assertion handler function pointer,
   ensuring thread-safe access across the library, similar to how the standard library stores the terminate handler.

2. **Handler Invocation**: Modify `assertion_failure` to call the currently set handler instead of directly
   executing the default logic, mirroring how `std::terminate` calls the registered handler.

3. **TBB 2020 Compatibility**: Use identical function signatures to ensure existing TBB 2020 code works without
   modification.

#### Thread Safety Design

Following the `std::set_terminate`/`std::get_terminate` model, the implementation will use atomic operations with
appropriate memory ordering (`memory_order_acq_rel` for exchanges, `memory_order_acquire` for loads), consistent with
standard library implementations. This approach will ensure multiple threads can safely call `set_assertion_handler`
and `get_assertion_handler` concurrently while guaranteeing that handler changes are visible across all threads
without requiring additional synchronization.

The assertion handler is invoked on each assertion failure, even if it happens concurrently in several threads.
The default handler uses `atomic_do_once` to ensure that assertions are reported exactly once per failure. For custom
assertion handlers, it is the user's responsibility to ensure that their handler maintains appropriate thread safety
if multiple threads might trigger the same assertion simultaneously. This behavior is consistent with TBB 2020, where
custom handlers were also responsible for their own thread safety.

The documentation should be extended to provide guidance and examples for implementing thread-safe custom handlers,
given that this responsibility shifts to the user. It should also include migration notes for TBB 2020 users,
confirming that existing `set_assertion_handler` calls work unchanged, with identical function signatures and
behavior.

#### Backward Compatibility Preservation

The default assertion handling behavior will remain unchanged, so existing applications will continue to work exactly
as before. There will be no changes to existing assertion macros or calling conventions. Applications using custom
assertion handlers in TBB 2020 will be able to use the same code with little to no modification, ensuring seamless
migration to oneTBB.

#### Null Handler Protection

Following both the `std::set_terminate` pattern and TBB 2020 behavior, the implementation will provide protection
against null handler pointers. When `set_assertion_handler(nullptr)` is called, the function will reset to the default
handler rather than allowing null dereference. This approach provides a safe way to restore default behavior, similar
to how `std::set_terminate` handles null pointers in the standard library.

#### TBBBind Integration

To ensure consistent assertion handling across oneTBB components, the TBB shared library will provide the TBBBind
shared library with a pointer to the assertion function (which internally calls the active handler). This approach
provides unified assertion handling where assertions originating from TBBBind will automatically use the custom
assertion handler set by the application, creating a consistent user experience.

This "soft" runtime dependency addition is acceptable, since TBBBind is exclusively used from within the oneTBB shared
library. In the unexpected case TBB does not provide the function pointer, the default assertion handler will continue
to be used.

#### No impact on TBBMalloc

TBBMalloc, on the other hand, will not be changed. It will continue to reuse the assertion handling
implementation from the core oneTBB library source code yet maintain its own assertion behavior that does not
access the oneTBB custom assertion handler. This design preserves TBBMalloc's portability, as it can be used entirely
separately from oneTBB and cannot depend on the oneTBB shared library.

### Platform Considerations

The implementation will add new `set_assertion_handler` and `get_assertion_handler` public entry points to the oneTBB
shared library across all supported platforms. We will use function pointers instead of `std::function` to avoid
additional C++ runtime dependencies. Even though `std::function` provides type safety and supports lambda captures,
it also introduces additional complexity and potential performance overhead. Using function pointers maintains ABI
stability across different C++ standards and compilers and is consistent with both the standard library approach and
the TBB 2020 implementation.

### Performance Impact Analysis

The proposed implementation will have minimal performance impact, following the `std::set_terminate` model:

- **Normal Execution**: No overhead on normal execution paths
- **Assertion Failure Path**: One additional atomic load
- **Memory Overhead**: One atomic pointer per process
- **Binary Size**: Minimal increase due to additional exported functions

### Usage Examples

#### Adding Custom Steps

```cpp
auto default_handler = tbb::get_assertion_handler();

void wrapper_assertion_handler(const char* location, int line,
                               const char* expression, const char* comment) {
    // Perform a custom step before calling default handler
    // ... (e.g., log to a file, notify system, custom cleanup, etc.)
    default_handler(location, line, expression, comment);
}

int main() {
    // Set custom handler (identical to TBB 2020 usage)
    tbb::set_assertion_handler(wrapper_assertion_handler);

    // Use oneTBB normally - any assertion failures will use custom handler
    // ...

    // Restore previous handler if needed
    tbb::set_assertion_handler(default_handler);
}
```

#### Integration with Logging Framework

```cpp
void logging_assertion_handler(const char* location, int line,
                               const char* expression, const char* comment) {
    spdlog::critical("TBB Assertion Failed: {} at {}:{} - {}",
                     expression, location, line, comment ? comment : "");

    // Flush logs before terminating
    spdlog::shutdown();
    std::abort();
}
```

#### Development/Testing Handler

```cpp
void test_assertion_handler(const char* location, int line,
                            const char* expression, const char* comment) {
    // In testing environments, throw exception instead of aborting
    // to allow test frameworks to catch and handle assertion failures
    std::string msg = std::string("TBB Assertion: ") + expression +
                      " at " + location + ":" + std::to_string(line);
    if (comment) {
        msg += " - " + std::string(comment);
    }
    throw std::runtime_error(msg);
}
```
