#pragma once
// Issue #981: test overriding allocation in a DLL that is compiled independent of mimalloc. 
// This is imported by the `mimalloc-test-override` project.

#include <string>

class TestAllocInDll
{
public:
	__declspec(dllexport) std::string GetString();
	__declspec(dllexport) void TestHeapAlloc();
};
