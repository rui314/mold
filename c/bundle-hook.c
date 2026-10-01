// The hook mold links into an image for the classes of the mergeable
// libraries it merges (-merge_*) or re-exports (-no_merge_*), and into
// a debug build of a mergeable dylib for the classes it doesn't export
// (-add_mergeable_debug_hook), as ld-prime does with a hook of its own.
// Such a class's code is no longer in its framework's binary, but the
// framework's resources still are in the framework's bundle in the
// app. The hook has the Objective-C runtime name the framework's
// binary in the app as the class's image, so that NSBundle's
// bundleForClass: (Swift's Bundle(for:)) finds that bundle.
//
// mold fills in the table of the classes (see src/bundle_hook.rs) and
// embeds the objects build-bundle-hook.sh makes of this file.

#include <dlfcn.h>
#include <limits.h>
#include <mach-o/dyld.h>
#include <objc/runtime.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// A class, the leaf name of its library's install name, and the path
// the hook gives for it, made on first use.
struct entry {
  Class cls;
  const char *name;
  char *path;
};

struct table {
  unsigned long count;
  struct entry entries[];
};

extern struct table __mold_bundle_hook_table;

static char app_bundle[PATH_MAX];
static objc_hook_getImageName next_hook;

// The framework binary of a class's library in the app (as macOS lays
// an app out): <app>/Contents/Frameworks/<name>.framework/<name>. Two
// threads may make the path at once; then one leaks.
static BOOL get_image_name(Class cls, const char **out) {
  struct table *t = &__mold_bundle_hook_table;
  for (unsigned long i = 0; i < t->count; i++) {
    struct entry *e = &t->entries[i];
    if (e->cls != cls)
      continue;
    char *path = e->path;
    if (!path && asprintf(&path, "%s/Contents/Frameworks/%s.framework/%s",
                          app_bundle, e->name, e->name) < 0)
      break;
    e->path = path;
    *out = path;
    return YES;
  }
  return next_hook(cls, out);
}

// Finds the main bundle as NSBundle does, from the executable's path:
// <bundle>/Contents/MacOS/<executable>, else the executable's
// directory. The hook is for an app's bundle only.
static int find_app_bundle(void) {
  char exe[PATH_MAX];
  uint32_t size = sizeof(exe);
  if (_NSGetExecutablePath(exe, &size) || !realpath(exe, app_bundle))
    return 0;
  *strrchr(app_bundle, '/') = '\0';
  size_t len = strlen(app_bundle);
  const char *macos = "/Contents/MacOS";
  if (len >= strlen(macos) && !strcmp(app_bundle + len - strlen(macos), macos))
    app_bundle[len -= strlen(macos)] = '\0';
  return len >= 4 && !strcmp(app_bundle + len - 4, ".app");
}

// Installs the hook as the image is initialized. Swift has a hook of
// its own, which answers for every Swift class and calls no other, and
// installs it on its first lookup of a type by name; the runtime asks
// the hook installed last first. So Swift's goes in first, if Swift is
// loaded. The objects are built for macOS releases that may lack
// objc_setHook_getImageName (10.14 has it), which is looked up too.
// ld-prime's diagnostics about static initializers name its hook's by
// the name this one has.
static void install(void) __asm__("__ZL11constructorv");

__attribute__((constructor)) static void install(void) {
  if (!find_app_bundle())
    return;
  const void *(*lookup_type)(const char *, size_t, const void *,
                             const void *) =
      dlsym(RTLD_DEFAULT, "swift_getTypeByMangledNameInContext");
  if (lookup_type)
    lookup_type("s14PartialKeyPathCyytG", 22, NULL, NULL);
  void (*set_hook)(objc_hook_getImageName, objc_hook_getImageName *) =
      dlsym(RTLD_DEFAULT, "objc_setHook_getImageName");
  if (set_hook)
    set_hook(get_image_name, &next_hook);
}
