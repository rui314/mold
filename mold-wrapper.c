#define _GNU_SOURCE 1

#include <dlfcn.h>
#include <spawn.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

extern char **environ;

static char *get_mold_path() {
  char *path = getenv("MOLD_PATH");
  if (path)
    return path;
  fprintf(stderr, "MOLD_PATH is not set\n");
  exit(1);
}

static void debug_print(char *fmt, ...) {
  if (!getenv("MOLD_WRAPPER_DEBUG"))
    return;

  va_list ap;
  va_start(ap, fmt);
  fprintf(stderr, "mold-wrapper.so: ");
  vfprintf(stderr, fmt, ap);
  fflush(stderr);
  va_end(ap);
}

static va_list get_args(va_list ap, int argc, char **argv) {
  for (int i = 1; i < argc - 1; i++) {
    char *arg = va_arg(ap, char *);
    if (!arg)
      break;
    argv[i] = arg;
  }
  return ap;
}

static bool is_ld(const char *path) {
  const char *ptr = path + strlen(path);
  while (path < ptr && ptr[-1] != '/')
    ptr--;

  return !strcmp(ptr, "ld") || !strcmp(ptr, "ld.lld") ||
         !strcmp(ptr, "ld.gold");
}

int execve(const char *path, char *const *argv, char *const *envp) {
  debug_print("execve %s\n", path);

  if (is_ld(path)) {
    path = get_mold_path();
    ((const char **)argv)[0] = path;
  }

  typeof(execve) *real = dlsym(RTLD_NEXT, "execve");
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
  ap = get_args(ap, 4096, argv);
  char **env = va_arg(ap, char **);
  return execve(path, argv, env);
}

int execv(const char *path, char *const *argv) {
  return execve(path, argv, environ);
}

int execvp(const char *file, char *const *argv) {
  return execvpe(file, argv, environ);
}

int execvpe(const char *file, char *const *argv, char *const *envp) {
  debug_print("execvpe %s\n", file);

  if (!strcmp(file, "ld") || is_ld(file)) {
    file = get_mold_path();
    ((const char **)argv)[0] = file;
  }

  typeof(execvpe) *real = dlsym(RTLD_NEXT, "execvpe");
  return real(file, argv, environ);
}

int posix_spawn(pid_t *pid, const char *path,
                const posix_spawn_file_actions_t *file_actions,
                const posix_spawnattr_t *attrp,
                char *const *argv, char *const *envp) {
  debug_print("posix_spawn %s\n", path);

  if (is_ld(path)) {
    path = get_mold_path();
    ((const char **)argv)[0] = path;
  }

  typeof(posix_spawn) *real = dlsym(RTLD_NEXT, "posix_spawn");
  return real(pid, path, file_actions, attrp, argv, envp);
}
