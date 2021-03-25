#define _GNU_SOURCE 1

#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

extern char **environ;

static char *get_mold_path() {
  char *path = getenv("REAL_MOLD_PATH");
  if (!path) {
    fprintf(stderr, "REAL_MOLD_PATH is not set\n");
    exit(1);
  }
  return path;
}

int execve(const char *path, char * const *argv, char * const *envp) {
  if (getenv("MOLD_WRAPPER_DEBUG")) {
    fprintf(stderr, "mold: execve %s\n", path);
    fflush(stderr);
  }

  if (!strcmp(path, "/usr/bin/ld"))
    path = get_mold_path();

  typedef int T(const char *, char *const *, char *const *);
  T *real = dlsym(RTLD_NEXT, "execve");
  return real(path, argv, envp);
}

#if 0
int execl(const char *path, const char *arg, ...
          /* (char  *) NULL */) {
  fprintf(stderr, "mold: execl\n");
  fflush(stderr);
}

int execlp(const char *file, const char *arg, ...
           /* (char  *) NULL */) {
  fprintf(stderr, "mold: execlp\n");
  fflush(stderr);
}

int execle(const char *path, const char *arg, ...
           /*, (char *) NULL, char *const *envp */) {
  fprintf(stderr, "mold: execle\n");
  fflush(stderr);
}
#endif

int execv(const char *path, char *const *argv) {
  return execve(path, argv, environ);
}

int execvp(const char *file, char *const *argv) {
  if (getenv("MOLD_WRAPPER_DEBUG")) {
    fprintf(stderr, "mold: execvp %s\n", file);
    fflush(stderr);
  }

  if (!strcmp(file, "ld") || !strcmp(file, "/usr/bin/ld"))
    file = get_mold_path();

  typedef int T(const char *, char *const *);
  T *real = dlsym(RTLD_NEXT, "execvp");
  return real(file, argv);
}

int execvpe(const char *file, char *const *argv, char *const *envp) {
  if (getenv("MOLD_WRAPPER_DEBUG")) {
    fprintf(stderr, "mold: execvpe %s\n", file);
    fflush(stderr);
  }

  if (!strcmp(file, "ld") || !strcmp(file, "/usr/bin/ld"))
    file = get_mold_path();

  typedef int T(const char *, char *const *, char *const *);
  T *real = dlsym(RTLD_NEXT, "execvpe");
  return real(file, argv, environ);
}
