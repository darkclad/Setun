# Trinux: the Setun-386 kernel

**Status: v0.1, accepted. Milestones K1–K3 are implemented** (`kernel/`, built by `mkdisk`).
Items marked **⚠ REVIEW** were open choices at drafting time; §13 lists them, and §14 records what K1 settled.

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
| K1 ✅ | loader, `start_kernel`, paging on, `printk`, frame allocator, timer interrupt | the kernel boots from a disk image and prints its banner |
| K2 ✅ | tasks, scheduler, context switch, user mode, first system calls (`write`, `exit`, `getpid`) | two user processes print, interleaved under the timer |
| K3 ✅ | `fork`/`execve`/`wait4`, COW, page faults, TEF loader, initramfs (ramfs) | `init` starts a program from the initramfs and waits for it |
| K4 ✅ | VFS complete, devfs, the tty with its line discipline, pipes, `dup2` | a shell runs `echo hi \| cat` |
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

## 14. K1 as built

```
BIOS → disk 0 sector 0 (kernel/boot/loader.tri)
     → loader reads sectors 1..K into frames 4..  (K and the entry are patched in by mkdisk)
     → kernel crt0 → main(disk, ram) → start_kernel()
```

**Physical layout:**

| Frames | Contents |
|---|---|
| 1 | loader |
| 2 | loader stack |
| 3 | kernel boot stack |
| 4…30 | kernel image (≤ 27 sectors) |
| 31…57 | kernel globals (31…39 until K4) |
| 58… | the triddy allocator (40… until K4) |

**Files:**

| File | Contents |
|---|---|
| `kernel/params.tri` | layout and PTE constants |
| `kernel/printk.tri` | `printk` with uptime stamps, `panic` |
| `kernel/mm.tri` | the triddy allocator: blocks of 3^k frames centred on multiples of 3^k, split into three, merge three siblings; self-test |
| `kernel/arch.tri` | one `interrupt fn trap_entry` for all 243 vectors (IRQs are claimed from the TPIC; kernel faults panic with vector, EPC and address), plus `paging_init` (the L1 table and two L2 tables, each a 3-frame order-1 triddy block) |
| `kernel/main.tri` | `start_kernel`: banner, traps, mm, paging, 100 Hz timer, idle in `hlt` |

`cargo run -p trinux --bin mkdisk -- -o trinux.disk` builds the disk, and `setun --disk trinux.disk` boots it. `crates/trinux/tests/boot.rs` boots it on the emulator.

## 15. K2 as built

**Trap path:**
- `trap_stub` (assembly in `kernel/arch.tri`) saves every register, plus `EPC`, `EMODE` and `KSP`, as a 28-word frame on the current kernel stack.
- It calls `trap_handler(frame)`, then restores the frame and runs `IRET`.
- System calls read `r1`–`r6` from the frame and write the result back into the frame's `r1`.

**Context switch:**
- `switch_to(prev, next)` pushes `lr` and `fp`, saves `sp` in `prev`'s first field, loads `next`'s, pops and returns.
- A new task's kernel stack is pre-built (a frame with `EPC` = entry, `EMODE` = ring −1 + IE + PG, `KSP` = user stack, under a fake switch frame whose `lr` is `trap_return`). So its first switch "returns" into user mode.
- Kernel code runs with interrupts off (as in early single-CPU Linux). The idle task, task 0 (the `start_kernel` loop), schedules with interrupts off, then sleeps in `hlt` with them on.

**Address spaces:** each process has its own L1 table, sharing the kernel's slots 0 and +1. User pages are ring −1.

| Region | Address | Pages |
|---|---|---|
| code | from `USER_BASE` = −3^25 | up to 9 |
| globals | 9 pages up | 3 |
| stack | top at −3^18 − 3 | 3 |

**Boot modules** (the precursor of the initramfs):
- `mkdisk` writes a table at sector 28: magic, count, then (first sector, sector count, first page) for each program.
- Each program follows as page images, compiled with `trine`'s user kind (unprivileged startup code that ends in `exit`).
- The kernel loads each program into fresh frames and creates a process for it.

**System calls:**

| Call | Number |
|---|---|
| `write` | 64 (fd 1 and 2: the console) |
| `exit` / `exit_group` | 93 / 94 |
| `sched_yield` | 124 |
| `getpid` | 172 |

Anything else returns `−ENOSYS`.
- Pointers are checked against the user range (`−EFAULT`), and bad descriptors get `−EBADF`.
- A fault in user mode kills the process and prints the cause; the kernel carries on.

