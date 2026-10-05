# Trinux: the Setun-11̅ kernel

**Status: v0.1, accepted. Milestones K1–K3 are implemented** (`kernel/`, built by `mkdisk`).
Items marked **⚠ REVIEW** were open choices at drafting time; §13 lists them, and §14 records what K1 settled.

Trinux is a Unix kernel for Setun-11̅, **modelled on Linux** and providing
**POSIX.1-2017 interfaces**. It is written in Trine and boots through the BIOS
([`BOOT.md`](BOOT.md)).

## 0. What "POSIX" can mean on a ternary machine

POSIX.1-2001 and later require 8-bit bytes (`CHAR_BIT == 8`) and a C compiler
(`c99`). Setun-11̅ has 9-trit trytes, and its language is Trine. So Trinux
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
- On-disk structures: a superblock, an inode table, a block map (a tryte per block holding a trit: free / used / bad), and directories holding (inode, name) entries. As built: §19.
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

**The text cache (as built):** text pages are read-only, so `execve` shares them. Each page it reads stays in a 243-entry cache after the program exits, keyed by the file's inode slot, the inode's generation and the page. The generation is new whenever the slot is filled or the file is written or truncated, so a changed or replaced file never matches. The next exec of the same file maps the cached frames instead of reading the file again, and code the emulator's JIT compiled for those frames stays valid. When memory runs short, `alloc_pages` empties the cache before it fails. Frames still mapped by a process stay with it.

