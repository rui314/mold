#define _GNU_SOURCE 1

#include <dlfcn.h>
#include <spawn.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

extern char **environ;

static char *get_mold_path() {
  char *path = getenv("REAL_MOLD_PATH");
  if (!path) {
    fprintf(stderr, "REAL_MOLD_PATH is not set\n");
    exit(1);
  }
  return path;
}

static void get_args(va_list ap, int argc, char **argv) {
  for (int i = 1; i < argc - 1; i++) {
    char *arg = va_arg(ap, char *);
    if (!arg)
      break;
    argv[i] = arg;
  }
}

int execve(const char *path, char *const *argv, char *const *envp) {
  if (getenv("MOLD_WRAPPER_DEBUG")) {
    fprintf(stderr, "mold-wrapper: execve %s\n", path);
    fflush(stderr);
  }

  if (!strcmp(path, "/usr/bin/ld")) {
    path = get_mold_path();
    ((const char **)argv)[0] = path;
  }

  typedef int T(const char *, char *const *, char *const *);
  T *real = dlsym(RTLD_NEXT, "execve");
  return real(path, argv, envp);
}

int execl(const char *path, const char *arg0, ...) {
  va_list ap;
  va_start(ap, arg0);
  char *argv[4096] = {(char *)arg0};
  get_args(ap, 4096, argv);
  return execve(path, argv, environ);
}

int execlp(const char *file, const char *arg0, ...) {
  va_list ap;
  va_start(ap, arg0);
  char *argv[4096] = {(char *)arg0};
  get_args(ap, 4096, argv);
  return execvpe(file, argv, environ);
}

int execle(const char *path, const char *arg0, ...) {
  va_list ap;
  va_start(ap, arg0);
  char *argv[4096] = {(char *)arg0};
  get_args(ap, 4096, argv);
  char **env = va_arg(ap, char **);
  execve(path, argv, env);
}

int execv(const char *path, char *const *argv) {
  return execve(path, argv, environ);
}

int execvp(const char *file, char *const *argv) {
  return execvpe(file, argv, environ);
}

int execvpe(const char *file, char *const *argv, char *const *envp) {
  if (getenv("MOLD_WRAPPER_DEBUG")) {
    fprintf(stderr, "mold: execvpe %s\n", file);
    fflush(stderr);
  }

  if (!strcmp(file, "ld") || !strcmp(file, "/usr/bin/ld")) {
    file = get_mold_path();
    ((const char **)argv)[0] = file;
  }

  typedef int T(const char *, char *const *, char *const *);
  T *real = dlsym(RTLD_NEXT, "execvpe");
  return real(file, argv, environ);
}

int posix_spawn(pid_t *pid, const char *path,
                const posix_spawn_file_actions_t *file_actions,
                const posix_spawnattr_t *attrp,
                char *const *argv, char *const *envp) {
  if (getenv("MOLD_WRAPPER_DEBUG")) {
    fprintf(stderr, "mold: posix_spawn %s\n", path);
    fflush(stderr);
  }

  if (!strcmp(path, "/usr/bin/ld")) {
    path = get_mold_path();
    ((const char **)argv)[0] = path;
  }

  typedef int T(pid_t *, const char *,
                const posix_spawn_file_actions_t *,
                const posix_spawnattr_t *,
                char *const *, char *const *);

  T *real = dlsym(RTLD_NEXT, "posix_spawn");
  return real(pid, path, file_actions, attrp, argv, envp);
}
