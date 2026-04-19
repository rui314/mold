// Issue #981: test overriding allocation in a DLL that is compiled independent of mimalloc. 
// This is imported by the `mimalloc-test-override` project.
#include <string>
#include "main-override-dep.h"

std::string TestAllocInDll::GetString()
{
	char* test = new char[128];
	memset(test, 0, 128);
	const char* t = "test";
	memcpy(test, t, 4);
	std::string r = test;
	delete[] test;
	return r;
}