**Clean page tables (as built):** a page table is 3 frames (19683 words), and zeroing one cost more than most of a fork. `free_address_space` now clears the entries it walks (those in `tbl_lo..tbl_hi`, plus an L1 table's kernel slots 0 and +1) and keeps up to 27 tables, all zero, for `alloc_table` to hand out without zeroing. `alloc_pages` gives them back when memory runs short, with the text cache. A test checks that every cached table is entirely zero after processes come and go.

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
- **`/bin/calc`:** a calculator on the FPU (`math.tri`): `+ − * / % ^`, functions, variables, `0t`/`0z` integers, and `:t` to show results in balanced ternary as well (`1/3` → `0t0.1`).

## 12. Known departures from POSIX

1. **Bytes:** a byte is a 9-trit tryte, so `CHAR_BIT` is not 8 and text is Unicode code points, not UTF-8.
2. **Languages:** there is no C compiler, so no `c99`; programs are written in Trine.
3. **Formatted output:** there is no variadic `printf` unless Trine gains varargs (§10).
4. **Threads:** no `pthread` in v1. `clone` exists only in its fork form.
5. **Users:** real users and groups with login, `su` and `passwd` (§22). Passwords use crypt-style `$id$salt$hash` strings with pluggable schemes; the only scheme so far is a single salted MD5, which is fast and therefore weak.

## 13. Milestones and review points

**Milestones.** Each one ends with tests that run on the emulator.

| # | Milestone | Done when |
|---|---|---|
| K1 ✅ | loader, `start_kernel`, paging on, `printk`, frame allocator, timer interrupt | the kernel boots from a disk image and prints its banner |
| K2 ✅ | tasks, scheduler, context switch, user mode, first system calls (`write`, `exit`, `getpid`) | two user processes print, interleaved under the timer |
| K3 ✅ | `fork`/`execve`/`wait4`, COW, page faults, TEF loader, initramfs (ramfs) | `init` starts a program from the initramfs and waits for it |
| K4 ✅ | VFS complete, devfs, the tty with its line discipline, pipes, `dup2` | a shell runs `echo hi \| cat` |
| K5 ✅ | signals, job control, `^C` | `sleep 100`, then `^C` interrupts it |
| K6 ✅ | `tfs` on disk, the buffer cache, `mkdisk`; procfs | files survive a reboot; `ps` works |
| K7 ✅ | `tlibc` complete, `sh`, core utilities | an interactive POSIX-like system |
| K8 ✅ | the window system on the framebuffer | a terminal window runs the shell, with the mouse and keyboard |

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
- `switch_to(prev, next)` pushes `lr`, `fp` and the callee-saved `r-1`–`r-9` (compiled code keeps locals there across `schedule()`), saves `sp` in `prev`'s first field, loads `next`'s, restores and returns; `prepare_stack` builds the same frame for a new task.
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
- **Input:** the keyboard (USB HID codes with shift and ctrl) and the serial port, on IRQs 1 and 3. Keys become characters through `keymap.tri` (in the Trine standard library, shared with `trwm`):
  - two layouts, US and Russian ЙЦУКЕН (as on Windows: `ё` on the backquote key, `№;:?` on Shift+3/4/6/7, `.`/`,` on the slash key);
  - **Alt+Shift** switches layout, whichever key is pressed first. Caps Lock flips the case of letters in both layouts, and Ctrl+letter gives the control code of the key's position, so `^C` stays on the same key in Russian;
  - **Right Alt (AltGr)** held with a digit or a letter (Latin or Cyrillic) types it overlined (the negated code point, ISA §1.1); with any other key it changes nothing. AltGr wins over a Left Ctrl that comes with it (as Windows sends AltGr). `` ` `` is a plain backquote, as over ssh, where an overlined character arrives as the character followed by U+0305;
  - the console and `trwm` each keep their own layout, and `trwm` shows its layout (`EN`/`RU`) at the right end of the panel. The timer tick and every read also poll both devices, so type-ahead queued before the IRQs were enabled is not lost.
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

## 18. K5 as built

**Signals** ([`kernel/signal.tri`](../kernel/signal.tri)): signals 1–31 with Linux numbers. A signal set is one word with bit `sig − 1`; there are no real-time signals yet.
- **Generation:** `send_signal` adds the signal to the target's pending set. Ignored signals are discarded at once, unless blocked.
  - `SIGCONT` resumes a stopped task and cancels pending stop signals; a stop signal cancels a pending `SIGCONT`.
  - A signal that is not blocked wakes a sleeping task.
- **Delivery:** on every return to user mode (after a system call, an IRQ that interrupted user code, or a user fault), the kernel takes the lowest pending signal that is not blocked.

  | Action | Signals | What happens |
  |---|---|---|
  | ignore | `SIGCHLD`, `SIGURG`, `SIGWINCH`, `SIGCONT` | nothing |
  | stop | `SIGSTOP`, `SIGTSTP`, `SIGTTIN`, `SIGTTOU` | `TASK_STOPPED`; the parent gets `SIGCHLD` and a `wait4` wake-up |
  | terminate | everything else | `exit` with the signal number as the wait status |
  | handler | any caught signal | a signal frame is pushed (below) |

  `SIGKILL` and `SIGSTOP` can be neither caught nor blocked.
- **Signal frame:** 113 words, pushed below an 81-tryte red zone under the user `sp`:
  - the whole 28-word trap frame;
  - the old blocked mask;
  - a magic word;
  - a flag and the 82-word FPU state (§23), when the process has used the FPU.

  The handler starts with `r1` = the signal number and `lr` = `sa_restorer` (tlibc's `__restore_rt`: `sys 139`). While it runs, the signal itself and `sa_mask` are blocked.
- **`rt_sigreturn`** restores the frame. It checks the magic word, and that the saved `MODE` trits 23–27 (IOPL, PG, IE, ring) match user mode, so a forged frame cannot raise privileges. `FE` (trit 24) is left out of the check: the kernel sets it on every return.
- **CPU faults** go to a handler when one is installed and the signal is not blocked; otherwise the process is killed with the message as before. A fault inside a system call always kills, since retrying would fault again.
- **System-call restart:**
  - Interrupted sleeps return `ERESTARTSYS` (pipes, tty, `wait4`), `ERESTARTNOHAND` (`sigsuspend`) or `ERESTART_RESTARTBLOCK` (`nanosleep`).
  - If no handler runs (for example, the process was stopped and then continued), the call is re-executed: `EPC` goes back over the 1-word `SYS`, and `r1` gets back the call's first argument.
  - `nanosleep` resumes with the time it had left.
  - With a handler, the call fails with `EINTR`, or restarts if the handler has `SA_RESTART` (for `ERESTARTSYS` only).
- `execve` resets caught signals to `SIG_DFL` and keeps ignored ones; `fork` copies the handlers and the mask, and the child starts with no pending signals.

**Process groups and sessions:** `Task.pgid` and `Task.sid`. Init is pid 1, group 1, session 1, and children inherit both.
- `setpgid` works on the caller or a child in the same session, never on a session leader.
- `getpgid`, `getsid` and `setsid` behave as in POSIX.
- `kill(pid)`:

  | `pid` | Target |
  |---|---|
  | > 0 | that process |
  | 0 | the caller's group |
  | −1 | every process but init and the caller |
  | < −1 | group −pid |

  `ESRCH` when nothing matches; signal 0 only checks.
- `wait4` takes the same `pid` forms. `WUNTRACED` reports a stopped child once, as `stopsig·256 + 127`.

**Terminal job control** ([`kernel/drivers/tty.tri`](../kernel/drivers/tty.tri)):
- With `ISIG`, `^C`, `^\` and `^Z` (`VINTR`, `VQUIT`, `VSUSP`) echo as `^X`, flush the input, and send `SIGINT`, `SIGQUIT` or `SIGTSTP` to the foreground process group.
- `TIOCGPGRP` and `TIOCSPGRP` read and set that group. It must exist in the caller's session.
- A read from a background group gets `SIGTTIN`, or `EIO` if `SIGTTIN` is ignored or blocked. The read restarts once the process is back in the foreground.
- `SIGTTOU`/`TOSTOP` are not implemented.

**Timing:** `nanosleep` (101) sleeps on a channel that the timer wakes every tick; the resolution is one tick (10 ms).

**System calls** (in addition to K4's):

| Call | Number |
|---|---|
| `nanosleep` | 101 |
| `kill` | 129 |
| `rt_sigsuspend` | 133 |
| `rt_sigaction` | 134 (`struct sigaction`: handler, flags, restorer, mask, one word each) |
| `rt_sigprocmask` | 135 |
| `rt_sigpending` | 136 |
| `rt_sigreturn` | 139 |
| `setpgid` / `getpgid` | 154 / 155 |
| `getsid` / `setsid` | 156 / 157 |

**User space:**
- **tlibc:**
  - `sigaction` and `signal`, the `sig*set` functions, `sigprocmask`, `sigpending`, `sigsuspend`;
  - `kill`, `raise`, `pause`;
  - `nanosleep`, `sleep`;
  - the process-group calls, `tcgetpgrp` and `tcsetpgrp`;
  - `WIFSTOPPED` / `WSTOPSIG`, `signame`, `strsignal`.
- **`/bin/sh`** has job control when interactive:
  - each pipeline is a process group, and the foreground one owns the terminal;
  - `&` starts a background job, and `$!` holds its last pid;
  - the builtins `jobs`, `fg`, `bg` and `kill [-s sig | -sig | -l] pid | %job`;
  - "Done", "Stopped", "Terminated" and "Killed" notices appear before the next prompt;
  - `^C` at the prompt drops the line and prints a fresh prompt (a `SIGINT` handler without `SA_RESTART`);
  - the shell ignores `SIGQUIT`, `SIGTSTP`, `SIGTTIN` and `SIGTTOU`, and puts them back to the default in its children;
  - non-interactive `&` jobs ignore `SIGINT` and `SIGQUIT`, as POSIX asks.
- **`/bin/sleep`**, and **`/bin/sigtest`**, which holds the signal scenarios.

**Verified on the emulator:**
- `sleep 100`, then `^C`, prints a prompt, and `$?` is 130;
- 18 `sigtest` checks: handlers, pending while blocked, nested handlers, `SIG_IGN`, uncatchable `SIGKILL`, `SIGPIPE`, `EINTR` with the time left, `SA_RESTART`, stop/continue through `WUNTRACED`, and group kills;
- a job-control session: `&`, `jobs`, `kill %1`, `^Z`, `bg`, `fg`, `^C`, `^C` in a pipeline and at the prompt, and `SIGTTIN` for `cat &`.

## 19. K6 as built

**Boot disk** (`mkdisk`, 3^6 = 729 sectors):

| Sectors | Contents |
|---|---|
| 0 | loader |
| 1…27 | kernel |
| 28…242 | the initramfs, only on a `Root::Initramfs` disk (the fallback) |
| 243…728 | the **tfs root volume**, 486 blocks and 729 inodes |

At boot the kernel mounts the tfs volume as `/`, and prints `VFS: Mounted root (tfs filesystem)`. Without one it unpacks the initramfs into a ramfs. With neither it stops, with Linux's `Kernel panic - not syncing: VFS: Unable to mount root fs`. It then mounts devfs on `/dev`, procfs on `/proc` and a tmpfs (ramfs) on `/tmp`.

**tfs on disk** ([`kernel/fs/tfs.tri`](../kernel/fs/tfs.tri); host side [`crates/trinux/src/tfs.rs`](../crates/trinux/src/tfs.rs)):
- **Blocks:** a block is one sector, 3^9 trytes = 6561 words. Block numbers count from the start of the volume.

  | Block | Contents |
  |---|---|
  | 0 | superblock: magic `0zB007AB18` (115 067 810 204), version, block and inode counts, the block numbers of the maps, inode table and data, free counts, root inode (1), state (1 clean, 0 mounted) |
  | 1 | inode map, a tryte per inode |
  | 2… | block map, a tryte per block (0 free, 1 used, −1 bad) |
  | then | inode table: 243 inodes of 27 words per block |
  | then | data |

- **Inode** (27 words): mode, nlink, uid, gid, size in trytes, atime, mtime, ctime, block count, 15 direct pointers, single indirect, double indirect, generation. An indirect block holds 6561 pointers.
- **Directory:** a file of 81-tryte entries, 243 per block. Each entry is an inode-number word (0 = free slot) and a name of up to 77 characters, one code point per tryte. Every directory starts with `.` and `..`.
- **Mounting:** the kernel checks every superblock field against the disk size and the fixed layout, and refuses an inconsistent volume. The superblock state is set to "mounted" while in use; a volume that was not cleanly unmounted is reported at the next boot.
- **Damage gives EIO:** every block pointer, inode number and inode mode read from the disk is range-checked. Damage gives `EIO` and a `tfs: bad …` message, never a stray access.

**Buffer cache** ([`kernel/fs/bcache.tri`](../kernel/fs/bcache.tri)):
- 27 one-sector buffers in one order-3 triddy block, LRU, write-back.
- Dirty buffers reach the disk:
  - on eviction;
  - on `sync`/`fsync`;
  - from the idle loop once 5 s have passed with dirty buffers (like `bdflush`);
  - on `reboot`/`poweroff`, which also marks the volume clean.

**VFS** ([`kernel/fs/vfs.tri`](../kernel/fs/vfs.tri)):
- **Inode table:** one table of 243 in-core inodes for ramfs, tfs and procfs.
  - A ramfs inode *is* its file.
  - A tfs or procfs inode is a cache entry. It can be evicted unless it is held by an open file, a working directory, a mount, or use in the current system call (a per-syscall stamp).
- **Mounts** link a directory (`mounted`) and a file-system root (`covered`). Lookup crosses them both ways, including for `..` and `getcwd`. Mounts stack, as on Linux.
- **getcwd** walks `..` and finds each name in the parent directory (tfs), or from the inode (ramfs, procfs).

**procfs** ([`kernel/fs/procfs.tri`](../kernel/fs/procfs.tri)): contents are generated on every read.

| File | Contents |
|---|---|
| `/proc/meminfo` | memory totals |
| `/proc/uptime` | seconds since boot |
| `/proc/mounts` | the mount table |
| `/proc/<pid>/stat` | Linux field order: pid, comm, state, ppid, pgrp, session, tty, tpgid, …, utime as field 14, starttime as field 22 |
| `/proc/<pid>/status` | name, state, pid, ppid, pgid, sid |
| `/proc/<pid>/cmdline` | the arguments, NUL-separated |

`execve` records `comm` (the last path component) and `cmdline`.

**System calls** (in addition to K5's):

| Call | Number |
|---|---|
| `renameat` | 38 |
| `umount2` | 39 |
| `mount` | 40 (`ramfs`/`tmpfs` and `proc`; `tfs` is the root, so `EBUSY`; anything else `ENODEV`) |
| `statfs` | 43 (one word per field) |
| `sync` / `fsync` | 81 / 82 |
| `reboot` | 142 (Linux magic numbers; restart, power off, halt) |

**Errors:**
- **rename:** `EXDEV` across file systems, `EINVAL` when moving a directory into itself, `ENOTEMPTY` when the target is a full directory, and `ENOTDIR`/`EISDIR` when the types differ.
- **Busy:** `EBUSY` for unmounting or removing something in use, and for removing a working directory (Linux allows the last one).
- **Names:** `ENAMETOOLONG` past 77 characters on tfs, or 26 on ramfs.
- **Space:** `ENOSPC` for blocks and for inodes.

**User space:**
- **tlibc:** `rename`, `sync`, `fsync`, `mount`, `umount`, `statfs`, `reboot`.
- **New programs:**
  - `/bin`: `ps`, `mount`, `umount`, `sync`, `mv`, `touch`, `df`, and `fstest` (the K6 scenarios);
  - `/sbin`: `reboot`, `poweroff`, `halt`.
- The shell looks commands up in `/bin`, then `/sbin`.

**Verified** (`crates/trinux/tests/fs.rs`, `tests/tfs.rs`):
- **Persistence:** files written, moved and powered off survive a reboot; the host reads them back and its fsck finds the volume clean.
- **Shutdown:** pulling the plug is detected at the next boot; `sync` and the 5-second flush put data on the disk.
- **fstest:** about 90 checks pass, most of them error paths (the `ENO*`/`E*` cases above, plus `EBADF`, `EFAULT`, `EMFILE`, `ESPIPE`, `EACCES`/`ENOEXEC` for `exec`, and the mount and `/proc` refusals). Afterwards the host fsck finds no damage.
- **A full volume** (81 blocks) gives `ENOSPC` for data, `mkdir` and inodes, and gets every block and inode back.
- **Damaged disks:**
  - a bad superblock magic, or an inconsistent superblock, leads to the root-fs panic;
  - a damaged block pointer, inode mode or directory entry gives `EIO` and a message, and the system carries on.
- **Fallback:** the initramfs root still boots.
- **`ps` and `/proc`:** `ps` and the `/proc` files show the expected processes and fields.
- **Host refusals:** the host tfs refuses bad names, duplicates, impossible geometry and damaged superblocks, and its fsck reports out-of-range or doubly used blocks, wrong link and free counts, unreachable inodes and bad entries. It found a real inode leak in `mkdisk` on the way.

## 20. K7 as built

**Kernel additions for user space:**
- **Environment:** `execve` copies `envp` along with `argv`, and builds Linux's stack layout (`argv[]`, NULL, `envp[]`, NULL). `envp` arrives in `r3`.
  - The user startup code stores it in the program's `environ` global when the program has one (tlibc does), like glibc's start code.
  - Init gets `HOME=/` and `TERM=linux`, as on Linux.
  - Argument and environment strings share 2187 trytes and 81 slots (`E2BIG` past that).
  - A path or string outside user space gives `EFAULT`.
- **New system calls:**

  | Call | Number |
  |---|---|
  | `fcntl` | 25: `F_DUPFD`, `F_DUPFD_CLOEXEC`, `F_GETFD`/`F_SETFD`, `F_GETFL`/`F_SETFL` |
  | `linkat` | 37: hard links, tfs only; `EPERM` for a directory or on ramfs, `EXDEV` across file systems |
  | `fchmodat` | 53 |
  | `clock_gettime` | 113: `CLOCK_REALTIME` from the RTC, `CLOCK_MONOTONIC` from the ticks |
  | `uname` | 160: six 65-character fields |
  | `umask` | 166: 022 at boot, applied when creating files and directories |
  | `getuid`, `geteuid`, `getgid`, `getegid` | 174–177 (see §22 for the rest of the credential calls) |

- **Close-on-exec:** `O_CLOEXEC` on `openat`, `pipe2` and `dup3`, and `FD_CLOEXEC`; `execve` closes the marked descriptors.
- **Console:** a VT100 subset on the text screen: cursor position (`ESC[r;cH`), `ESC[J`/`2J`, `ESC[K`, cursor moves (`A`–`D`), and colours (`ESC[…m` with 0, 30–37, 40–47). The serial port passes escape sequences through unchanged.
- **Trine and the user layout:**
  - Trine gains the `\e` escape.
  - User code may now be 27 pages (`USER_DATA = USER_BASE + 27·3^9`), to make room for the shell.

**tlibc** is split like the C headers. A program imports only what it uses:

| Module | Contents |
|---|---|
| `lib/tlibc.tri` | the core (unistd, fcntl, signal, stat, dirent, termios, wait, sysinfo, mount, statfs, reboot, utsname, uid calls), plus `getenv`, `execv`, `execvp` (`$PATH`), `exit` |
| `lib/string.tri` | `strcpy`, `strncpy`, `strcat`, `strncat`, `strncmp`, `memcmp`, `memmove`, `strchr`, `strrchr`, `strstr`, `strspn`, `strcspn` |
| `lib/ctype.tri` | the ASCII classes, `toupper`, `tolower` |
| `lib/stdlib.tri` | first-fit `malloc`/`free`/`calloc`/`realloc` over `sbrk` (results are word-aligned, like C's), `strdup`/`strndup`, `strtol` (bases 2–36, `ERANGE` clamping at ±(3^27−1)/2), `atoi`, `setenv`/`unsetenv`, `qsort` |
| `lib/stdio.tri` | buffered `FILE` (`fopen`/`fdopen`/`fclose`/`fflush`, `fgetc`/`ungetc`/`fgets`, `fputc`/`fputs_f`/`fput_int`/`fput_padded`) |
| `lib/getopt.tri` | POSIX `getopt` (clusters, arguments, `:` mode, `--`) |
| `lib/time.tri` | `time`, `gmtime_r`, a `strftime` subset |
| `lib/testexpr.tri` | the `test`/`[` expression, shared by the shell and `/bin/test` |

There is still no `printf`; the typed writers replace it (§12).

**`/bin/sh`** ([`user/bin/sh.tri`](../user/bin/sh.tri)) was rewritten as a tokenizer, a recursive-descent parser building a syntax tree, and an executor:
- **Grammar:**
  - simple commands, pipelines with `!`, lists (`;`, `&`, `&&`, `||`, newline);
  - `if`/`elif`/`else`, `while`, `until`, `for` (with or without `in`), `case` with `|` patterns;
  - `( … )` subshells and `{ … }` groups;
  - redirections on any command, compound ones included (`while read l; do …; done < file`): `<`, `>`, `>>`, `n<&m`, `n>&m`, `n>&-`;
  - here-documents: `<<word` and `<<-word` (leading tabs removed). With an unquoted delimiter the body gets `$` expansions; with any quoting in the delimiter it is taken as typed. Several on one line are read in order. The body goes to the command through a pipe, and since a body (at most 3^9 trytes) always fits in one pipe, no writer process is needed;
  - functions: `name() compound-command [redirections]`. Calls get their own `$1 … $#` (`$0` stays), `return [n]` ends them, prefix assignments (`V=x f`) last for the call only, and `break`/`continue` don't reach loops outside the function. Calls nest at most 81 deep. Names are looked up as special builtins, then functions, then other builtins, then `PATH`.
- **Expansions:**
  - quoting with `'…'`, `"…"` and `\`;
  - `$name`, `${name}`, `${name:-w}`, `${name-w}`, `${name:+w}`, `${name:=w}`, `${name:?w}`, `${#name}`;
  - `$?`, `$$`, `$!`, `$#`, `$@` (`"$@"` keeps the words), `$*`, `$-`, `$0`–`$9`;
  - `$(…)` (nested; trailing newlines removed), and the older `` `…` ``: inside it a backslash is kept unless it comes before `$`, `` ` `` or `\` (or `"` within double quotes), so `` \` `` nests one;
  - `$((…))`: `+ - * / %`, comparisons, `&& || !`, parentheses, variables;
  - `~`, field splitting of unquoted results, and globbing (`* ? [...] [!...]`, sorted; a pattern that matches nothing stays literal).
- **Variables:** `export`, `unset`, and prefix assignments (`VAR=x cmd` exports to that command only). The shell imports the environment at startup, and `PATH` defaults to `/bin:/sbin`.
- **Builtins:** `: true false echo cd pwd exit export unset set shift read test [ break continue jobs fg bg kill wait exec umask type eval . return local alias unalias`.
  - `eval args` joins its arguments with spaces and runs the result. `. file` runs a file in the current shell (a name without `/` is looked up in `PATH`), and `return` inside it ends the file;
  - `unset -f name` removes a function, and `type` reports functions and special builtins;
  - how it works: a function is kept as the source text of its body. Function bodies, `eval` text and `.` files are lexed and parsed after the running program's text, tokens and syntax nodes, run, and then dropped, so the running program is never disturbed;
  - `set -e`, `set -x` and `set -- args`;
  - `break n` and `continue n`;
  - `read` takes one character at a time, so it leaves the rest of its input for the next reader;
  - `exec` with only redirections makes them permanent.
- **Input:** multi-line commands at the terminal continue with `PS2` (`> `).
- **Prompts:** `PS1` and `PS2` are worked out each time they are shown, as in bash: first the escapes `\u` (`$USER`), `\h` (the host name up to its first dot), `\w` (the working directory, `~` for home), `\W` (its last part), `\$` (`#` for root, else `$`), `\n`, `\e`, `\a` and `\\` (`\[` and `\]` are dropped), then parameter, command and arithmetic expansion as in double quotes. So `PS1='\u:\w\$ '` or `PS1='$(pwd)> '` follow `cd`, while `PS1="$(pwd)> "` (double quotes) is expanded once, when it is assigned. The prompt's own commands leave `$?` alone, and a line-editing redraw repeats only the prompt's last line.
- **Line editing** at a terminal: the shell reads each line itself, with the terminal in raw mode (ISIG stays on, so ^C still interrupts).
  - Left/Right, Home/End and ^A/^E move; Backspace, Delete, ^K, ^U and ^W delete; ^D on an empty line ends the input.
  - Up/Down step through the history: the last 81 lines, without repeats in a row. The `history` builtin lists them.
  - Tab completes a command (builtins, functions, aliases, `PATH`) or a file name; the second Tab in a row lists the choices.
  - Typing at the end of the line echoes just the character, so a session looks the same as with canonical mode. Other edits redraw the line with `\r`, `ESC[K` and `ESC[nD`.
  - The keyboard's arrows, Home, End, Insert, Delete and Page Up/Down type VT100 sequences (`ESC [ A` …; `keymap.tri`), on the console and in `trwm`'s terminal alike.
- **Errors:**
  - syntax errors print bash's message ("syntax error near unexpected token `fi'") and set status 2;
  - an expansion error (division by zero, `${x:?}`, a bad substitution) abandons the whole command line, and makes a non-interactive shell exit;
  - commands that are not found give 127, and ones that are not executable give 126.
- **`local` and aliases:**
  - `local name[=value]` in a function saves the variable, and the call puts it back (value and export flag) when it returns. Scope is dynamic, as in dash: functions called from there see the local. `local name` without a value keeps the current value. `export` and `local` are declaration utilities, so `local s=$1` is not split or globbed.
  - `alias name=value`, `alias [name]` (prints `alias name='value'`), `unalias name` and `unalias -a`. An unquoted command word that names an alias is replaced when the command runs, and the command is re-read as text. While a value ends in a blank, the next word is checked too. An alias is never expanded inside its own expansion, so `alias ls='ls -l'` works and alias loops end with "not found". `\ls` or `'ls'` skips the alias. Unlike bash, an alias defined earlier on the same line already applies, and aliases work in scripts too.
- **Not yet:** a here-document's body must lie inside its function's text: if the operator is in a function body, the body text has to come before the closing `}` line.

**Utilities** (`/bin`):
- `head`, `tail`, `wc`;
- `grep` (basic regular expressions, `-i -v -c -n -q`, status 0/1/2);
- `sort` (`-r -n -u`), `uniq` (`-c`);
- `cp`, `ln`, `chmod` (octal and `ugoa±=rwx`);
- `edit [file]`: a full-screen editor in the spirit of nano. Arrows, Home/End and PgUp/PgDn move; ^S saves, ^X exits (asking whether to save changes), ^K/^U cut and paste lines, ^W finds and ^G goes to a line. It uses the terminal size from `TIOCGWINSZ`, shows tabs as spaces to the next multiple of 8, and redraws only the cursor's line while typing;
- the text and file tools, one multi-call program (`tools.tri`, hard-linked under each name):
  - `sed`: addresses `N`, `$`, `/re/`, ranges and `!`; commands `s` (with `g p N`, `&` and `\1`–`\9`), `y d p q = n a i c`; options `-n -e -i`;
  - `tr` (`-c -d -s`, ranges, `[:class:]`, so `tr а-я А-Я` works too) and `cut` (`-c`/`-f` lists, `-d`, `-s`);
  - `od`: offsets in octal, values as `-o` (the default) `-d -x -c`, `-t` for balanced base 27 (three digits per tryte, negative ones overlined) or `-3` for balanced base 3 (nine trits per tryte);
  - `diff` (normal format from the longest common subsequence, `-q`, status 0/1/2), `find` (`-name -type -maxdepth -mindepth -print -exec … ;` and `!`), `xargs` (`-n`, `-I`, status 123/126/127);
  - `md5sum`: text is hashed as UTF-8, so the digests match other systems'.
- Shared libraries: `regex.tri` (BRE with groups and back-references), `fnmatch.tri` (wildcards, also used by `sh`), and `md5.tri`. Since `&` and `|` are ternary min/max, MD5's bit operations run one bit at a time, about 250k instructions per 64-byte block.

A busybox-style multi-call program, stored once with hard links, provides `rmdir basename dirname env printenv uname date clear seq tee yes kill test [`. `mkdisk` makes a hard link for each source file that appears under several names.

**Verified** (`crates/trinux/tests/userland.rs`):
- **The shell language:** each construct above, including nested `continue 2`, `"$@"`, globbing, `while read … < file`, compound redirections, scripts with arguments, `set -e`/`-x`, and multi-line input.
- **Shell errors:** 24 cases, each with its message and status. Expansion errors stop the rest of the line.
- **The utilities**, and 28 utility failure cases, each with its message and status.
- **libctest:** about 80 checks, including:
  - malloc stress;
  - `strtol` edge cases;
  - setenv refusals;
  - environment inheritance through `execvp`;
  - `FILE` I/O;
  - `getopt`;
  - `gmtime` on a leap day;
  - umask, chmod, link and fcntl errors;
  - close-on-exec;
  - `E2BIG`, and `EFAULT` for `execve`.
- **The screen:** `clear` erases it through the VT100 subset.
- **Bugs found:**
  - `chmod` combined permission bits with `|`, which in Trine is tritwise max, not binary OR;
  - `malloc` returned the first tryte instead of a word-aligned address;
  - init passed an empty environment to the shell.

## 21. K8 as built

**The design, as on Linux:** a user-space display server, `trwm`, owns the framebuffer and the input devices. Each client owns one window and talks to the server over a pipe pair. A terminal window runs the shell on a pseudo-terminal.

**Kernel devices:**

| Node | Number | What it does |
|---|---|---|
| `/dev/fb0` | 29,0 | One process may hold it (`EBUSY` otherwise). |
| `/dev/mouse` | 13,63 | Six-word events: x, y (absolute), buttons, wheel, dx, dy. |
| `/dev/kbd` | 13,64 | Raw keyboard events, one word each (±HID code). While it is open the console gets no keys. |
| `/dev/ptmx` | 5,2 | Each open makes a pseudo-terminal pair; the slave appears as `/dev/pts/N`. |

- **`/dev/fb0` details:**
  - `FBIOGET_VSCREENINFO` and `FBIOPUT_VSCREENINFO` (`{xres, yres, pitch, mode}`; mode −1 text, 0 = 640×480, 1 = 800×600);
  - `FBIO_CURSOR` drives the hardware cursor;
  - `read`, `write` and `lseek` work over the pixels;
  - `mmap` (`MAP_SHARED`, offset 0) maps its device frames at a fixed user address (page −3^15). The PTE's SW trit is −1 there, so the pages are never copy-on-write or reference-counted;
  - while a graphics mode is on, console output still reaches the serial port but not the screen;
  - when the owner closes it, or dies, the text console comes back.
- **`/dev/mouse` and `/dev/kbd`:** each can be open once (like a grabbed evdev device). Reads return whole events, and `O_NONBLOCK` gives `EAGAIN`.
- **Pseudo-terminals:**
  - The tty driver has ten instances: tty 0 is the console, ttys 1–9 are ptys. All share the line discipline, `termios`, job-control signals and `SIGTTIN`.
  - The master gives `TIOCGPTN` and accepts `TIOCSPTLCK`. What it writes is typed into the slave; the slave's output, echo included, is read from the master.
  - `TIOCSWINSZ` sets the window size.
  - Closing the master sends `SIGHUP` and `SIGCONT` to the slave's foreground group, and slave reads then see end of file.
  - The master reads `EIO` once the slave has closed. `/dev/pts/N` disappears when both sides are gone, and there are nine ptys (`EAGAIN` after that).
- **`ppoll` (73):**
  - `struct pollfd` is one word per field;
  - readiness is supported for pipes, terminals, pty masters, the input devices and files; `POLLNVAL` is returned for bad descriptors;
  - the timeout is in ticks;
  - a signal gives `EINTR`;
  - every `wake_up` also wakes the pollers, which then check again.
- **Also:** `O_NONBLOCK` for pipes, terminals and the input devices; `mmap` (222) and `munmap` (215) for the framebuffer; and a `WChan:` line in `/proc/<pid>/status`.
- **ioctl order:** an unknown command is `ENOTTY` before the argument is checked, as on Linux.

**The protocol** ([`user/lib/trw.tri`](../user/lib/trw.tri)): the server starts each client with requests on fd 3 and events on fd 4. A message is words: the op, the argument count, then the arguments (strings one character per word).

| Direction | Messages |
|---|---|
| Requests | `CREATE w h title`, `FILL x y w h colour`, `TEXT x y fg bg scale text`, `PIXELS x y w h …`, `SCROLL x y w h dy fill`, `FLUSH`, `TITLE text`, `SPAWN path` |
| Events | `KEY char`, `BUTTON x y buttons`, `MOTION x y buttons`, `CLOSE`, `FOCUS in` |

The server keeps every window's pixels (server-side rendering), so clients never redraw on exposure.

**`trwm`** ([`user/bin/trwm.tri`](../user/bin/trwm.tri)):
- **Desktop:** a panel with launchers (Terminal, Clock, Colors, Paint) and [Exit].
- **Windows:** framed, with a title bar and a close box. Click to raise, drag the title bar to move; keys go to the window in front, and mouse events go to the window under the pointer while a button is held.
- **Drawing:** damage is collected into one rectangle, drawn into a scratch buffer of its size, then copied to the mapped framebuffer. The inner loops are assembly (`lib/gfx.tri`).
- **Clients:** it reaps clients and closes a window when its client exits or is killed. A client that breaks the protocol is logged ("client N: bad request (…)"), sent `SIGTERM`, and cut off. Events go out non-blocking, so a slow client cannot stall the server.

**Clients:**

| Client | What it does |
|---|---|
| `term` | runs `/bin/sh` on a pty in a new session, so job control and `^C` work in the window; a VT100 subset on a 76×24 grid; `SCROLL` moves the server's pixels |
| `clock` | the time, once a second (`poll` with a timeout) |
| `colors` | every one of the 3^9 = 19683 pixel colours, once each |
| `paint` | draws with the mouse, with a colour strip |

`mkdisk` generates `/usr/share/font8x8` from the same font the emulator's text mode uses: 353 glyphs, namely code points 0..255, then the Cyrillic block U+0400..U+045F as glyphs 256..351, then № (for the Russian keyboard layout) as glyph 352. `gfx.tri` maps a code point to its glyph (`font_glyph`) and draws `?` for anything else.

**Pixels and gfx:** a pixel is one tryte: red, green and blue are 3 trits each, so there are 27 levels per channel. `lib/gfx.tri` has surfaces, clipped fill, blit, scroll and text (scaled or not), and a fast path for whole unscaled characters.

**Verified** (`crates/trinux/tests/wm.rs`, `user/bin/k8test.tri`):
- **End to end**, driving the machine's mouse and keyboard and reading the framebuffer back:
  - the panel, the desktop and the focus colours;
  - a paint drag leaves red dots exactly where the pointer went;
  - raising the terminal by its title bar, then typing a command and a pipeline into it: the window's text is read back by matching 8×8 glyphs against the font, and the console sees the file the window's shell wrote;
  - `^C` in the window interrupts `sleep 100`;
  - dragging a window uncovers the one beneath, with its pixels intact;
  - the close box closes Paint, and [Exit] gives the console back in text mode.
- **Refusals:**
  - a second server (`EBUSY` on `/dev/fb0`);
  - clients that draw before `CREATE`, send an unknown op, send an impossible length, or ask for a window bigger than the screen are each cut off with their own message;
  - a missing program is reported;
  - a client that crashes, or is killed with `kill -9`, loses its window while the server carries on.
- **k8test (48 checks):** framebuffer ioctls and `mmap` (including `EBUSY`, `EINVAL`, `EBADF`, `ENODEV`, `ENOTTY`, sharing after fork, and the console coming back); input devices (`EBUSY`, `EAGAIN`, `EINVAL`, `EBADF`); `poll` (timeouts, `POLLHUP`, `POLLNVAL`, `EINVAL`, `EFAULT`, `EINTR`); ptys (line discipline and echo, window size, `^C` to the foreground group, hang-up `SIGHUP`, `EIO`, `ENOENT`, nine then `EAGAIN`).
- **Bugs found on the way:**
  - `mmap`'s result is a negative address here, which the wrapper took for an error; only −1…−4095 are errors, as on Linux;
  - tryte buffers inside structs need not be word-aligned, so the server decodes words arithmetically instead of casting pointers;
  - an unknown `ioctl` returned `EFAULT` instead of `ENOTTY`;
  - `lseek` was refused on `/dev/fb0`.

## 22. Users and logins

**Credentials** (`kernel/cred.tri`):
- **What a task has:** real, effective and saved user and group IDs, plus up to 9 supplementary groups. `fork` copies them.
- **Set-ID programs:** `execve` of a set-user-ID (04000) or set-group-ID (02000) file takes the file's owner or group as the effective ID.
- **Calls,** with Linux's numbers and rules: `setuid` 146, `setgid` 144, `setreuid` 145, `setregid` 143, `setresuid`/`getresuid` 147/148, `setresgid`/`getresgid` 149/150, `getgroups`/`setgroups` 158/159 (setting needs root), `fchownat` 54 and `fchown` 55.

**Ownership:** every inode has an owner and a group.
- tfs keeps them in its on-disk inodes (words 2 and 3).
- A new file belongs to its creator's effective IDs.
- `stat` reports them, and `/proc/<pid>/status` shows `Uid:` and `Gid:` lines.

**Permission checks:** by the owner, group (including supplementary groups) or other bits.
- Root passes everything, except that it needs some execute bit to run a file.
- **Paths:** search (x) on every directory walked through.
- **open:** read and/or write by the access mode; `O_TRUNC` needs write. A file the call itself creates opens whatever its mode.
- **Creating, removing, renaming, linking:** write and search on the directory. A sticky directory (01000, e.g. `/tmp`, which is mounted 01777) lets only the entry's owner, the directory's owner or root remove or rename an entry.
- **chdir:** search. **execve:** execute.
- **chmod:** owner or root.
- **chown:** root may give anything. The owner may only change the group, to one of its own groups. A change by anyone else clears the set-ID bits.
- **kill:** root, or a real/effective user ID equal to the target's real/saved one. `SIGCONT` also reaches the same session. Otherwise `EPERM`.
- **mount, umount, reboot:** root only (`EPERM`).

**Accounts:**
- `/etc/passwd` (`name:x:uid:gid:gecos:home:shell`), `/etc/group` (`name:x:gid:members`) and `/etc/shadow` (`name:hash:…`, mode 0600).
- `user/lib/pwd.tri` reads them: `getpwnam`, `getpwuid`, `getgrnam`, `getgrgid`, `getspnam`, `initgroups`. `user/lib/dbedit.tri` rewrites them through a temporary file and a rename.
- The disk comes with `root` (password `root`), `user` (uid 1000, password `user`) and the group `wheel` holding both. `mkdisk` sets owners and modes from `SPECIAL_MODES`: `/etc/shadow` 0600, `su` and `passwd` set-user-ID root, `/root` 0700, `/home/user` owned by `user`.

**Password hashing** (`user/lib/crypt.tri`), modular in the style of crypt(3):
- **Format:** a stored hash reads `$id$salt$hash`; the id picks the scheme. Hashes from different schemes can live together, and an old one keeps working after the default changes.
- **Adding a scheme:** write a function `(key, salt, out)` and register it in `crypt_init()`.
- **The `md5` scheme:** the hex MD5 of salt and password, the only scheme so far. `crypt_new` makes a new 8-character salt from the clock, the pid, a counter and the previous salt.
- **Entries that match nothing:** locked ones (`!`, `*`) and unknown schemes.

**Programs:**
- **init:** runs `/bin/login` on the console and starts it again whenever the session ends ("`init: login session exited with N`"). If login cannot run three times in a row, it falls back to a root shell.
- **login:**
  - asks for a name and a password, the password with the echo off, and allows three tries;
  - hands `/dev/console`, `fb0`, `mouse` and `kbd` to the user, so they can run the window system;
  - sets the groups, the group and the user;
  - shows `/etc/motd`, then starts the shell as `-sh` in the home directory, with `HOME USER LOGNAME SHELL PATH TERM` set;
  - `login -f user` (root only) skips the password.
- **su [-] [user] [-c cmd]** (set-user-ID root): anyone but root must give the target's password.
- **passwd [user]** (set-user-ID root): users change their own after giving the current password; root may change anyone's.
- **useradd** `[-u uid] [-G groups] [-d home] [-s shell] [-c comment] name` gives the new user a private group (same number, from 1000 up), a home directory and a locked password. **userdel** `[-r] name` removes the user, its group and its memberships. Both are root only.
- **id, whoami, groups, chown, chgrp** are applets of `misc`. **ls -l** shows owner and group names, and `s`/`t` for the set-ID and sticky bits.
- **Tests:** they log in as root at the first prompt. `tests/users.rs` covers accounts, set-ID `passwd`, `su`, login failures and every permission refusal.

## 23. The FPU

[`kernel/fpu.tri`](../kernel/fpu.tri) switches the FPU registers (ISA §14) **lazily**, as Linux did on the 386 with `CR0.TS`:

- **One owner.** The hardware holds one task's 27 registers and FSR at a time: `fpu_owner`'s. Every other task's state is in its `Task.fpu`: 82 words, the FSR and then `f−13`…`f13` as real81.
- **Trapping the first use.** `trap_return` calls `fpu_fixup`, which sets `MODE.FE` in the frame of every return to user mode: +1 for the owner, 0 for everyone else. A task that is not the owner traps (vector −12) on its first FPU instruction. The handler saves the owner's registers, loads the current task's and makes it the owner, then retries the instruction. A task that never touches the FPU never pays for it, and switching between two FPU users costs one trap per time slice.
- **A fresh state** (a new program, or a signal handler): all registers zero and the reset FSR, with every exception masked. `Task.fpu_used` stays 0 until the first FPU instruction, so the kernel neither saves nor loads anything for such tasks.
- **fork** copies the parent's state (it is flushed first if it is live). **execve** and **exit** drop it.
- **Signals:** the interrupted state goes into the signal frame and the handler starts fresh; `rt_sigreturn` restores the state from the frame and discards the handler's.
- **Exceptions:** an unmasked FPU exception (vector −11) is `SIGFPE`, like an integer divide by zero. Vector −12 in the kernel is a panic, since the kernel itself only touches the FPU in `fpu_save` and `fpu_load` (with its own `MODE.FE` turned on first).
- **`/proc/<pid>/status`** has an `FPU:` line: `unused`, `live` (the owner) or `saved`.
- **Tests:** `/bin/fputest` (`tests/boot.rs`) keeps values in `f12` and `f−12`, which compiled Trine code never uses, so only the kernel can preserve them. It checks two processes trading the FPU, fork, execve, a signal handler, and two refusals: unmasked divide by zero and an unmasked invalid `sqrt` kill only the offending child with `SIGFPE`. With switching turned off, five of its checks fail.

## 24. Networking

The TCP/IP stack, sockets and the network programs are described in [`NETWORK.md`](NETWORK.md). In short: `kernel/drivers/tnic.tri` and `kernel/net/` (Ethernet, ARP, IPv4, ICMP, UDP, TCP); sockets are anonymous `I_SOCK` inodes behind file descriptors, with Linux's system calls 198..210 and 242; `/proc/net/{dev,arp,tcp,udp,stat}`; init starts `/sbin/telnetd` when `eth0` is up.