**Known limits until K3:** exited processes stay zombies and keep their memory (there is no `wait` yet), and pages are mapped read/write/execute.

## 16. K3 as built

**Boot to init:** `mkdisk` packs an **initramfs** at sector 28.
- The header is words: magic, file count, data sectors, then per file a 27-tryte path, size and offset. The data follows from sector 29.
- The kernel loads it into one contiguous triddy block and starts **`/sbin/init` as PID 1**, with `argv = { "/sbin/init" }`.
- If init exits, the kernel prints Linux's `Kernel panic - not syncing: Attempted to kill init!`.

**TEF v1:** a 6-word header (magic, entry, text start, text size, bss start, bss size), followed by the text.
- `execve` maps the text read-only + execute.
- The heap runs from the bss start up to `brk` and is demand-zero.
- The stack is 27 pages below `USER_LIMIT`, demand-zero, with its top page mapped up front. It holds the `argv` strings, `argv[]`, NULL and an empty `envp`.
- `main(argc, argv)` gets them in `r1` and `r2`.

**Copy-on-write fork:** `clone(SIGCHLD)`.
- Writable pages become read-only with the PTE's `SW` trit set in both processes. `frame_ref` counts the sharers.
- A write fault copies the page, or simply makes it writable again when only one sharer remains.

**Page faults:**

| Fault | Result |
|---|---|
| not present, in the heap or stack | demand-zero |
| protection fault on a write to a COW page | copy |
| illegal or privileged instruction | `SIGILL` |
| misaligned access | `SIGBUS` |
| divide by zero | `SIGFPE` |
| breakpoint, single step | `SIGTRAP` |
| anything else | `SIGSEGV` |

Kernel-mode faults on user addresses go through the same path, so system calls can touch demand-zero and COW user memory.

**`exit` and `wait4`:**
- `exit` frees the address space at once, reparents children to init, and wakes a parent sleeping in `wait4`.
- `wait4(pid or −1, status, WNOHANG)` reaps a zombie (freeing its kernel stack) and returns Linux-encoded status: `code·256`, or the signal number.
- It returns `−ECHILD` when there are no children, and sleeps otherwise.

**System calls:**

| Call | Number |
|---|---|
| `write` | 64 |
| `exit` / `exit_group` | 93 / 94 |
| `sched_yield` | 124 |
| `getpid` | 172 |
| `getppid` | 173 |
| `sysinfo` | 179 (Linux field order, `mem_unit` = 1 tryte) |
| `brk` | 214 |
| `clone` (fork) | 220 |
| `execve` | 221 |
| `wait4` | 260 |

**Verified on the emulator by `/sbin/init`**, which became [`/bin/k3test`](../user/bin/k3test.tri) in K4:
- fork + exec + wait with `argv`;
- COW isolation;
- `SIGSEGV` reporting;
- `ECHILD`;
- no leaked pages across 5 fork/exec/wait cycles (checked with `sysinfo.freeram`);
- the heap via `sbrk`.

## 17. K4 as built

**Boot to a shell:** `start_kernel` builds the VFS, `/dev` and `/tmp`, starts the tty, and unpacks the initramfs into ramfs. `/sbin/init` ([`user/init.tri`](../user/init.tri)) starts `/bin/sh` on `/dev/console` (descriptors 0, 1, 2), reaps orphans, and exits with the shell's status.

**VFS** ([`kernel/fs/vfs.tri`](../kernel/fs/vfs.tri)): a simplified version of Linux's objects.

| Object | As built |
|---|---|
| inode | a table of 243: type (regular, directory, character device, FIFO), mode, size, data, device number, open count. Freed on its last close once unlinked. |
| dentry | merged into the inode: one name per inode (no hard links yet); a directory keeps its children on a sibling list |
| file | a table of 243 open files: references, inode, position, `O_*` flags. Shared after `fork` and `dup`, as in Unix. |
| descriptors | 27 per task (`Task.fds`), plus `Task.cwd` |
| superblock | not yet: ramfs is the only file system until `tfs` (K6) |

- Path lookup handles absolute and relative paths, `.`, `..`, repeated slashes and `ENAMETOOLONG` (26-character names).
- **ramfs data:** a regular file's contents live in one triddy block that grows by powers of 3. An initramfs file points straight into the archive until its first write copies it out.
- `exec` now looks programs up through the VFS (relative to the cwd) and needs the owner-execute bit.

**devfs** ([`kernel/fs/devfs.tri`](../kernel/fs/devfs.tri)): Linux device numbers.

