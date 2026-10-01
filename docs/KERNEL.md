# Trinux: the Setun-386 kernel

**Status: DRAFT v0.1, under design review. Nothing is implemented yet.**
Items marked **⚠ REVIEW** are open choices. §13 lists them.

Trinux is a Unix kernel for Setun-386, **modelled on Linux** and providing
**POSIX.1-2017 interfaces**. It is written in Trine and boots through the BIOS
([`BOOT.md`](BOOT.md)).

## 0. What "POSIX" can mean on a ternary machine

POSIX.1-2001 and later require 8-bit bytes (`CHAR_BIT == 8`) and a C compiler
(`c99`). Setun-386 has 9-trit trytes, and its language is Trine. So Trinux
cannot be *certified*. It aims at the next best thing:

- **Interfaces:** the POSIX system interfaces with POSIX semantics, adapted so that **the tryte plays the role of the byte**.
  - `read(fd, buf, n)` transfers `n` trytes.
  - `off_t` and `size_t` count trytes.
  - Text is one Unicode code point per tryte, not UTF-8.
- **Behaviour:** the same process, file, directory, pipe, signal and terminal behaviour a POSIX program expects. Error numbers, signal numbers, flags and system-call numbers come from Linux.
- **Tools:** a POSIX shell (`sh`, a subset) and the core utilities, as Trine programs.

Where Trinux departs from POSIX, the departure is listed in §12.

## 1. Architecture

Like Linux, Trinux is a **monolithic kernel**: the scheduler, memory manager,
VFS, file systems and drivers all run together in kernel mode.

| Ring | Linux equivalent | Runs |
|---|---|---|
| **+1** kernel | kernel mode | the whole kernel, including drivers |
| 0 drivers | – | unused in v1 (reserved for sandboxed drivers later) ⚠ REVIEW |
| **−1** user | user mode | every process |

