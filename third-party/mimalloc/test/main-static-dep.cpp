// test allocation in a DLL that is statically linked to mimalloc
#include <string>
#include <iostream>
#include "main-static-dep.h"
#include <mimalloc.h>

class Static {
private:
  void* p;
public:
  Static() {
    printf("static-dep: static constructor\n");
    p = mi_malloc(64);
    return;
  }
  ~Static() {
    mi_free(p);
    printf("static-dep: static destructor\n");
    return;
  }
};

static Static s = Static();

void Test(void) {
  char* s = mi_mallocn_tp(char, 128);
  strcpy_s(s, 128, "hello world!");
  printf("message from static dll: %s\n", s);
  mi_free(s);
}


#include <windows.h>

BOOL WINAPI DllMain(HINSTANCE module, DWORD reason, LPVOID reserved) {
  (void)(reserved);
  (void)(module);
  if (reason==DLL_PROCESS_ATTACH) {
    printf("static-dep: dll attach\n");
  }
  else if (reason==DLL_PROCESS_DETACH) {
    mi_option_enable(mi_option_destroy_on_exit);
    printf("static-dep: dll detach\n");
  }
  return TRUE;
}