| Node | Device | Number |
|---|---|---|
| `/dev/null` | discards writes, reads EOF | 1,3 |
| `/dev/zero` | reads zeros | 1,5 |
| `/dev/tty` | the console tty | 5,0 |
| `/dev/console` | the console tty | 5,1 |
| `/dev/ttyS0` | the console tty | 4,64 |

Every terminal node is the same tty in K4; separate virtual consoles come with the window system.

**tty** ([`kernel/drivers/tty.tri`](../kernel/drivers/tty.tri)), after Linux's `n_tty`:
- **Input:** the keyboard (USB HID codes with shift and ctrl) and the serial port, on IRQs 1 and 3. The timer tick and every read also poll both devices, so type-ahead queued before the IRQs were enabled is not lost.
- **Canonical mode:** erase (`^H`, DEL), kill (`^U`), EOF (`^D`), `ICRNL`, echo (control characters as `^X`). A read returns at most one line.
- **Raw mode:** clear `ICANON` and characters pass straight through.
- **ioctl:** `TCGETS`, `TCSETS`/`TCSETSW`/`TCSETSF` and `TIOCGWINSZ` (80×30, 640×480). `termios` has one word per flag field, then `c_line` and `c_cc[19]` as trytes; Linux bit values.
- **Output:** the serial port and the text screen.

**Pipes** ([`kernel/fs/pipe.tri`](../kernel/fs/pipe.tri)): 27 pipes, each a 3^9-tryte ring buffer in one frame. Reads block while the pipe is empty and a writer is left; after that they return EOF. Writes block while it is full; writing with no reader returns `EPIPE` (`SIGPIPE` comes in K5).

**Sleeping:** `sleep_on(chan)` and `wake_up(chan)` with a per-task `wchan`. Pipes, the tty and `wait4` all use them, and sleepers re-check their condition in a loop.

**System calls** (in addition to K3's):

| Call | Number |
|---|---|
| `getcwd` | 17 |
| `dup` / `dup3` | 23 / 24 |
| `ioctl` | 29 |
| `mkdirat` / `unlinkat` | 34 / 35 (`AT_REMOVEDIR`) |
| `chdir` | 49 |
| `openat` | 56 (`AT_FDCWD`, `O_CREAT`, `O_EXCL`, `O_TRUNC`, `O_APPEND`, `O_DIRECTORY`) |
| `close` | 57 |
| `pipe2` | 59 |
| `getdents64` | 61 |
| `lseek` | 62 |
| `read` / `write` | 63 / 64 |
| `newfstatat` / `fstat` | 79 / 80 |

- `struct stat` has one word per field: dev, ino, mode, nlink, uid, gid, rdev, size, blksize, blocks, atime, mtime, ctime.
- A `linux_dirent64` record is `d_ino`, `d_off`, `d_reclen` (words), `d_type` (tryte 9), then the NUL-terminated name, padded to whole words.

**User space:**
- **tlibc** gained the file, directory (`opendir`/`readdir`), termios, `stat` and `strerror` APIs.
- **`/bin/sh`** ([`user/bin/sh.tri`](../user/bin/sh.tri)) supports:
  - pipelines, and lists with `;`, `&&` and `||`;
  - redirections `<`, `>`, `>>`, `n>`, `n>&m`;
  - quoting with `'…'`, `"…"` and `\`;
  - `#` comments, `$?` and `$$`;
  - the builtins `cd`, `pwd` and `exit`;
  - `sh -c` and `sh script`.

  Commands without a `/` are looked up in `/bin`.
- **Utilities:**
  - `echo [-n]`;
  - `cat`;
  - `ls [-al]`, sorted;
  - `mkdir [-p]`;
  - `rm [-d]`;
  - `true`, `false`;
  - `k3test`, `hello`, `k2_hello`.
- `mkdisk` compiles `.tri` files to TEF and copies anything else as text, for example `/etc/motd`.

**Performance:** fork + exec + exit + wait fell from about 17M instructions to 1.5M.
- Each page table records its lowest and highest used entries (two trytes per table frame), so address-space walks skip the empty parts.
- Zeroing and copying frames are 5-instruction assembly loops.
- `exec` copies the text a word at a time.

**Verified on the emulator** (`crates/trinux/tests/boot.rs`, typing at the prompt over the serial port):
- `echo hi | cat` and `exit 5`;
- redirection and append, `cat < f | cat | cat`, `mkdir -p` with `cd`/`pwd`, and `ls` / `ls -l`;
- `$?` after `false` and after a missing command (127), the `&&`/`||` chains and quoting;
- error messages through `2>&1 >/dev/null | cat`, and `rm`;
- `/bin/k3test` run from the shell.
