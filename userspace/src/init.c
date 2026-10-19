#include "syscall.h"

#define INPUT_MAX 128
#define MAX_ARGS 8
#define PATH_MAX 256

static unsigned long strlen(const char *s) {
  unsigned long len = 0;
  while (s[len]) {
    len++;
  }
  return len;
}

static void print(const char *s) { sys_write(1, s, strlen(s)); }

static void print_char(char c) { sys_write(1, &c, 1); }

static int strcmp(const char *a, const char *b) {
  while (*a && (*a == *b)) {
    a++;
    b++;
  }
  return *(const unsigned char *)a - *(const unsigned char *)b;
}

__attribute__((unused)) static int strncmp(const char *a, const char *b,
                                           unsigned long n) {
  for (unsigned long i = 0; i < n; i++) {
    if (a[i] != b[i]) {
      return (unsigned char)a[i] - (unsigned char)b[i];
    }
    if (a[i] == '\0') {
      return 0;
    }
  }
  return 0;
}

static void print_num(long n) {
  char buf[32];
  int i = 0;
  if (n < 0) {
    buf[i++] = '-';
    n = -n;
  }
  if (n == 0) {
    buf[i++] = '0';
    sys_write(1, buf, i);
    return;
  }
  char digits[32];
  int d = 0;
  while (n > 0) {
    digits[d++] = '0' + (n % 10);
    n /= 10;
  }
  while (d > 0) {
    buf[i++] = digits[--d];
  }
  sys_write(1, buf, i);
}

static char input[INPUT_MAX];
static int input_len;

static void input_reset(void) {
  input_len = 0;
  input[0] = '\0';
}

static void input_back(void) {
  if (input_len == 0) {
    return;
  }
  input_len--;
  input[input_len] = '\0';
  print("\b \b");
}

static char cwd[PATH_MAX] = "/";

static void path_join(const char *arg, char *out) {
  int at = 0;

  if (arg[0] == '/') {
    out[at++] = '/';
  } else {
    for (int i = 0; cwd[i] && at < PATH_MAX - 2; i++) {
      out[at++] = cwd[i];
    }

    if (at > 1 && out[at - 1] != '/') {
      out[at++] = '/';
    }
  }

  for (int i = 0; arg[i] && at < PATH_MAX - 1; i++) {
    out[at++] = arg[i];
  }

  out[at] = '\0';
}

static void path_normalise(char *path) {
  char parts[16][32];
  int lengths[16];
  int count = 0;

  int i = 0;
  int len = (int)strlen(path);

  while (i < len) {
    while (i < len && path[i] == '/') {
      i++;
    }

    int start = i;

    while (i < len && path[i] != '/') {
      i++;
    }

    int size = i - start;

    if (size == 0 || size >= 32) {
      continue;
    }

    if (size == 1 && path[start] == '.') {
      continue;
    }

    if (size == 2 && path[start] == '.' && path[start + 1] == '.') {
      if (count > 0) {
        count--;
      }
      continue;
    }

    for (int j = 0; j < size; j++) {
      parts[count][j] = path[start + j];
    }

    lengths[count] = size;
    count++;

    if (count == 16) {
      break;
    }
  }

  int at = 0;

  path[at++] = '/';

  for (int p = 0; p < count; p++) {
    if (p > 0) {
      path[at++] = '/';
    }

    if (at + lengths[p] >= PATH_MAX) {
      break;
    }

    for (int j = 0; j < lengths[p]; j++) {
      path[at++] = parts[p][j];
    }
  }

  path[at] = '\0';
}

static void resolve_path(const char *arg, char *out) {
  path_join(arg, out);
  path_normalise(out);
}

static void input_put(char c) {
  if (input_len >= INPUT_MAX - 1) {
    return;
  }
  input[input_len++] = c;
  input[input_len] = '\0';
  print_char(c);
}

static void input_read_line(void) {
  input_reset();

  while (1) {
    char c = 0;
    long n = sys_read(0, &c, 1);

    if (n <= 0) {
      sys_yield();
      continue;
    }

    if (c == 0x03) {
      print("^C\n");
      input_reset();
      return;
    }

    if (c == '\r' || c == '\n') {
      print("\n");
      return;
    }

    if (c == '\b' || c == 0x7f) {
      input_back();
      continue;
    }

    if (c >= 32 && c <= 126) {
      input_put(c);
    }
  }
}

static int split_args(char *argv[]) {
  int argc = 0;
  int i = 0;

  argv[argc++] = &input[0];

  while (input[i] != '\0' && argc < MAX_ARGS) {
    if (input[i] == ' ') {
      input[i] = '\0';
      argv[argc++] = &input[i + 1];
    }
    i++;
  }
  argv[argc] = 0;
  return argc;
}

