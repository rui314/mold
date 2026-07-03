// Issue #981: test overriding allocation in a DLL that is compiled independent of mimalloc. 
// This is imported by the `mimalloc-test-override` project.
#include <string>
#include <iostream>
#include "main-override-dep.h"

std::string TestAllocInDll::GetString()
{
	char* test = new char[128];
	memset(test, 0, 128);
	const char* t = "test";
	memcpy(test, t, 4);
	std::string r = test;
  std::cout << "override-dep: GetString: " << r << "\n";
	delete[] test;
	return r;
}


class Static {
private:
  void* p;
public:
  Static() {
    printf("override-dep: static constructor\n");
    p = malloc(64);
    return;
  }
  ~Static() {
    free(p);
    printf("override-dep: static destructor\n");
    return;
  }
};

static Static s = Static();


#include <windows.h>

BOOL WINAPI DllMain(HINSTANCE module, DWORD reason, LPVOID reserved) {
  (void)(reserved);
  (void)(module);
  if (reason==DLL_PROCESS_ATTACH) {
    printf("override-dep: dll attach\n");
  }
  else if (reason==DLL_PROCESS_DETACH) {
    printf("override-dep: dll detach\n");
  }  
  return TRUE;
}