Source layout (in Trine, in the repository's `kernel/` directory):

```
kernel/
  boot/      loader (boot sector), early entry
  arch/      traps, interrupts, context switch, page tables
  mm/        frame allocator, kmalloc, address spaces, page faults, COW
  kernel/    tasks, scheduler, fork/exec/exit/wait, signals, time, syscalls
  fs/        VFS (superblock, inode, dentry, file), ramfs, devfs, procfs, tfs
  drivers/   tty (console + serial), keyboard, disk, framebuffer, rtc, timer
  lib/       string, printk, lists
```

## 2. Booting

1. **The BIOS** loads sector 0 (the **loader**) into frame 1 and jumps to it ([`BOOT.md`](BOOT.md)).
2. **The loader** reads the kernel image from sectors 1…K, as named in a header, into physical frames 4 and up, then jumps to the kernel entry.
3. **`start_kernel()`**, as in Linux:
   1. `setup_arch()`: set the interrupt table, build the kernel page tables, turn on paging;
   2. `mm_init()`: the frame allocator and `kmalloc`;
   3. `time_init()`: timer at 100 Hz and the RTC;
   4. console and tty;
   5. VFS, then mount the root file system (the initramfs, then `tfs` from disk);
   6. start **`/sbin/init`** as PID 1.

**Disk image layout** (built by a host tool, `mkdisk`):

| Sectors | Contents |
|---|---|
| 0 | the loader (boot signature at the end) |
| 1 … K | the kernel image |
| K+1 … | the root file system (`tfs`) |

## 3. Memory

### 3.1 Address space: user space is the negative half ⚠ REVIEW

Balanced ternary gives virtual addresses a natural split by sign. The L1 page-table index is the top 9 trits.

| L1 index | Virtual addresses | Used for |
|---|---|---|
| **−9841 … −1** | negative, below −1.9·10^8 | **user space**, private to each process |
| **0 … +1** | about −1.9·10^8 … +5.8·10^8 | **the kernel**: identity-mapped physical memory, all of RAM, ROM and the framebuffer |
| +2 … +9841 | above | reserved (future kernel virtual space: `vmalloc`, module space) |

- **The kernel half** is the same in every process. Each process's L1 table shares the same two kernel entries, marked `G` (global) and ring +1, as Linux shares its kernel mappings.
- **The kernel runs identity-mapped** (virtual = physical). Trine code uses absolute addresses for globals, so it can run before and after paging is turned on.
- **Null pointers:** address 0 lies in the kernel half, so a user program that follows a null pointer takes a protection fault and gets `SIGSEGV`.

**Process memory map**, in the spirit of Linux's:

| Region | Virtual address | Grows |
|---|---|---|
| text, data, bss | from `USER_BASE` = −3^25 (−847 288 609 443) | – |
| heap (`brk`) | after the bss | upward |
| mmap area | middle of user space | downward |
| stack | top at −3^18 − 3 (just below the kernel half) | downward |
| arguments and environment | top of the stack | – |

### 3.2 Physical memory

- **Frame allocator:** a buddy allocator over 3^9-tryte frames, the analogue of Linux's 4 KiB pages. Blocks are grouped by powers of **3** (a "triddy" allocator) rather than 2: each block of 3^k frames splits into three. ⚠ REVIEW
- **`kmalloc`:** a slab allocator for kernel objects (tasks, inodes, files).
- **Page cache** for file data; **copy-on-write** after `fork`, using the PTE's `SW` trit to mark COW pages.

### 3.3 Page faults

The fault handler (vector −3 or −4) decides between:
- demand-zero (heap, stack, bss);
- copy-on-write;
- loading a page from a file;
- `SIGSEGV`.

## 4. Processes

- **`task_struct`:** PID, parent, state (running / sleeping / zombie / stopped), saved registers, address space (`mm`), open files, current directory, signal state, credentials (uid/gid/euid), process group and session.
- **Scheduler:** round-robin with priorities (nice values) in v1, preemptive on the 100 Hz timer. A CFS-like fair scheduler can come later.
- **Context switch:** save `r-13`…`r13`, `PC` and `MODE`; switch the stack pointer `KSP` and `PTB`; flush the TLB (keeping `G` entries).
- **System calls:**
  - creating and replacing: `fork`, `vfork`, `execve`;
  - ending and waiting: `exit`, `exit_group`, `wait4`/`waitpid`;
  - identity: `getpid`, `getppid`, `getuid`/`setuid` and friends;
  - groups and sessions: `setpgid`, `setsid`;
  - memory and time: `brk`, `mmap`/`munmap` (anonymous first), `nanosleep`.
- **Kernel threads** for deferred work (flushing dirty pages), as in Linux.

## 5. System-call interface ⚠ REVIEW

- **Trap:** `SYS n` (ISA vector 0). Arguments in `r1`–`r6`, result in `r1`, as in Trine's calling convention.
- **Errors:** a failing call returns **`−errno`**, the Linux convention. The C-library layer sets `errno` and returns −1.
- **Numbers:** Linux's **generic system-call table** (`asm-generic/unistd.h`, the one RISC-V and ARM64 use). For example:

| | | | |
|---|---|---|---|
| `openat` 56 | `close` 57 | `read` 63 | `write` 64 |
| `lseek` 62 | `fstat` 80 | `exit` 93 | `exit_group` 94 |
| `kill` 129 | `rt_sigaction` 134 | `getpid` 172 | `brk` 214 |
| `clone` 220 (fork) | `execve` 221 | `wait4` 260 | |

All of these fit `SYS`'s ±9841 range.
- **Error and signal numbers** follow Linux: `ENOENT` = 2, `SIGSEGV` = 11, `SIGCHLD` = 17, and so on.

## 6. Files

**VFS**, with Linux's four objects:

| Object | Role |
|---|---|
| superblock | a mounted file system |
| inode | a file, directory or device |
| dentry | a name in the path cache |
| file | an open file: position, flags |

**System calls:**
- open/close/read/write family: `openat`/`open`, `close`, `read`, `write`, `lseek`, `pread`/`pwrite`;
- descriptors: `dup`, `dup2`/`dup3`, `pipe2`, `fcntl`, `ioctl`;
- metadata: `stat`/`fstat`/`lstat`;
- directories: `getdents64`, `mkdirat`, `unlinkat`, `renameat`, `linkat`, `symlinkat`, `readlinkat`, `chdir`, `getcwd`;
- attributes: `chmod`, `chown`, `umask`, `truncate`;
- sync and mounts: `fsync`, `mount`/`umount`.

**Permissions:** POSIX mode bits (owner/group/other rwx, setuid/setgid/sticky), stored as ordinary integers.

**File systems:**

| Name | Linux model | Purpose |
|---|---|---|
| `ramfs` | ramfs/tmpfs | the initramfs root, `/tmp` |
| `devfs` | devtmpfs | `/dev`: `null`, `zero`, `tty`, `console`, `ttyS0`, `fb0`, `hda`, `hdb` |
| `procfs` | procfs | `/proc/<pid>/…`, `/proc/meminfo`, `/proc/uptime` |
| **`tfs`** | ext2 | the on-disk file system ⚠ REVIEW |

**`tfs`, the "ternary file system":**
- A block is one disk sector (3^9 trytes).
- On-disk structures: a superblock, an inode table, a block bitmap (one trit per block: free / used / bad), and directories holding (inode, name) entries.
- Inodes have direct, single- and double-indirect block pointers, as in ext2.
- File names are Unicode, one tryte per character, up to 81 characters.

**Pipes and FIFOs:** a 3^9-tryte ring buffer, with POSIX atomic writes up to `PIPE_BUF` = 2187 trytes.

## 7. Terminals

- **tty layer:** the console (keyboard plus the 80×30 text screen) is `/dev/tty1`, and the serial port is `/dev/ttyS0`.
- **Line discipline:** canonical mode (line editing with erase, kill and EOF characters) and raw mode. Signals from `^C` (SIGINT), `^\` (SIGQUIT) and `^Z` (SIGTSTP).
- **termios:** `tcgetattr`/`tcsetattr` through `ioctl`, supporting `ICANON`, `ECHO`, `ISIG`, `VMIN`/`VTIME`, and the window size `TIOCGWINSZ`.
- **Job control:** foreground process groups, `tcsetpgrp`, and `SIGTTIN`/`SIGTTOU`.
- **Escape sequences:** a VT100 subset on the console (cursor movement, clear, colours), so ordinary terminal programs work.

## 8. Signals

- **The POSIX set with Linux numbers:** `SIGHUP` 1, `SIGINT` 2, `SIGQUIT` 3, `SIGILL` 4, `SIGKILL` 9, `SIGSEGV` 11, `SIGPIPE` 13, `SIGALRM` 14, `SIGTERM` 15, `SIGCHLD` 17, `SIGCONT` 18, `SIGSTOP` 19, `SIGTSTP` 20, …
- **System calls:** `rt_sigaction`, `rt_sigprocmask`, `kill`, `sigreturn`, `pause`/`sigsuspend` and `alarm`.
- **Delivery:** on return to user mode, the kernel builds a signal frame on the user stack and enters the handler. The handler returns through a `sigreturn` trampoline, as on Linux.
- **CPU faults become signals:**

| Fault | Signal |
|---|---|
| illegal instruction | `SIGILL` |
| page or protection fault | `SIGSEGV` |
| misaligned access | `SIGBUS` |
| divide by zero | `SIGFPE` |
| breakpoint | `SIGTRAP` |

## 9. Executables ⚠ REVIEW

**TEF, the Ternary Executable Format**, is a small ELF-like format. Its header holds:
- a magic word;
- the entry point;
- a list of segments, each with a virtual address, sizes in the file and in memory, and permissions (R/W/X, mapped to the PTE's `V` and `X` trits).

`trine build --user` produces TEF binaries linked at `USER_BASE`. `execve` maps their segments (demand-loaded through the page cache), builds the stack with `argc`, `argv`, `envp` and an auxiliary vector, and jumps to the entry point in ring −1. Shell scripts start with `#!`.

## 10. The C library, in Trine (`tlibc`)

**POSIX functions with POSIX names and semantics**, in Trine syntax:
- I/O and files: `open`, `read`, `write`, `close`, `stat`, `opendir`/`readdir`;
- processes and signals: `fork`, `execve`/`execvp`, `waitpid`, `exit`, `kill`, `signal`/`sigaction`, `pipe`, `dup2`;
- terminals and time: `isatty`, `tcgetattr`, `time`, `sleep`;
- environment and strings: `getenv`/`setenv`, `strerror`, `errno`.

**Formatted output:** Trine has no variadic functions, so instead of `printf` there is a small typed formatter:

```
fmt_str(f, "pid ");
fmt_int(f, pid);
fmt_nl(f);
```

⚠ REVIEW: alternatively, add variadic functions to Trine for a real `printf`.

**Buffered I/O:** `stdin`, `stdout` and `stderr` as `FILE` streams.

## 11. User space

- **`/sbin/init`:** mounts `/proc`, starts `getty`/`sh` on the console and the serial port, and reaps orphans.
- **`/bin/sh`:** a POSIX shell subset:
  - simple commands, pipelines, `&&`/`||`, `;`, `&`;
  - redirections `<`, `>`, `>>`, `2>&1`;
  - variables, quoting, `$?`;
  - `if`/`while`/`for`;
  - builtins: `cd`, `exit`, `export`, `jobs`, `fg`/`bg`.
- **Core utilities:** `ls cat echo pwd mkdir rmdir rm cp mv ln touch chmod ps kill sleep date uname true false head tail wc grep sort env clear`.

## 12. Known departures from POSIX

1. **Bytes:** a byte is a 9-trit tryte, so `CHAR_BIT` is not 8 and text is Unicode code points, not UTF-8.
2. **Languages:** there is no C compiler, so no `c99`; programs are written in Trine.
3. **Formatted output:** there is no variadic `printf` unless Trine gains varargs (§10).
4. **Threads:** no `pthread` in v1. `clone` exists only in its fork form.
5. **Users:** single-user at first (root). The permission checks exist, but there are no login or `passwd` tools yet.

## 13. Milestones and review points

**Milestones.** Each one ends with tests that run on the emulator.

| # | Milestone | Done when |
|---|---|---|
| K1 | loader, `start_kernel`, paging on, `printk`, frame allocator, timer interrupt | the kernel boots from a disk image and prints its banner |
| K2 | tasks, scheduler, context switch, user mode, first system calls (`write`, `exit`, `getpid`) | two user processes print, interleaved under the timer |
| K3 | `fork`/`execve`/`wait4`, COW, page faults, TEF loader, initramfs (ramfs) | `init` starts a program from the initramfs and waits for it |
| K4 | VFS complete, devfs, the tty with its line discipline, pipes, `dup2` | a shell runs `echo hi \| cat` |
| K5 | signals, job control, `^C` | `sleep 100`, then `^C` interrupts it |
| K6 | `tfs` on disk, the buffer cache, `mkdisk`; procfs | files survive a reboot; `ps` works |
| K7 | `tlibc` complete, `sh`, core utilities | an interactive POSIX-like system |
| K8 | (then) the window system on the framebuffer | – |

**Decisions for review:**
1. **The name "Trinux".**
2. **Rings:** the whole kernel, drivers included, in ring +1 (Linux-style), with ring 0 unused in v1.
3. **Address space:** user space is the negative half; the kernel is identity-mapped in the middle (L1 slots 0…+1); a null pointer faults for user code.
4. **System calls:** Linux generic numbers, `−errno` returns, arguments in `r1`–`r6`.
5. **Buddy allocator in powers of 3** ("triddy").
6. **`tfs`** as an ext2-like on-disk file system with one-sector blocks.
7. **TEF** as the executable format.
8. **No `printf`:** typed formatting instead, unless we add varargs to Trine.
9. **"POSIX" means interfaces and semantics with the tryte as the byte** (§0, §12).