static void cmd_cd(const char *arg) {
  char full[PATH_MAX];

  resolve_path(arg, full);

  long fd = sys_open_dir(full);

  if (fd < 0) {
    print("cd: no such directory: ");
    print(arg);
    print("\n");
    return;
  }

  sys_close(fd);

  for (int i = 0; i < PATH_MAX - 1; i++) {
    cwd[i] = full[i];

    if (full[i] == '\0') {
      break;
    }
  }

  cwd[PATH_MAX - 1] = '\0';
}

static void cmd_pwd(void) {
  print(cwd);
  print("\n");
}

static void cmd_mkdir(const char *arg) {
  char full[PATH_MAX];

  resolve_path(arg, full);

  if (sys_mkdir(full) < 0) {
    print("mkdir: cannot create ");
    print(arg);
    print("\n");
    return;
  }

  print("created ");
  print(full);
  print("\n");
}

static void cmd_touch(const char *arg) {
  char full[PATH_MAX];

  resolve_path(arg, full);

  if (sys_create(full) < 0) {
    print("touch: cannot create ");
    print(arg);
    print("\n");
  }
}

static void cmd_rm(const char *arg) {
  char full[PATH_MAX];

  resolve_path(arg, full);

  if (sys_unlink(full) < 0) {
    print("rm: cannot remove ");
    print(arg);
    print("\n");
  }
}

static void cmd_rmdir(const char *arg) {
  char full[PATH_MAX];

  resolve_path(arg, full);

  if (sys_rmdir(full) < 0) {
    print("rmdir: cannot remove ");
    print(arg);
    print("\n");
  }
}

static void cmd_help(void) {
  print("Commands:\n");
  print("  help          show this list\n");
  print("  echo <text>   print text\n");
  print("  pid           print the process id\n");
  print("  ls [dir]      list a directory\n");
  print("  cd <dir>      change directory\n");
  print("  pwd           print the current directory\n");
  print("  mkdir <dir>   create a directory\n");
  print("  rmdir <dir>   remove an empty directory\n");
  print("  touch <file>  create an empty file\n");
  print("  rm <file>     remove a file\n");
  print("  cat <file>    print a file\n");
  print("  read <file>   read a file and report the size\n");
  print("  run <file>    execute a program\n");
  print("  clear         clear the screen\n");
  print("  yield         give up the rest of the time slice\n");
  print("  exit          leave the shell\n");
}

static void cmd_echo(char **argv, int argc) {
  for (int i = 1; i < argc; i++) {
    if (i > 1) {
      print_char(' ');
    }
    print(argv[i]);
  }
  print("\n");
}

static void cmd_pid(void) {
  print("pid ");
  print_num(sys_getpid());
  print("\n");
}

#define DIRENT_HEADER 19
#define DT_DIR 4

static void print_name(const char *name, int len) {
  int actual_len = 0;
  while (actual_len < len && name[actual_len]) actual_len++;
  if (actual_len > 0) {
    sys_write(1, name, actual_len);
  }
}

static void cmd_ls(const char *path) {
  char full[PATH_MAX];

  resolve_path(path, full);

  long fd = sys_open_dir(full);

  if (fd < 0) {
    print("ls: cannot open ");
    print(path);
    print("\n");
    return;
  }

  char buf[1024];
  long got;

  while ((got = sys_getdents(fd, buf, sizeof(buf))) > 0) {
    long at = 0;

    while (at + DIRENT_HEADER <= got) {
      unsigned char type = (unsigned char)buf[at + 18];
      int reclen =
          (unsigned char)buf[at + 16] | ((unsigned char)buf[at + 17] << 8);

      if (reclen <= DIRENT_HEADER || at + reclen > got) {
        break;
      }

      print_name(&buf[at + DIRENT_HEADER], reclen - DIRENT_HEADER - 1);

      if (type == DT_DIR) {
        print("/");
      }

      print("\n");

      at += reclen;
    }
  }

  if (got < 0) {
    print("ls: readdir failed\n");
  }

  sys_close(fd);
}

static void cmd_cat(const char *path) {
  char full[PATH_MAX];

  resolve_path(path, full);

  long fd = sys_open(full, 0);

  if (fd < 0) {
    print("cat: cannot open ");
    print(path);
    print("\n");
    return;
  }

  char buf[256];
  long bytes;

  while ((bytes = sys_read(fd, buf, sizeof(buf) - 1)) > 0) {
    buf[bytes] = '\0';
    print(buf);
  }

  print("\n");
  sys_close(fd);
}

static void cmd_read(const char *path) {
  char full[PATH_MAX];

  resolve_path(path, full);

  long fd = sys_open(full, 0);

  if (fd < 0) {
    print("read: cannot open ");
    print(path);
    print("\n");
    return;
  }

  char buf[256];
  long total = 0;
  long bytes;

  while ((bytes = sys_read(fd, buf, sizeof(buf))) > 0) {
    total += bytes;
  }

  print(path);
  print(": ");
  print_num(total);
  print(" bytes\n");

  sys_close(fd);
}

static void cmd_run(int argc, char **argv) {
  char full[PATH_MAX];
  char *child_argv[16];
  char *envp[1] = {0};

  resolve_path(argv[1], full);

  child_argv[0] = full;

  int i = 2;
  for (; i < argc && i < 15; i++) {
    child_argv[i - 1] = argv[i];
  }
  child_argv[i - 1] = 0;

  long pid = sys_fork();
  if (pid < 0) {
    print("run: fork failed\n");
    return;
  }
#ifndef TIOCSPGRP
#define TIOCSPGRP 0x5410
#endif

  if (pid == 0) {
    int child_pid = (int)sys_getpid();
    sys_ioctl(0, TIOCSPGRP, &child_pid);
    long ret = sys_execve(full, child_argv, envp);
    print("run: execve failed: ");
    print_num(ret);
    print("\n");
    sys_exit(1);
  }
  int p = (int)pid;
  sys_ioctl(0, TIOCSPGRP, &p);
  int status = 0;
  sys_wait4(pid, &status, 0, 0);
  int shell_pid = (int)sys_getpid();
  sys_ioctl(0, TIOCSPGRP, &shell_pid);

  int exit_code = (status >> 8) & 0xff;
  if (exit_code != 0) {
    print("run: child exited with code ");
    print_num(exit_code);
    print("\n");
  }
}

static void cmd_clear(void) {
  print("\033[2J\033[H");
}

static void dispatch(char **argv, int argc) {
  const char *cmd = argv[0];

  if (strcmp(cmd, "help") == 0) {
    cmd_help();
  } else if (strcmp(cmd, "echo") == 0) {
    cmd_echo(argv, argc);
  } else if (strcmp(cmd, "pid") == 0) {
    cmd_pid();
  } else if (strcmp(cmd, "clear") == 0) {
    cmd_clear();
  } else if (strcmp(cmd, "yield") == 0) {
    print("yielding\n");
    sys_yield();
  } else if (strcmp(cmd, "exit") == 0) {
    print("bye\n");
    sys_exit(0);
  } else if (strcmp(cmd, "cd") == 0) {
    cmd_cd(argc < 2 ? "/" : argv[1]);
  } else if (strcmp(cmd, "pwd") == 0) {
    cmd_pwd();
  } else if (strcmp(cmd, "mkdir") == 0) {
    if (argc < 2) {
      print("mkdir: needs a name\n");
    } else {
      cmd_mkdir(argv[1]);
    }
  } else if (strcmp(cmd, "touch") == 0) {
    if (argc < 2) {
      print("touch: needs a name\n");
    } else {
      cmd_touch(argv[1]);
    }
  } else if (strcmp(cmd, "rm") == 0) {
    if (argc < 2) {
      print("rm: needs a name\n");
    } else {
      cmd_rm(argv[1]);
    }
  } else if (strcmp(cmd, "rmdir") == 0) {
    if (argc < 2) {
      print("rmdir: needs a name\n");
    } else {
      cmd_rmdir(argv[1]);
    }
  } else if (strcmp(cmd, "ls") == 0) {
    cmd_ls(argc < 2 ? "." : argv[1]);
  } else if (strcmp(cmd, "cat") == 0) {
    if (argc < 2) {
      print("cat: needs a file\n");
    } else {
      cmd_cat(argv[1]);
    }
  } else if (strcmp(cmd, "read") == 0) {
    if (argc < 2) {
      print("read: needs a file\n");
    } else {
      cmd_read(argv[1]);
    }
  } else if (strcmp(cmd, "run") == 0) {
    if (argc < 2) {
      print("run: needs a program\n");
    } else {
      cmd_run(argc, argv);
    }
  } else {
    print("unknown command: ");
    print(cmd);
    print("\n");
  }
}

int main(void) {
  print("\n=========================================\n");
  print("     Welcome to BeanieOS User Shell!     \n");
  print("=========================================\n");
  print("Ring 3, loaded from init.c in userspace\n");
  print("proper shell loaded as a init proc with multi threading and forking "
        "via execve\n");
  print("do run hi to test elf loading and execution\n");
  print("You can play Doom with: run doom\n");
  print("KEYBOARD controls only: WASD/Arrows to move, Ctrl/F to fire, Space to use, Esc for menu\n");
  print("Type 'help' for the command list.\n\n");

  char *argv[MAX_ARGS];

  while (1) {
    print(cwd);
    print("$ ");
    input_read_line();

    int argc = split_args(argv);

    if (argc == 0 || argv[0][0] == '\0') {
      continue;
    }

    dispatch(argv, argc);
  }

  return 0;
}
