# Trinux manual

Trinux is the operating system of Setun-11̅: a Unix kernel modelled on
Linux, a C library, a shell, the usual utilities, a window system and a
network stack, all written in Trine. This manual describes the system as it
is built, by topic, for someone who wants to use it and to write programs
for it.

It is the reference. The design log, organized by milestone, is
[`KERNEL.md`](KERNEL.md); networking is in [`NETWORK.md`](NETWORK.md) and
the self-hosted toolchain in [`SELFHOST.md`](SELFHOST.md). Those files keep
the history and some finer detail. Where this manual and the code disagree,
the code is right.

## Contents

1. [Overview](#1-overview)
2. [Getting started](#2-getting-started)
3. [The shell](#3-the-shell)
4. [Command reference](#4-command-reference)
5. [Files and file systems](#5-files-and-file-systems)
6. [Processes](#6-processes)
7. [Memory](#7-memory)
8. [Terminals](#8-terminals)
9. [Signals](#9-signals)
10. [The FPU and the vector unit](#10-the-fpu-and-the-vector-unit)
11. [Text](#11-text)
12. [The window system](#12-the-window-system)
13. [Networking](#13-networking)
14. [Programming for Trinux](#14-programming-for-trinux)
15. [System call reference](#15-system-call-reference)
16. [Library reference](#16-library-reference)
17. [Departures from POSIX, and limits](#17-departures-from-posix-and-limits)
18. [How Trinux is tested](#18-how-trinux-is-tested)

---

## 1. Overview

### 1.1 What Trinux is

Trinux is a monolithic kernel. The scheduler, the memory manager, the VFS,
the file systems, the drivers and the TCP/IP stack all run in ring +1. Ring
0 is unused, and every process runs in ring −1. The kernel boots from disk
through the BIOS ([`BOOT.md`](BOOT.md)) and starts `/sbin/init` as process 1.

`uname -a` prints:

```text
Trinux setun 0.1 #8 K8 balanced ternary setun11̅
```

### 1.2 Design principles

- **POSIX interfaces on a ternary machine.** Trinux offers POSIX system
  interfaces with POSIX semantics. The tryte plays the role of the byte:
  `CHAR_BIT` is 9, `read(fd, buf, n)` moves `n` trytes, and sizes and
  offsets count trytes.
- **Text is code points.** A tryte holds one Unicode code point. Text is
  not UTF-8 inside the machine. UTF-8 appears only at the border: the
  serial console, the network, and file transfer (§11).
- **Linux numbers.** System call numbers come from Linux's generic table
  (`asm-generic/unistd.h`, as used by RISC-V and ARM64). Error numbers,
  signal numbers, open flags, `ioctl` numbers and device numbers are
  Linux's too. A failing system call returns `−errno`.
- **One word per field.** System structures (`struct stat`, `struct
  sigaction`, `struct pollfd`, `struct sockaddr_in`, `termios` flag fields)
  have one 27-trit word per field.
- **Powers of 3.** Where Linux picks a size freely, Trinux picks a power of
  3: 27 processes, 27 descriptors, 243 open files, 3^9-tryte pages and
  pipe buffers, 27-entry queues.

### 1.3 What runs

On the default disk (`mkdisk`):

- a login on the console, and `telnetd`, `sshd` and `httpd` when a network
  is attached;
- `/bin/sh`, a POSIX-style shell with job control, line editing, history
  and completion;
- 76 programs, several of them under many names: files, text tools
  (`sed`, `diff`, `find`, `xargs`), processes, users and groups, an editor,
  a calculator on the FPU;
- a window system (`trwm`) with a terminal, a clock, a colour chart and a
  paint program;
- networking: `ping`, `nc`, `host`, `wget` (HTTP and HTTPS over TLS 1.3),
  `ssh`, `scp`, `sftp`, `ntpdate`;
- a Trine compiler (`tric`) and an assembler (`tas`) that rebuild every
  program, the compiler included, on Trinux itself;
- test programs that the host's test suites run inside the system.

The disk built by `mkdisk` holds 171 files in a 486-block volume, with 119
blocks free; the kernel takes 9 sectors.

---

## 2. Getting started

### 2.1 Running Trinux

All commands run from the repository root (see [`README.md`](../README.md)).

| Command | Effect |
|---|---|
| `cargo run --release -p trinux --bin mkdisk -- -o trinux.disk` | build a bootable disk image |
| `cargo run --release -p setun-native -- --disk trinux.disk` | boot it in a window (screen, keyboard, mouse) |
| `cargo run --release -p setun-native -- --console` | no window: the terminal is the serial console; without `--disk`, a disk built from the source tree, fresh each time |
| `cargo run --release -p setun-native -- --console --net 2323:23` | with a network; `telnet localhost 2323` logs in |
| `cargo run --release -p setun-native -- --console --net 2222:22` | `ssh -p 2222 root@localhost` logs in |
| `cargo run --release -p setun-native -- --console --net 8080:80` | `http://localhost:8080/` is Trinux's `httpd` |

In `--console` mode, Ctrl-] leaves the emulator.

Options of the native front end, `setun` (from its usage message):

| Option | Meaning |
|---|---|
| `--disk FILE` | the disk image in drive 0 |
| `--ram FRAMES` | RAM in 3^9-tryte pages (default 19683, that is 3^18 trytes) |
| `--mips N` | emulated speed in millions of instructions per second (default 10) |
| `--console` | no window; the terminal is the serial console |
| `--net HOST:GUEST` | attach the network and forward a host port to a guest port; repeatable. `ADDR:HOST:GUEST` listens on another host address; `--net on` attaches the network without forwards |
| `--headless N`, `--keys TEXT`, `--press A,B,..`, `--screenshot F` | run without a window for N instructions, feed input, save the screen |

Forwards listen on 127.0.0.1 unless an address is given. Without `--disk`,
every `--console` boot has a fresh disk, so `sshd` makes a new host key each
time and an SSH client warns about a changed key. Use a disk file to keep
it.

**The browser build.** `sh web/build.sh` rebuilds `web/setun.wasm` with the
BIOS and a Trinux disk inside, and `python -m http.server 8026 --directory
web` serves it at `http://localhost:8026`. The page boots Trinux with 3^7
pages of RAM. **Window system** starts `trwm term clock colors paint`.
Power-cycling keeps the disk; **Fresh disk** restores it. The web build has
no network: the card reports no link.

### 2.2 Booting

The BIOS loads the loader from sector 0. The loader reads the kernel from
the following sectors, and the kernel prints its boot log on the screen and
the serial port: memory, traps, the page allocator's self-test, the timer
(100 Hz), the root file system, and the random-number source. It then
mounts:

| Mount point | File system |
|---|---|
| `/` | the tfs volume on disk 0 (or, on a disk without one, a ramfs unpacked from the initramfs) |
| `/dev` | devfs |
| `/proc` | procfs |
| `/tmp` | tmpfs (a ramfs), mode 01777 |

`/sbin/init` starts `/bin/login` on the console. If `eth0` is up (a network
is attached), it also starts `/sbin/telnetd`, `/sbin/sshd` and
`/sbin/httpd`, with their output in `/var/log/telnetd.log`,
`/var/log/sshd.log` and `/var/log/httpd.log`. When the login session ends,
init starts `login` again and prints `init: login session exited with N`.
If `login` fails to run (status 127) three times in a row, init starts a
root shell instead, so a damaged disk can be repaired. Init also reaps
orphaned processes.

### 2.3 Logging in

The disk comes with two accounts:

| User | Password | uid | Home | Groups |
|---|---|---|---|---|
| `root` | `root` | 0 | `/root` (mode 0700) | `root`, `wheel` |
| `user` | `user` | 1000 | `/home/user` | `user`, `wheel` |

`login` asks for a name and a password (the password with the echo off) and
allows three tries. After a successful login it:

- on the console only, gives `/dev/console`, `/dev/fb0`, `/dev/mouse` and
  `/dev/kbd` to the user, so that a normal user can run the window system;
- sets the supplementary groups, the group and the user;
- changes to the home directory (or `/`, with a message, if it is missing);
- prints `/etc/motd`;
- starts the shell as a login shell (`-sh`) with `HOME`, `USER`, `LOGNAME`,
  `SHELL`, `PATH` and `TERM` set. `PATH` is `/bin:/sbin` for root and
  `/bin` for other users; `TERM` is inherited, or `linux`.

### 2.4 A first session

```text
login: user
Password:
Welcome to Trinux on Setun-11̅.
Everything here is a power of 3.
$ id
uid=1000(user) gid=1000(user) groups=1000(user),10(wheel)
$ ls -l /etc
$ cat /etc/pangrams
$ echo 'hello' > note.txt; wc note.txt
$ ps
$ trwm term clock            # the window system; [Exit] on the panel returns
$ calc '2^0.5'
$ exit
```

The default prompt is `$ `. Set `PS1` for another one, for example
`PS1='\u@\h:\w\$ '` (§3.9).

### 2.5 Shutting down

`reboot`, `poweroff` and `halt` (in `/sbin`, root only) flush the buffer
cache, mark the volume clean, and restart or stop the machine. A normal user
gets `EPERM`. `sync` writes dirty buffers at any time; the kernel also
writes them from its idle loop once 5 seconds have passed with dirty
buffers. A volume that was not cleanly unmounted is reported at the next
boot.

---

## 3. The shell

`/bin/sh` ([`user/bin/sh.tri`](../user/bin/sh.tri)) is a POSIX-style shell.
It tokenizes, parses into a syntax tree, then runs the tree.

```text
sh                       interactive when standard input is a terminal
sh -c 'commands' [name [args]]
sh script [args]
```

A non-interactive shell reads all of its input (a script, `-c` text, or
standard input) before running it. The text may be at most 19 682
characters. The shell reads no start-up files.

### 3.1 Commands and lists

- **Simple commands** with prefix assignments: `VAR=x cmd` sets `VAR` in
  the environment of that command only.
- **Pipelines**: `a | b | c`, and `! pipeline` to negate the status.
- **Lists**: `;`, newline, `&` (background), `&&`, `||`.
- **Compound commands**:
  - `if list; then list; [elif list; then list;] [else list;] fi`
  - `while list; do list; done`, `until list; do list; done`
  - `for name [in words]; do list; done` (without `in`, the positional
    parameters)
  - `case word in pattern [| pattern]) list ;; ... esac`
  - `( list )` runs in a subshell, `{ list; }` in the current shell.
- **Functions**: `name() compound-command [redirections]`. A call gets its
  own `$1 … $#` (`$0` stays). `return [n]` ends it. Prefix assignments
  (`V=x f`) last for the call only. `break` and `continue` do not reach
  loops outside the function. Calls nest at most 81 deep.
- **Comments** start with `#`.

Names are looked up in this order: special builtins, functions, other
builtins, then `PATH`. A name containing `/` is run as given. A command
that is not found gives status 127; one that is not executable gives 126.

### 3.2 Quoting

| Form | Meaning |
|---|---|
| `'…'` | everything literal |
| `"…"` | literal except `$`, `` ` `` and `\` |
| `\c` | `c` literal |

### 3.3 Parameters and expansions

| Expansion | Value |
|---|---|
| `$name`, `${name}` | the variable |
| `${name:-w}`, `${name-w}` | `w` if unset or empty (`:`), or unset |
| `${name:+w}` | `w` if set and not empty |
| `${name:=w}` | assign `w` if unset or empty, then expand |
| `${name:?w}` | error with `w` if unset or empty |
| `${#name}` | the length |
| `$?` | status of the last command |
| `$$` | the shell's pid |
| `$!` | pid of the last background job |
| `$#`, `$@`, `$*` | count, and the positional parameters (`"$@"` keeps the words) |
| `$-` | the shell's options |
| `$0` … `$9` | the script name and positional parameters |
| `$(…)` | command output, trailing newlines removed; nests |
| `` `…` `` | the same; inside it a backslash is kept unless it comes before `$`, `` ` `` or `\` (or `"` within double quotes) |
| `$((…))` | arithmetic: `+ - * / %`, unary `- + !`, `< <= > >= == !=`, `&&`, `||`, parentheses, decimal numbers, variables (an empty or unset variable is 0) |
| `~`, `~/…` | `$HOME` |

Unquoted results of expansions are split into fields on blanks. There is
no `IFS`. Unquoted words are then globbed: `*`, `?`, `[...]` and `[!...]`,
with sorted results. A pattern that matches nothing stays as it is.

An expansion error (division by zero, `${x:?}`, a bad substitution)
abandons the whole command line, and makes a non-interactive shell exit.

### 3.4 Redirections

Redirections work on any command, compound ones included (`while read l;
do …; done < file`).

| Form | Meaning |
|---|---|
| `[n]< file` | input from file |
| `[n]> file` | output to file (truncated) |
| `[n]>> file` | append |
| `n<&m`, `n>&m` | duplicate descriptor `m` |
| `n>&-` | close `n` |
| `<<word`, `<<-word` | here-document; `<<-` removes leading tabs |

In a here-document with an unquoted delimiter, the body gets `$`
expansions; any quoting in the delimiter makes the body literal. Several
here-documents on one line are read in order. A body is at most 3^9
trytes. A here-document whose operator is inside a function body must have
its body before the function's closing `}` line.

`exec` with only redirections makes them permanent.

### 3.5 Variables and the environment

The shell imports the environment at start-up. `PATH` defaults to
`/bin:/sbin` when it is not set. `export name[=value]` marks a variable for
the environment of commands; `unset name` removes it (`unset -f name`
removes a function). `local name[=value]` inside a function saves the
variable and puts it back, value and export flag, when the function returns.
Scope is dynamic, as in dash. `export` and `local` are declaration
utilities, so `local s=$1` is not split or globbed.

### 3.6 Builtins

| Builtin | Notes |
|---|---|
| `:`, `true`, `false` | |
| `echo [-n] [args]` | |
| `cd [dir]` | no argument: `$HOME`; `cd -`: `$OLDPWD`; sets `OLDPWD` |
| `pwd` | |
| `exit [n]` | |
| `export`, `unset`, `local` | §3.5 |
| `set [-e] [-x] [+e] [+x] [-- args]` | `-e` exit on error, `-x` trace; without arguments, list the variables |
| `shift [n]` | |
| `read [-r] [name ...]` | reads one character at a time, so the rest of the input stays for the next reader; without names, `REPLY` |
| `test expr`, `[ expr ]` | §16.3 (`testexpr.tri`) |
| `break [n]`, `continue [n]` | |
| `eval [args]` | joins the arguments with spaces and runs the result |
| `. file` | runs a file in the current shell; a name without `/` is looked up in `PATH`; `return` ends the file |
| `return [n]` | |
| `jobs`, `fg [%job]`, `bg [%job]` | §3.7 |
| `kill [-s sig \| -sig \| -l] pid \| %job` | |
| `wait [pid \| %job]` | without an argument, waits for all children |
| `exec [command]` | |
| `umask [mode]` | |
| `type name ...` | reports builtins, special builtins, functions, aliases and paths |
| `alias [name[=value]]`, `unalias name \| -a` | §3.8 |
| `history` | lists the history |

The special builtins (found before functions) are `: break continue eval
exec exit export return set shift unset .`.

### 3.7 Job control

An interactive shell puts each pipeline in its own process group and gives
the terminal to the foreground job.

- `&` starts a background job; `$!` holds its last pid.
- `^Z` stops the foreground job; `bg` continues it in the background, `fg`
  in the foreground.
- `jobs` lists the jobs; `%n` names job `n` in `fg`, `bg`, `kill` and
  `wait`.
- "Done", "Stopped", "Terminated" and "Killed" notices appear before the
  next prompt.
- `^C` at the prompt drops the line and prints a fresh prompt.
- The shell ignores `SIGQUIT`, `SIGTSTP`, `SIGTTIN` and `SIGTTOU`, and
  restores the defaults in its children.
- In a non-interactive shell, `&` jobs ignore `SIGINT` and `SIGQUIT`.

A command killed by a signal sets `$?` to 128 + the signal number (130
for `^C`).

### 3.8 Aliases

`alias name=value` defines an alias, and `alias` alone prints all of them
as `alias name='value'`. An unquoted command word that names an alias is
replaced when the command runs. If the value ends in a blank, the next word
is checked too. An alias is never expanded inside its own expansion, so
`alias ls='ls -l'` works. `\ls` or `'ls'` skips the alias. Unlike bash, an
alias defined earlier on the same line already applies, and aliases work in
scripts.

### 3.9 Prompts

`PS1` (default `$ `) and `PS2` (default `> `, for continued lines) are
worked out each time they are shown, as in bash:

1. backslash escapes: `\u` (`$USER`), `\h` (the host name up to its first
   dot), `\w` (the working directory, `~` for home), `\W` (its last part),
   `\$` (`#` for root, else `$`), `\n`, `\e`, `\a`, `\\`; `\[` and `\]` are
   dropped;
2. parameter, command and arithmetic expansion, as in double quotes.

So `PS1='\u:\w\$ '` or `PS1='$(pwd)> '` follow `cd`. The prompt's commands
leave `$?` alone.

### 3.10 Line editing, history and completion

At a terminal the shell reads each line itself, with the terminal in raw
mode (`ISIG` stays on, so `^C` still interrupts).

| Key | Action |
|---|---|
| Left, Right | move one character |
| Home, `^A` / End, `^E` | start / end of the line |
| Backspace | delete before the cursor |
| Delete | delete under the cursor |
| `^D` | on an empty line, end of input; otherwise delete under the cursor |
| `^K` | delete to the end of the line |
| `^U` | delete to the start of the line |
| `^W` | delete the word before the cursor |
| Up, Down | step through the history |
| Tab | complete a command (builtins, functions, aliases, `PATH`) or a file name; a second Tab lists the choices |
| Enter | run the line |

The history keeps the last 81 lines, without repeats in a row. A 3-tryte
character (§11) is inserted, deleted and stepped over whole, and the editor
counts screen columns with `tchar_width`, so East Asian wide characters
take two.

### 3.11 Errors

- A syntax error prints bash's message, for example ``syntax error near
  unexpected token `fi'``, and sets status 2.
- `set -e` makes a failing command end a script.
- See §3.3 for expansion errors and §3.1 for 126 and 127.

---

## 4. Command reference

Options are listed as each program parses them. Commands in `/sbin` are for
root unless noted. Several programs are one file under many names (hard
links): `misc` (busybox style), `tools`, `useradm`, `reboot`, and
`badclient`. Each picks its behaviour from `argv[0]`.

### 4.1 Files and directories

| Command | Synopsis | Description |
|---|---|---|
| `ls` | `ls [-acltur] [path ...]` | list directories, sorted by name. `-a` shows dot files; `-l` the long form with owner and group names, `s`/`t` (or `S`/`T`) for the set-ID and sticky bits, and the modification time (`Oct  8 13:30` within six months, `Oct  8  2024` otherwise); `-t` sorts by that time, newest first; `-u` uses the access time, `-c` the change time; `-r` reverses the order |
| `cat` | `cat [file ...]` | copy files, or standard input (also for `-`), to standard output |
| `cp` | `cp source target`, `cp source ... dir` | copy files |
| `mv` | `mv source target` | rename within one file system |
| `ln` | `ln source target` | make a hard link (tfs only; there are no symbolic links) |
| `rm` | `rm [-d] file ...` | remove files; `-d` also empty directories |
| `mkdir` | `mkdir [-p] dir ...` | make directories; `-p` makes missing parents |
| `rmdir` | `rmdir dir ...` | remove empty directories |
| `touch` | `touch [-acm] [-r file \| -d date \| -t [[CC]YY]MMDDhhmm[.SS]] file ...` | set the access and modification times (now, or from the reference file, a date, or a stamp), making missing files (not with `-c`); `-a` or `-m` sets only that time. A date is `@SECONDS[.FRACTION]` or `YYYY-MM-DD[ HH:MM[:SS]]`, UTC |
| `chmod` | `chmod mode file ...` | octal mode, or `[ugoa][+-=][rwx]` |
| `chown` | `chown owner[:group] file ...` | change owner (and group); names or numbers |
| `chgrp` | `chgrp group file ...` | change group |
| `stat` | `stat [-c format] file ...` | a file's inode: size, blocks, inode, links, mode, owner, and the access, modification and change times; `-c` with `%n %s %b %i %h %u %g %U %G %a %A %F %X %Y %Z %x %y %z %%` |
| `time` | `time command [args]` | run the command, then `real 1.234` (seconds, the monotonic clock) on stderr; its exit status (127 if it cannot run). No user and sys times: Trinux does not keep them |
| `basename` | `basename path [suffix]` | |
| `dirname` | `dirname path` | |
| `find` | `find [path ...] [expression]` | tests `-name pattern`, `-type c`, `-newer file`, `-mtime [+-]n` (whole days), `-mmin [+-]n` (minutes), `-maxdepth n`, `-mindepth n`, `!`; actions `-print`, `-exec cmd {} ;` |
| `df` | `df [path ...]` | free space in blocks of 3^9 trytes; default `/` and `/tmp` |
| `sync` | `sync` | write every dirty buffer to disk |
| `mount` | `mount`, `mount -t type source dir` | list mounts (`/proc/mounts`), or mount `ramfs`/`tmpfs` or `proc` (root) |
| `umount` | `umount dir` | unmount (root) |
| `md5sum` | `md5sum [file ...]` | MD5 digests; text is hashed as UTF-8, so digests match other systems |
| `od` | `od [-c\|-d\|-o\|-x\|-t\|-3] [file]` | dump trytes: octal (default), decimal, hex, characters, `-t` balanced base 27 (three digits per tryte, negative digits overlined), `-3` balanced base 3 (nine trits per tryte) |

### 4.2 Text

| Command | Synopsis | Description |
|---|---|---|
| `echo` | `echo [-n] [string ...]` | |
| `head` | `head [-n lines] [file ...]` | first lines (default 10) |
| `tail` | `tail [-n lines] [file]` | last lines (default 10) |
| `wc` | `wc [-lwc] [file ...]` | lines, words, characters |
| `grep` | `grep [-ivcnq] pattern [file ...]` | basic regular expressions: literals, `. * ^ $ [abc] [^a-z]` and `\` escapes. `-i` ignore case, `-v` invert, `-c` count, `-n` line numbers, `-q` quiet. Status 0 matched, 1 none, 2 error |
| `sort` | `sort [-rnu] [file ...]` | `-r` reverse, `-n` numeric, `-u` unique |
| `uniq` | `uniq [-c] [file]` | drop repeated adjacent lines; `-c` counts |
| `sed` | `sed [-n] [-i] [-e script] ... [script] [file ...]` | addresses `N`, `$`, `/re/`, ranges and `!`; commands `s` (flags `g p N`, `&`, `\1`–`\9`), `y d p q = n a i c`; `-i` edits in place |
| `tr` | `tr [-cds] set1 [set2]` | ranges and `[:class:]`; works on any code points (`tr а-я А-Я`) |
| `cut` | `cut -c list \| -f list [-d c] [-s] [file ...]` | |
| `diff` | `diff [-q] file1 file2` | normal format from the longest common subsequence; status 0 same, 1 different, 2 trouble |
| `xargs` | `xargs [-n max] [-I str] [command [arg ...]]` | status 123, 126, 127 as POSIX |
| `tee` | `tee [-a] [file ...]` | |
| `seq` | `seq [first] last` | |
| `yes` | `yes [word]` | |
| `edit` | `edit [file]` | full-screen editor (below) |
| `calc` | `calc [expression ...]` | calculator on the FPU (below) |

**`edit`** works in the spirit of nano. Arrows, Home, End, PgUp and PgDn
move; Backspace and Delete delete; `^S` saves; `^X` exits (asking whether
to save a changed text); `^K` cuts the line and `^U` pastes it; `^W` finds
(Enter again finds the next match); `^G` goes to a line number. It uses the
terminal size from `TIOCGWINSZ`, shows tabs as spaces to the next multiple
of 8, scrolls sideways for long lines, and moves over and deletes a 3-tryte
character whole.

**`calc`** evaluates its arguments, or reads expressions line by line (with
a prompt on a terminal). Expressions: `+ - * / %`, `^` (right-associative),
unary minus, parentheses, decimal numbers (`1.5e-3`), balanced ternary and
base-27 integers (`0t1T0`, `0zD9`), variables (`x = 2*pi`; `ans` is the
last result), the constants `pi` and `e`, and the functions `sin cos tan
asin acos atan atan2 sinh cosh tanh exp ln log log3 sqrt abs pow hypot floor
ceil round trunc`. Commands: `:t` also shows results in balanced ternary
(`1/3` is `0t0.1`), `:d N` sets the number of decimal digits (1–12), `:q`
quits.

### 4.3 Processes and the environment

| Command | Synopsis | Description |
|---|---|---|
| `ps` | `ps` | processes from `/proc`: PID, PPID, PGID, state, time, command |
| `kill` | `kill [-sig] pid ...` | signal by number or name (`-9`, `-KILL`, `-SIGKILL`); default `SIGTERM` (the shell's builtin also takes `%job`) |
| `sleep` | `sleep seconds` | seconds, with a fraction allowed (`0.25`); the clock ticks every 1/100 s |
| `env` | `env [-i] [NAME=v ...] [cmd [args]]` | run a command with a changed environment, or print it; `-i` starts from an empty one. Status 127 not found, 126 not runnable |
| `printenv` | `printenv [NAME]` | |
| `true`, `false` | | |
| `test`, `[` | `test expr` | §16.3 |

### 4.4 Users and the system

| Command | Synopsis | Description |
|---|---|---|
| `login` | `login`, `login -f user` | §2.3; `-f` (root only) skips the password; must run as root |
| `su` | `su [-] [user] [-c command]` | set-user-ID root; a shell as another user (root by default). Anyone but root gives the target's password. With `-`, a login shell with a fresh environment in the target's home; otherwise `HOME`, `SHELL`, `USER` and `LOGNAME` change |
| `passwd` | `passwd [user]` | set-user-ID root; users change their own password after giving the current one; root sets anyone's |
| `id` | `id [user]` | `uid=1000(user) gid=1000(user) groups=1000(user),10(wheel)`; for the caller also `euid=`/`egid=` when they differ |
| `whoami` | `whoami` | |
| `groups` | `groups [user]` | |
| `useradd` | `useradd [-u uid] [-G group,...] [-d home] [-s shell] [-c comment] name` | `/sbin`; a new user with a private group (same name and number, from 1000 up), a home owned by it, and a locked password |
| `userdel` | `userdel [-r] name` | `/sbin`; removes the user, its group and its memberships; `-r` also the home directory |
| `uname` | `uname [-asnrvm]` | |
| `date` | `date [-u] [-d date \| -r file] [+format]` | now, a date (as `touch -d`) or a file's modification time; UTC (`-u` is accepted); default format `%a %b %e %H:%M:%S UTC %Y`; `strftime`'s conversions (§16.2) and `%N`, the nanoseconds |
| `clear` | `clear` | `ESC[H ESC[2J` |
| `reboot`, `poweroff`, `halt` | | `/sbin`; §2.5 |
| `ntpdate` | `ntpdate [-q] [-p port] server` | §4.6 |

### 4.5 The window system

| Command | Synopsis | Description |
|---|---|---|
| `trwm` | `trwm [client ...]` | the display server and window manager (§12); a client name without `/` is taken from `/bin` |
| `term` | | a terminal window running `/bin/sh` |
| `clock` | | the time (UTC), once a second |
| `colors` | | all 3^9 pixel colours, once each |
| `paint` | | draw with the mouse; a colour strip on top |

### 4.6 Networking

| Command | Synopsis | Description |
|---|---|---|
| `ifconfig` | `ifconfig [iface [addr] [netmask m] [gw g] [up\|down]]` | `/sbin`; show the interfaces, or set one (root) |
| `ping` | `ping [-c count] [-s size] [-W timeout] host` | set-user-ID root (raw ICMP), drops root once its socket is open; `size` at most 1472 |
| `nc` | `nc [-lubzv] [-p port] [-w secs] [host] port` | connect, or `-l` listen for one connection; `-u` UDP; `-b` octets as they are (otherwise text goes out as UTF-8 and UTF-8 comes in as characters); `-z` only test the port; `-v` verbose; `-w` connect timeout |
| `netstat` | `netstat [-tualn]` | sockets from `/proc/net/tcp` and `/proc/net/udp`; `-a` all, `-l` listening only; addresses are always numeric |
| `host` | `host [-s server] name` | DNS A records only (`/etc/hosts` is not read). Status 0 found, 1 not, 2 usage |
| `wget` | `wget [-q] [-b] [-S] [-O file] [--max-redirect=n] [--no-check-certificate] [--ca-certificate=file] URL` | HTTP/1.1 and HTTPS (TLS 1.3); follows up to 20 redirects; chunked bodies; `-O -` writes to standard output; `-b` keeps octets; `-S` prints headers. Exit status as GNU wget: 4 network or TLS trouble, 5 certificate not verified, 7 malformed answer, 8 error answer or too many redirects |
| `ssh` | `ssh [-p port] [-l user] [-i key] [-t \| -T] [user@]host [command ...]` | SSH client; `~.` at the start of a line disconnects; exit status is the remote one, or 255 |
| `ssh-keygen` | `ssh-keygen [-f file] [-C comment]`, `ssh-keygen -l -f file` | make an ssh-ed25519 key (default `~/.ssh/id_ed25519`), or show a fingerprint |
| `scp` | `scp [-P port] [-i key] [-q] [-r] source ... target` | copy over SFTP; one side is `[user@]host:path`; several sources need a directory target. Status 0, 1 if any copy failed, 255 if the connection failed |
| `sftp` | `sftp [-P port] [-i key] [-b batchfile] [user@]host` | commands `ls cd pwd get [-r] put [-r] mkdir rmdir rm rename lcd lpwd lls help bye quit exit`; with `-b` the first failure ends the session with status 1 unless the line starts with `-` |
| `ntpdate` | `ntpdate [-q] [-p port] server` | SNTP v4; `-q` only reports; without it, sets the clock (root). Status 0, 1 no usable answer, 2 usage or clock not set |
| `x509` | `x509 [-C cafile] [-n host] [-t seconds] chain.pem`, `x509 -l [-C cafile]` | check a PEM certificate chain as a TLS client would, or list the trust store. Status 0 holds, 1 not, 2 trouble reading |
| `certgen` | `certgen [-c P-256\|P-384] [-d days] [-k] key.pem cert.pem name...` | an EC key (PEM, mode 0600) and a self-signed certificate for the names; default 729 days; `-k` reuses an existing key (SEC1 or PKCS #8) |
| `telnetd` | `telnetd [-p port] [-1]` | `/sbin`; telnet logins (default port 23); `-1` serves one connection |
| `sshd` | `sshd [-p port] [-1]` | `/sbin`; SSH server (default port 22) |
| `httpd` | `httpd [-s [-c cert] [-k key]] [-p port] [-r root] [-t secs] [-1]` | `/sbin`; HTTP/1.1 server, root `/var/www`, port 80 (443 with `-s`), 27 s to send a request (`-t`) |
| `sftp-server` | | `/usr/lib/sftp-server`; run by `sshd` for the `sftp` subsystem |

### 4.7 Development

| Command | Synopsis | Description |
|---|---|---|
| `tric` | `tric [-S] [-v] [-O level] [-o out] file.tri` | the Trine compiler (§14.3); default output `a.out` (`a.s` with `-S`); `-O0` (default) to `-O4`; `-v` prints sizes and heap use |
| `tas` | `tas [-r] [-o out] file.s` | the assembler; output a TEF executable, or with `-r` the bare image |
| `hello` | `hello [args]` | prints its pid, parent and arguments; exit status = argc |

### 4.8 Data formats

The ternary file formats of [FORMATS.md](FORMATS.md), and their adapters
at the binary border. One program, `/bin/media`, under each name.

| Command | Synopsis | Description |
|---|---|---|
| `ttv` | `ttv file` · `ttv -c text out` | print a TTV file in its text notation, or make one from the notation (`null`, `true`, `unknown`, `false`, integers of any size, reals, `"text"`, `x"0a1f"` octets, `[…]` lists, `{key: value}` maps) |
| `wav2tau` | `wav2tau in.wav out.tau` | PCM WAV (8, 16 or 24 bits, 1 to 9 channels) to TAU, one balanced word a sample |
| `tau2wav` | `tau2wav [-b 8\|16\|24] in.tau out.wav` | TAU to PCM WAV (16 bits by default), rounding to the nearest; a WAV→TAU→WAV round trip is exact |
| `tone` | `tone [-r rate] [-a amplitude] freq seconds out.tau` | a sine wave (rate 8000, amplitude 0.5 of full scale) |
| `tauinfo` | `tauinfo file` | rate, channels, frames, length and peak |
| `ppm2tim` | `ppm2tim [-r] in.ppm out.tim` | binary PPM (P6) or PGM (P5), 8 bits a sample, to TIM at 6 trits a sample (v − 128), predicted (`-r`: raw) |
| `tim2ppm` | `tim2ppm in.tim out.ppm` | TIM to PPM (RGB) or PGM (grey); a PPM→TIM→PPM round trip is exact |
| `timinfo` | `timinfo file` | size, channels, depth, and the space prediction saves |
| `timview` | `timview file` | draw a TIM picture, centred on mid-grey, in 640×480 graphics mode; Enter returns to the text console |

### 4.9 Tests and diagnostics

These programs hold scenarios the host's test suites run. Each prints one
line per check.

| Program | Checks |
|---|---|
| `k3test` | fork, execve, wait4, copy-on-write, faults, `ECHILD`, memory accounting, the heap |
| `sigtest` | signal handlers, masks, `SIGPIPE`, `EINTR`, `SA_RESTART`, stop and continue, group kills |
| `fstest [enospc]` | file-system calls and their errors (in `/home/fst`); `enospc` fills the volume |
| `libctest` | tlibc functions and system-call errors |
| `k8test` | framebuffer, input devices, `poll`, ptys |
| `fputest` | lazy FPU switching |
| `vectest` | the vector unit and its lazy switching |
| `mtxtest` | shared memory, futex, the one-trit mutex, `MRG`/`STWM`/`TCP`/`TCN`/`TREV`/`CAS3` |
| `chartest` | 3-tryte characters and UTF-8 at the border |
| `dstest` | packed flags, Kleene vectors, the 3-ary heap, the B-tree |
| `fmttest` | tvarints, TTV and TIM: round trips, known answers, and the malformed inputs each must refuse |
| `nettest` | socket calls and their errors on loopback |
| `cryptotest`, `pktest` | crypto against published vectors |
| `fdcheck` | exits with the number of open descriptors among 3..26 |
| `k2_hello` | prints lines with busy work in between (timer interleaving) |
| `/usr/lib/trwtest/{nocreate,unknown,huge,crash,bigwin}` | window clients that break the protocol on purpose |

---

## 5. Files and file systems

### 5.1 Directory layout

| Path | Contents |
|---|---|
| `/bin` | commands |
| `/sbin` | `init`, `ifconfig`, `telnetd`, `sshd`, `httpd`, `useradd`, `userdel`, `reboot`, `poweroff`, `halt` |
| `/etc` | `passwd`, `group`, `shadow` (0600), `hosts`, `resolv.conf`, `motd`, `pangrams`, `ssl/cert.pem`; `ssh/` once `sshd` has run |
| `/home/user` | `user`'s home |
| `/root` | root's home (0700) |
| `/tmp` | tmpfs, 01777 |
| `/dev`, `/proc` | devfs, procfs |
| `/mnt` | an empty mount point |
| `/var/log` | daemon logs |
| `/var/www` | `httpd`'s files (`index.html`) |
| `/usr/share` | `font8x8`, `unifont` |
| `/usr/lib` | `sftp-server`, `trwtest/`, and `trine/` (the Trine standard library: `ports.tri rt.tri console.tri keymap.tri real.tri math.tri long.tri`, and `long.s`, the runtime `tric` appends for `long`) |
| `/usr/src` | `hello.tri`, `lib/` (tlibc and every library module as source), `bin/tric.tri` and `bin/tric/` (the compiler's own source) |

`mkdisk` makes programs 0755 and other files 0644, owned by root, with
these exceptions: `/etc/shadow` 0600, `/bin/su`, `/bin/passwd` and
`/bin/ping` 04755, `/root` 0700, and `/home/user` owned by `user`.

### 5.2 The VFS

The VFS follows Linux's objects in simplified form
([`kernel/fs/vfs.tri`](../kernel/fs/vfs.tri)).

| Object | As built |
|---|---|
| inode | one table of 243 in-core inodes for all file systems. Types: regular, directory, character device, FIFO, socket. A ramfs inode is its file; a tfs or procfs inode is a cache entry, evicted when nothing holds it |
| file | a table of 243 open files (position, flags), shared after `fork` and `dup` |
| descriptors | 27 per process |
| mounts | up to 9; lookups cross mount points both ways, `..` and `getcwd` included; mounts stack |

Paths may be absolute or relative and may contain `.`, `..` and repeated
slashes. A path is at most 242 characters (`ENAMETOOLONG`). A name is at
most 77 characters on tfs and 26 on ramfs. Names are code points, one per
tryte.

`rename` gives `EXDEV` across file systems, `EINVAL` when moving a
directory into itself, `ENOTEMPTY` for a non-empty target directory, and
`ENOTDIR`/`EISDIR` when the types differ. Removing or unmounting something
in use gives `EBUSY`, and so does removing a process's working directory.

### 5.3 tfs, the on-disk file system

tfs ([`kernel/fs/tfs.tri`](../kernel/fs/tfs.tri), host side
[`crates/trinux/src/tfs.rs`](../crates/trinux/src/tfs.rs)) is ext2-like. A
block is one disk sector, 3^9 trytes (6561 words). On the boot disk the
volume starts at sector 243.

| Block | Contents |
|---|---|
| 0 | superblock: magic `0zB007AB18`, version 1, block and inode counts, the locations of the maps, inode table and data, free counts, root inode (1), state (1 clean, 0 mounted) |
| 1 | inode map, one tryte per inode |
| 2 … | block map, one tryte per block: 0 free, 1 used, −1 bad |
| then | inode table: 243 inodes of 27 words per block |
| then | data |

- **Inode** (27 words): mode, nlink, uid, gid, size in trytes, atime,
  mtime, ctime, block count, 15 direct pointers, a single-indirect and a
  double-indirect pointer, generation. An indirect block holds 6561
  pointers.
- **Directory**: a file of 81-tryte entries, 243 per block. An entry is
  an inode-number word (0 is a free slot) and a name of up to 77
  characters. Every directory starts with `.` and `..`.
- **Mounting** checks every superblock field against the disk size and the
  layout, and refuses an inconsistent volume. Every block pointer, inode
  number and mode read from disk is range-checked; damage gives `EIO` and a
  `tfs: bad …` message.
- **Hard links** exist on tfs only, up to 19 683 per file (`EMLINK`).
- **The default disk** is 729 sectors: the loader, the kernel (at most 27
  sectors) and a 486-block volume with 729 inodes.

**The buffer cache** ([`kernel/fs/bcache.tri`](../kernel/fs/bcache.tri))
holds 27 one-sector buffers, LRU and write-back. Dirty buffers reach the
disk on eviction, on `sync` and `fsync`, from the idle loop after 5 s, and
on `reboot`, which also marks the volume clean.

### 5.4 ramfs and tmpfs

ramfs keeps a regular file's contents in one block of memory that grows by
powers of 3. A ramfs inode has one name (no hard links: `EPERM`). `/tmp`
and `/dev` are ramfs mounts; `mount -t ramfs` or `-t tmpfs` makes more.
Files in `/tmp` live in RAM, so a large file is better kept on disk.

### 5.5 devfs

| Node | Major, minor | Mode | Device |
|---|---|---|---|
| `/dev/null` | 1, 3 | 0666 | discards writes; reads end of file |
| `/dev/zero` | 1, 5 | 0666 | reads zeros |
| `/dev/random`, `/dev/urandom` | 1, 8 and 1, 9 | 0666 | octets (0..255 per tryte) from the `TRNG` device |
| `/dev/tty` | 5, 0 | 0666 | the console terminal |
| `/dev/console` | 5, 1 | 0600 | the console terminal |
| `/dev/ttyS0` | 4, 64 | 0660 | the console terminal |
| `/dev/ptmx` | 5, 2 | 0666 | each open makes a pseudo-terminal (§8.3) |
| `/dev/pts/N` | 136, N | 0620 | pty slaves, N = 1..9 |
| `/dev/fb0` | 29, 0 | 0660 | the framebuffer (§12.1) |
| `/dev/mouse` | 13, 63 | 0660 | mouse events (§12.1) |
| `/dev/kbd` | 13, 64 | 0660 | raw keyboard events (§12.1) |

`/dev/tty`, `/dev/console` and `/dev/ttyS0` are the same terminal: the
keyboard and text screen, and the serial port. The random device is a
ChaCha20 key stream; with no host seed (tests, the web build) it repeats on
every boot and the boot log says it is not for keys.

### 5.6 procfs

Contents are generated on every read. procfs files cannot be opened for
writing.

| File | Contents |
|---|---|
| `/proc/meminfo` | `MemTotal`, `MemFree`, `Buffers`, `PageSize`, in trytes |
| `/proc/uptime` | seconds since boot, with hundredths, then `0.00` |
| `/proc/mounts` | `source target type rw 0 0` per mount |
| `/proc/<pid>/stat` | Linux field order: pid, (comm), state, ppid, pgrp, session, tty, tpgid, …, utime as field 14, starttime as field 22 |
| `/proc/<pid>/status` | `Name`, `State`, `Pid`, `PPid`, `PGid`, `Sid`, `Uid` and `Gid` (real, effective, saved), `WChan`, `FPU` and `Vector` (`unused`, `live` or `saved`) |
| `/proc/<pid>/cmdline` | the arguments, NUL-separated (at most 80 trytes) |
| `/proc/net/dev`, `arp`, `tcp`, `udp` | Linux's layouts (`tcp` and `udp` add the owner's uid) |
| `/proc/net/stat` | one network counter per line |

### 5.7 Mounting

`mount(2)` takes `ramfs`/`tmpfs` and `proc`. `tfs` gives `EBUSY` (the one
volume is the root) and any other type `ENODEV`. Mounting and unmounting
need root. `/` cannot be unmounted.

### 5.8 Users, groups and permissions

**Credentials.** A process has real, effective and saved user and group
IDs, and up to 9 supplementary groups. `fork` copies them. `execve` of a
set-user-ID (04000) or set-group-ID (02000) file takes the file's owner or
group as the effective ID.

**Ownership.** Every inode has an owner and a group, kept on disk by tfs. A
new file belongs to its creator's effective IDs.

**Checks** use the owner, group (including supplementary groups) or other
bits:

- root passes every check, except that it needs some execute bit to run a
  file;
- every directory walked through needs search (x);
- `open` needs read and/or write by the access mode; `O_TRUNC` needs
  write; a file that the call itself creates opens whatever its mode;
- creating, removing, renaming and linking need write and search on the
  directory. In a sticky directory (01000) only the entry's owner, the
  directory's owner or root may remove or rename an entry;
- `chdir` needs search, `execve` execute;
- `chmod`: owner or root. `chown`: root may give anything; the owner may
  only change the group, to one of its own groups. A change by anyone but
  root clears the set-ID bits;
- `kill`: root, or a real or effective uid equal to the target's real or
  saved uid; `SIGCONT` also reaches any process in the same session;
- `mount`, `umount`, `reboot`, `clock_settime`, raw sockets and ports
  below 1024: root.

**Accounts** ([`user/lib/pwd.tri`](../user/lib/pwd.tri)):

| File | Format |
|---|---|
| `/etc/passwd` | `name:x:uid:gid:gecos:home:shell` |
| `/etc/group` | `name:x:gid:member,member,...` |
| `/etc/shadow` | `name:hash:lastchange:min:max:warn:inactive:expire:` (0600) |

`useradd`, `userdel` and `passwd` rewrite these files through a temporary
file (`<file>+`) and a rename, so a reader never sees half a file
([`user/lib/dbedit.tri`](../user/lib/dbedit.tri)).

**Password hashing** ([`user/lib/crypt.tri`](../user/lib/crypt.tri)) is
modular, in the style of crypt(3). A stored hash reads `$id$salt$hash`, and
the id picks the scheme. The only scheme is `md5`: the hex MD5 of the salt
and the password. It is fast and therefore weak. New salts are 8
characters. An entry of `!` or `*`, or an unknown scheme, matches no
password. An empty password field in the shadow file lets `login` in
without a password; `sshd` never accepts an empty password.

**umask** is 022 at boot, inherited, and applied when files and directories
are created.

---

## 6. Processes

### 6.1 Life cycle

- **fork** is `clone(SIGCHLD)` (system call 220). Any other `clone` flags
  give `EINVAL`. The child gets a copy-on-write copy of the address space,
  the descriptors, signal handlers and mask, credentials, umask, and the
  FPU and vector state. It starts with no pending signals.
- **execve** looks the program up through the VFS, relative to the working
  directory, and needs execute permission on a regular file (`EACCES`
  otherwise). A **script** (a first line `#!interpreter [argument]`) runs
  its interpreter with argv = { interpreter, [argument,] the script's path,
  argv[1..] }, as Linux's `binfmt_script`: spaces around the argument are
  dropped and it stays one argument; the interpreter may itself be a
  script, four deep (`ELOOP` past that); a `#!` with no interpreter or a
  first line past 81 trytes is `ENOEXEC`. Any other file that is not TEF
  gives `ENOEXEC`, and the shell then runs it as a shell script (POSIX).
  On success it resets
  caught signals to `SIG_DFL` (ignored ones stay ignored), drops the FPU
  and vector state, closes the close-on-exec descriptors and records
  `comm` and `cmdline` for procfs. Argument and environment strings
  together are at most 81 strings and 2187 trytes (`E2BIG`).
- **exit** frees the address space at once, reparents the children to
  init and wakes a waiting parent, which also gets `SIGCHLD`. The exit
  status is the low 8 bits of the code.
- **wait4(pid, status, options)** reaps a zombie. `pid` is a child (> 0),
  any child (−1), the caller's group (0) or group −pid. `WNOHANG` returns 0
  at once; `WUNTRACED` reports a stopped child once. The status is Linux's:
  `code·256` for an exit, the signal number for a kill, `stopsig·256 + 127`
  for a stop. With no matching child, `ECHILD`.
- If init exits, the kernel panics: `Kernel panic - not syncing: Attempted
  to kill init!`.

### 6.2 TEF, the executable format

A TEF file is a 6-word header followed by the program text
([`kernel/exec.tri`](../kernel/exec.tri)):

| Word | Field |
|---|---|
| 0 | magic `0zB007AB15` (115 067 810 201) |
| 1 | entry point |
| 2 | first tryte of the text |
| 3 | text size in trytes |
| 4 | first tryte of the bss |
| 5 | bss size in trytes |

`execve` maps the text read-only and executable, and makes the bss and the
heap demand-zero. Text pages are shared: each page read stays in a
243-entry text cache keyed by the file's inode slot, its generation and the
page, so the next run of the same file maps the cached frames. Writing or
truncating a file gives it a new generation, so a changed program never
runs from stale text. When memory runs short, the cache gives its frames
back.

`mkdisk` and `tas` write TEF files whose text starts at `USER_BASE`.

### 6.3 Start-up

At entry `r1` holds `argc`, `r2` `argv` and `r3` `envp`. The stack holds the
strings, then `argv[]`, NULL, `envp[]`, NULL, as on Linux. Trine's user
start-up code stores `envp` in the program's `environ` global when it has
one (tlibc does), calls `main(argc, argv)`, and passes its result to
`exit`. Init starts with `HOME=/` and `TERM=linux`.

### 6.4 Scheduling

- There are 27 task slots. Slot 0 is the idle task, so at most 26 user
  processes exist at once; `fork` then fails with `EAGAIN`.
- The scheduler is round robin without priorities, preemptive on the
  100 Hz timer: every tick switches to the next runnable process. There
  are no nice values.
- The kernel runs with interrupts off and is not preempted. A sleeping
  process waits on a channel and checks its condition again when woken.
- `sched_yield` gives up the processor. `nanosleep` has a resolution of one
  tick (10 ms).
- A process's CPU time (`utime` in `/proc/<pid>/stat`) counts ticks.

### 6.5 Sessions and process groups

Init is pid 1, group 1, session 1; children inherit both.

- `setpgid(pid, pgid)` works on the caller or a child in the same session,
  never on a session leader. `pid` and `pgid` 0 mean the caller.
- `setsid` makes a new session and group, unless the caller already leads
  a group (`EPERM`).
- `getpgid` and `getsid` take a pid, or 0 for the caller.
- `kill(pid, sig)`: `pid` > 0 is one process, 0 the caller's group, −1
  every process but init and the caller, < −1 group −pid. `ESRCH` when
  nothing matches; signal 0 only checks.

### 6.6 Faults

A fault in user mode becomes a signal (§9.4). A fault inside a system call
on a user address goes through the same path, so system calls can touch
demand-zero and copy-on-write memory; one that cannot be resolved kills
the process, and the kernel prints `[pid N] … at EPC, address A - killed by
signal S`.

---

## 7. Memory

### 7.1 Addresses

Addresses are balanced: user space is the negative half. The top 9 trits of
an address index the level-1 page table.

| L1 index | Used for |
|---|---|
| −9841 … −1 | user space, private to each process |
| 0, +1 | the kernel: all of RAM, ROM and the framebuffer, identity-mapped, shared by every process |
| +2 … +9841 | reserved |

Address 0 is in the kernel half, so a user program that follows a null
pointer gets `SIGSEGV`. Pages are 3^9 trytes. A word is 3 trytes and its
address is its middle tryte.

### 7.2 A process's address space

| Region | Address | Notes |
|---|---|---|
| text | from `USER_BASE` = −3^25 (−847 288 609 443) | read-only + execute; at most 27 pages |
| globals (data and bss) | from `USER_DATA` = `USER_BASE` + 27·3^9 | |
| heap | from the end of the bss up to `brk` | demand-zero; `brk` may move up to 729 pages past the start of the bss |
| framebuffer mapping | page −3^15, up to 25 pages | §7.5 |
| shared anonymous memory | from page −3^15 + 243, up to 243 pages | §7.5 |
| stack | top at −3^18 − 3 (−387 420 492), 27 pages | the top page is mapped at `execve`, the rest on demand |

The highest user address is −193 710 245. A system call checks every user
pointer against the user range and fails with `EFAULT` outside it.

### 7.3 Physical memory

Physical frames are 3^9 trytes. The page allocator is a buddy allocator in
powers of 3 (a "triddy" allocator): a block of 3^k frames splits into three.
The kernel image lives in frames 4–30 and its globals in frames 31–57; the
allocator owns frame 58 and up. The kernel keeps up to 27 cleared page
tables for reuse.

### 7.4 Demand paging and copy-on-write

| Fault | Result |
|---|---|
| page not present, in the heap or the stack | a zeroed page is mapped |
| write to a copy-on-write page | the page is copied, or made writable again if it has one sharer left |
| anything else | a signal (§9.4) |

After `fork` every writable private page is read-only in both processes,
with the page-table entry's software trit marking it copy-on-write, and a
reference count per frame. The kernel sets and clears the two trits with
one masked store (`STWM`), keeping the access trit. Programs are not paged
from disk: the text is read at `execve`, and there is no swap.

### 7.5 brk, mmap and futex

- **`brk(addr)`** sets the end of the heap if `addr` is between the start
  of the bss and 729 pages past it, and returns the current end. tlibc's
  `sbrk` and `malloc` are built on it.
- **`mmap` of `/dev/fb0`** (`MAP_SHARED`, offset 0, length up to the
  current mode's size) maps the framebuffer's device frames at page −3^15
  and returns the address. One process may hold `/dev/fb0` at a time. The
  pages are never copied on write, so a forked child shares them.
- **`mmap(0, len, PROT_READ|PROT_WRITE, MAP_SHARED|MAP_ANONYMOUS, -1, 0)`**
  maps fresh zeroed frames (up to 243 pages) at the first free run of pages
  in the shared area. `fork` shares these pages instead of copying them;
  they are reference-counted and freed with their last sharer. Private
  anonymous mappings are not offered: private memory comes from `brk`.
  Any other `mmap` gives `EINVAL` or `ENODEV`.
- **`munmap(addr, len)`** accepts only the framebuffer's address; shared
  anonymous pages stay mapped until `execve` or `exit`.
- **`futex(uaddr, op, val)`** (98): `FUTEX_WAIT` (0) sleeps while the word
  at `uaddr` holds `val` (`EAGAIN` if it does not); `FUTEX_WAKE` (1) wakes
  up to `val` waiters and returns how many. The wait key is the word's
  physical address, so processes sharing the page meet. `EINVAL` for an
  address that is not a multiple of 3 or an unknown operation; `EFAULT`
  outside user space; `EINTR` when a signal ends the wait.

tlibc builds a mutex on these (§16.1).

---

## 8. Terminals

### 8.1 The console

The console is the keyboard and the 80×30 text screen, together with the
serial port. Output goes to both; input comes from both. The screen
understands a VT100 subset: cursor position (`ESC[r;cH`), `ESC[J` and
`ESC[2J`, `ESC[K`, cursor moves (`ESC[nA` to `D`) and colours (`ESC[…m`
with 0, 30–37, 40–47). The serial port passes escape sequences through.
While a graphics mode is on, console output reaches only the serial port.

**Keyboard.** Two layouts, US and Russian ЙЦУКЕН (as on Windows). Alt+Shift
switches layout. Caps Lock flips the case of letters in both. Ctrl+letter
gives the control code of the key's position, so `^C` stays on the same key
in the Russian layout. Right Alt (AltGr) with a digit or letter types it
overlined (§11.1). Arrows, Home, End, Insert, Delete and Page Up/Down send
VT100 sequences (`ESC [ A` …). The console and `trwm` each keep their own
layout.

On the serial console, text is UTF-8: a character followed by U+0305
COMBINING OVERLINE arrives as the overlined character, and an overlined
character goes out that way.

### 8.2 The line discipline

All terminals share one line discipline after Linux's `n_tty`
([`kernel/drivers/tty.tri`](../kernel/drivers/tty.tri)).

- **Canonical mode** (`ICANON`, the default): erase (`VERASE`, DEL, and
  `^H`), kill (`VKILL`, `^U`), end of file (`VEOF`, `^D`), `ICRNL`. A line
  holds at most 242 characters. A read returns at most one line. Erase
  removes a 3-tryte character whole. Echo shows control characters as
  `^X`.
- **Non-canonical mode**: characters pass straight through. A read waits
  for at least one character and returns what is there. `VMIN` and
  `VTIME` are stored but not used.
- **Signals** (`ISIG`): `VINTR` (`^C`), `VQUIT` (`^\`) and `VSUSP` (`^Z`)
  echo as `^X`, flush the input, and send `SIGINT`, `SIGQUIT` or `SIGTSTP`
  to the foreground process group.
- **Job control**: a read from a background group gets `SIGTTIN`, or `EIO`
  if `SIGTTIN` is ignored or blocked. `SIGTTOU` and `TOSTOP` are not
  implemented: background writes are allowed.

**termios** has one word per flag field (`c_iflag`, `c_oflag`, `c_cflag`,
`c_lflag`), then `c_line` and `c_cc[19]` as trytes, with Linux's bit values
and `c_cc` indices. Defaults: `ICRNL`; `OPOST|ONLCR`; `ISIG|ICANON|ECHO|
ECHOE|ECHOK`; `VINTR`=3, `VQUIT`=28, `VERASE`=127, `VKILL`=21, `VEOF`=4,
`VMIN`=1, `VSUSP`=26.

| ioctl | Number | Effect |
|---|---|---|
| `TCGETS` | 21505 | read termios |
| `TCSETS`, `TCSETSW`, `TCSETSF` | 21506–21508 | set termios; `TCSETSF` also flushes input |
| `TIOCGPGRP`, `TIOCSPGRP` | 21519, 21520 | the foreground group; it must exist in the caller's session |
| `TIOCGWINSZ`, `TIOCSWINSZ` | 21523, 21524 | the window size (`ws_row`, `ws_col`, `ws_xpixel`, `ws_ypixel`, one word each): the console is 80 × 30 cells, 640 × 480 pixels; a pty has what `TIOCSWINSZ` set (pixels 0 until then); a new size sends `SIGWINCH` to the foreground group; `EINVAL` for no rows or columns or a negative pixel size |
| `TIOCGPTN` | 2147767344 | pty master: the pty number |
| `TIOCSPTLCK` | 1074025521 | pty master: accepted |

An unknown `ioctl` gives `ENOTTY`, before its argument is checked. The
console reports 80×30; a new pty starts at 80×24.

### 8.3 Pseudo-terminals

Opening `/dev/ptmx` makes a pseudo-terminal pair; `TIOCGPTN` gives its
number N, and the slave appears as `/dev/pts/N`. There are 9 ptys
(`EAGAIN` after that). What the master writes is typed into the slave; the
slave's output, echo included, is read from the master. Closing the master
sends `SIGHUP` and `SIGCONT` to the slave's foreground group, and slave
reads then see end of file. The master reads `EIO` once the slave has
closed. `/dev/pts/N` disappears when both sides are gone. tlibc has
`posix_openpt`, `grantpt`, `unlockpt` and `ptsname_r`.

`term`, `telnetd` and `sshd` run their shells on ptys.

---

## 9. Signals

### 9.1 The signals

Signals 1–31 use Linux's numbers. A signal set is one word with bit
`sig − 1`. There are no real-time signals.

| Number | Name | Default action |
|---|---|---|
| 1 | `SIGHUP` | terminate |
| 2 | `SIGINT` | terminate |
| 3 | `SIGQUIT` | terminate |
| 4 | `SIGILL` | terminate |
| 5 | `SIGTRAP` | terminate |
| 6 | `SIGABRT` | terminate |
| 7 | `SIGBUS` | terminate |
| 8 | `SIGFPE` | terminate |
| 9 | `SIGKILL` | terminate; cannot be caught, blocked or ignored |
| 10 | `SIGUSR1` | terminate |
| 11 | `SIGSEGV` | terminate |
| 12 | `SIGUSR2` | terminate |
| 13 | `SIGPIPE` | terminate |
| 14 | `SIGALRM` | terminate |
| 15 | `SIGTERM` | terminate |
| 17 | `SIGCHLD` | ignore |
| 18 | `SIGCONT` | continue a stopped process; otherwise ignore |
| 19 | `SIGSTOP` | stop; cannot be caught, blocked or ignored |
| 20 | `SIGTSTP` | stop |
| 21 | `SIGTTIN` | stop |
| 22 | `SIGTTOU` | stop |
| 23 | `SIGURG` | ignore |
| 28 | `SIGWINCH` | ignore |
| others up to 31 | | terminate |

There is no core dump. A process killed by a signal has that signal number
as its wait status. A stopped process's parent gets `SIGCHLD` and a
`wait4` wake-up. `SIGCONT` resumes a stopped process and cancels pending
stop signals; a stop signal cancels a pending `SIGCONT`. Nothing in the
real interval timer (`setitimer(ITIMER_REAL)`, and tlibc's `alarm` on it)
sends `SIGALRM`, at a resolution of one tick (1/100 s); a child starts
without one, and `execve` keeps it.

### 9.2 Handlers and masks

- `rt_sigaction(sig, act, oact)` takes `struct sigaction` with one word
  per field: handler (`SIG_DFL` 0, `SIG_IGN` 1, or an address), flags,
  restorer, mask. The flags used are `SA_RESTORER` (67108864) and
  `SA_RESTART` (268435456).
- `rt_sigprocmask(how, set, oset)` with `SIG_BLOCK` 0, `SIG_UNBLOCK` 1,
  `SIG_SETMASK` 2. `SIGKILL` and `SIGSTOP` are removed from any mask.
- `rt_sigpending`, `rt_sigsuspend` as in POSIX.
- An ignored signal is discarded when it is sent, unless it is blocked.
- `fork` copies handlers and the mask; `execve` resets caught signals to
  the default.

### 9.3 Delivery and the signal frame

On every return to user mode the kernel takes the lowest pending signal
that is not blocked. For a handler it pushes a 192-word frame below an
81-tryte red zone under the user stack pointer:

| Words | Contents |
|---|---|
| 0–27 | the trap frame: all registers, `EPC`, `EMODE`, `KSP` |
| 28 | the old blocked mask |
| 29 | magic `0zB007AB17` |
| 30 | 1 if the FPU state follows, with 82 words of it |
| 113 | 1 if the vector state follows, with 78 words of it |

The handler starts with `r1` = the signal number and `lr` = the restorer
(tlibc's `__restore_rt`, which executes `sys 139`). The signal itself and
`sa_mask` are blocked while it runs. The handler starts with a fresh FPU
and vector state. `rt_sigreturn` checks the magic and that the saved mode
is user mode, so a forged frame cannot raise privilege, then restores
everything.

**Interrupted system calls.** A blocking read or write on a pipe or a
terminal, `wait4`, `nanosleep` and `rt_sigsuspend` are restartable. If no
handler runs (for example, the process was stopped and continued), the
call is executed again. With a handler, it fails with `EINTR`, or, for the
pipe, terminal and `wait4` sleeps, restarts if the handler has
`SA_RESTART`. `nanosleep` resumes with the time it had left, and writes the
remainder to `rem` when it fails. `ppoll` and `futex` fail with `EINTR`
straight away.

### 9.4 CPU faults

| Fault (vector) | Signal |
|---|---|
| illegal or privileged instruction (−1, −2) | `SIGILL` |
| page or protection fault (−3, −4) that is not demand-zero or copy-on-write | `SIGSEGV` |
| misaligned access (−5) | `SIGBUS` |
| divide by zero (−6), unmasked FPU exception (−11) | `SIGFPE` |
| breakpoint, single step (−7, −8) | `SIGTRAP` |
| anything else | `SIGSEGV` |

A fault goes to a handler when one is installed and the signal is not
blocked; otherwise the process is killed and the kernel prints the cause.

---

## 10. The FPU and the vector unit

### 10.1 The FPU

The FPU (ISA §14) has 27 real registers and a status register (FSR). Trine
programs use it through the `real` type and `math.tri`. A program sees:

- a fresh state at `execve`: all registers zero and the reset FSR, with
  every exception masked;
- its own registers, preserved across preemption, system calls and other
  processes' FPU use;
- the parent's state after `fork`;
- a fresh state in a signal handler, and the interrupted state back after
  it returns;
- `SIGFPE` for an unmasked FPU exception.

The kernel switches the registers lazily
([`kernel/fpu.tri`](../kernel/fpu.tri)). One task owns the hardware
registers; every other task's state is kept in its task structure (82
words: the FSR, then `f−13`…`f13` as real81). Returning to user mode sets
`MODE.FE` to 1 for the owner and 0 for the others, so another task traps
(vector −12) on its first FPU instruction; the kernel then saves the
owner's registers, loads the current task's and retries the instruction. A
process that never uses the FPU never pays for it.

### 10.2 The vector unit

The vector unit (ISA §15) has registers `v−13`…`v13` of 81 trits (`v0`
reads as zero); a register is 9 tryte lanes or 3 word lanes. Trine has
memory-to-memory builtins for it (`vadd`, `vsub`, `vmin`, `vmax`, `vtmul`,
`vtsat`, `vcons` and their word-lane `w` forms, `vsum`, `vdotw`, `vmrg`,
`vbcst`) on `@align(9)` arrays ([`LANGUAGE.md`](LANGUAGE.md) §8).

[`kernel/vec.tri`](../kernel/vec.tri) switches the registers lazily, as for
the FPU: `MODE.VE` (trit 19) is set only for the owner, vector −13 traps
the first use, and the state is 26 registers (78 words). `fork` copies the
registers, `execve` and `exit` drop them, and a signal handler starts with
zeroes. A misaligned `VLD`/`VST` is `SIGBUS`, a store into read-only memory
`SIGSEGV`, and an invalid vector function `SIGILL`.

`/proc/<pid>/status` shows `FPU:` and `Vector:` as `unused`, `live` (the
owner) or `saved`.

---

## 11. Text

### 11.1 Characters in trytes

A character is one tryte or three ([`user/lib/tlibc.tri`](../user/lib/tlibc.tri),
ISA §8.4):

| Character | Trytes |
|---|---|
| code point `c` ≤ 9841 (U+2671) | `c` |
| the same, overlined (`c` ≥ 33) | `−c` |
| code point `c` > 9841 | the lead `−2`, then two trailing trytes −(33 + n / 9809) and −(33 + n mod 9809), where n = c − 9842 |
| the same, overlined | the lead `−3`, then the same two trytes |

- Trailing trytes lie in −9841…−33, so they are never NUL, a newline, an
  ASCII character or a lead. `strlen`, `strchr` and line splitting keep
  working on trytes.
- `−1` is not a lead because `fgetc` returns −1 for end of file. Leads
  `−1` and `−4`…`−32` are refused, as are surrogates (U+D800–U+DFFF),
  code points above U+10FFFF and overlong forms.
- `strlen` counts trytes. `tstr_next`, `tstr_chars` and `tchar_put` read,
  count and write characters (§16.1).
- An overlined character is the negated code point; the ISA calls this the
  overline. Control characters (≤ 32) are not overlined.
- The encoding is not self-synchronizing; text is decoded forward from a
  known start.

### 11.2 UTF-8 at the border

Text crosses to binary systems as UTF-8 ([`user/lib/socket.tri`](../user/lib/socket.tri)):
`utf8_encode` and `utf8_decode` work a character at a time, and an
overlined character becomes the character followed by U+0305. The decoder
refuses overlong forms, surrogates and anything above U+10FFFF. The serial
console, `telnetd`, `sshd`, `nc`, `wget`, `httpd`, `sftp-server`, `scp` and
`sftp` convert this way. File transfers follow
[`user/lib/octets.tri`](../user/lib/octets.tri): a file whose trytes are all
0..255 travels octet for octet, any other travels as UTF-8, and UTF-8 that
arrives with multi-octet characters is stored as characters.

Data on a socket are octets, one per tryte (0..255).

### 11.3 Terminal widths

`tchar_width(cp)` ([`user/lib/wcwidth.tri`](../user/lib/wcwidth.tri),
generated from Unicode's `EastAsianWidth-18.0.0.txt`) is 2 for East Asian
Wide and Fullwidth characters and 1 for every other character. `term`,
`edit` and the shell's line editor count columns with it.

### 11.4 Fonts

| File | Contents |
|---|---|
| `/usr/share/font8x8` | 353 glyphs of 8×8: code points 0–255, the Cyrillic block U+0400–U+045F as glyphs 256–351, and № as glyph 352; the same font as the text mode |
| `/usr/share/unifont` | GNU Unifont 18.0.01, planes 0 and 1: 512 pages, each a 256-tryte width map (none, 8 or 16 pixels) and its glyphs at 14 bits a tryte; about 2M trytes |

`gfx.tri` draws 8×8 text from `font8x8` and shrinks a Unifont glyph to 8×8
for a character outside it (or draws `?` without the file). It reads
Unifont a glyph at a time on first use and keeps 243 glyphs;
`draw_char16` draws a character 16 rows high in one or two 8-pixel cells.

The text-mode font covers Latin, Greek and the Cyrillic block.

### 11.5 Keyboard layouts

See §8.1. In the window system, `trwm` shows the current layout (`EN` or
`RU`) at the right end of its panel.

---

## 12. The window system

The design is the one Linux uses: a user-space display server, `trwm`, owns
the framebuffer and the input devices. Each client owns one window and
talks to the server over two pipes. The server keeps every window's pixels,
so clients never redraw on exposure.

### 12.1 Kernel devices

**`/dev/fb0`**: one process may hold it (`EBUSY` otherwise).

| ioctl | Number | Argument |
|---|---|---|
| `FBIOGET_VSCREENINFO`, `FBIOPUT_VSCREENINFO` | 17920, 17921 | `{xres, yres, pitch, mode}`; mode −1 text, 0 640×480, 1 800×600 |
| `FBIO_CURSOR` | 17928 | the hardware cursor |

`read`, `write` and `lseek` work over the pixels, and `mmap` maps them
(§7.5). A pixel is one tryte: blue in trits 1–3, green in 4–6, red in 7–9,
each channel −13…13, so 3^9 = 19 683 colours (white 9841, black −9841).
When the owner closes the device or dies, the text console comes back.

**`/dev/mouse`**: six-word events: x, y (absolute pixels), buttons (left +
3·middle + 9·right), wheel steps, dx, dy. **`/dev/kbd`**: one word per
event, +code on press and −code on release (USB HID usage IDs). While
`/dev/kbd` is open the console gets no keys. Each device can be open once
(`EBUSY`); reads return whole events into a word-aligned buffer
(`EINVAL` for a buffer smaller than one event or not aligned);
`O_NONBLOCK` gives `EAGAIN`; queues hold 81 events and drop the
oldest when full.

### 12.2 trwm

```text
trwm [client ...]           e.g. trwm term clock colors paint
```

- The panel at the top has launchers (Terminal, Clock, Colors, Paint),
  `[Exit]`, and the keyboard layout.
- Windows have a frame, a title bar and a close box. Click to raise, drag
  the title bar to move, click the close box to close. Keys go to the
  window in front. A button pressed in a window's content raises it and
  sends it `BUTTON`; while the button is held, that window gets `MOTION`
  events, and the release goes to it as another `BUTTON`.
- Up to 9 windows.
- `trwm` reaps clients, and closes a window when its client exits or is
  killed. A client that breaks the protocol is logged ("client N: bad
  request (…)"), sent `SIGTERM` and cut off. Events go out non-blocking, so
  a slow client cannot stall the server.
- It logs to standard error, for example `trwm: window 2 'Clock' (pid 7)`.
- `[Exit]` ends the session and gives the screen back to the text console.

Drawing collects damage into one rectangle, draws it into a scratch
buffer, and copies that to the mapped framebuffer; the inner loops are
assembly in `lib/gfx.tri`.

### 12.3 The trw protocol

`trwm` starts each client with requests on descriptor 3 and events on
descriptor 4 ([`user/lib/trw.tri`](../user/lib/trw.tri)). A message is a
sequence of words: the op, the number of argument words, then the
arguments. A string is one character per word. A message is at most 2187
words.

| Request | Op | Arguments | Effect |
|---|---|---|---|
| `CREATE` | 1 | `w h title…` | create the window (must come first; at most the screen size) |
| `FILL` | 2 | `x y w h colour` | fill a rectangle |
| `TEXT` | 3 | `x y fg bg scale text…` | 8×8 text, scale 1–8 |
| `PIXELS` | 4 | `x y w h pixels…` | `w·h` pixels |
| `SCROLL` | 5 | `x y w h dy fill` | move a rectangle's pixels by `dy` rows, filling the gap |
| `FLUSH` | 6 | none | show the changes |
| `TITLE` | 7 | `text…` | set the title (at most 40 characters) |
| `SPAWN` | 8 | `path…` | start another client; the path must be absolute |
| `TEXT16` | 9 | `x y fg bg text…` | Unifont text, 16 rows high, 8 pixels a cell |

| Event | Op | Arguments |
|---|---|---|
| `KEY` | 101 | `char` |
| `BUTTON` | 102 | `x y buttons` |
| `MOTION` | 103 | `x y buttons` |
| `CLOSE` | 104 | none |
| `FOCUS` | 105 | `in` |

Coordinates are relative to the window's content.

### 12.4 Clients

| Client | What it does |
|---|---|
| `term` | runs `/bin/sh` on a pty in a new session, so job control and `^C` work; a VT100 subset (CR, LF, BS, TAB, `ESC [ H J K A–D m`) on a 76×24 grid of 8×16 Unifont cells; an East Asian wide character takes two cells; scrolling moves the server's pixels |
| `clock` | the time and date (UTC), redrawn once a second |
| `colors` | every one of the 19 683 colours once: 81 rows × 243 columns, drawn at twice the size |
| `paint` | a colour strip on top; dragging on the canvas paints 3×3 dots |

### 12.5 Writing a client

A client imports `lib/trw.tri` (and usually `lib/gfx.tri` for `rgb`). The
library buffers requests until `trw_flush`, and `trw_wait(&ev, fd,
timeout_ms)` waits for an event, a second descriptor or a timeout. This is
the whole of `clock` ([`user/bin/clock.tri`](../user/bin/clock.tri)):

```
import "../lib/tlibc.tri";
import "../lib/gfx.tri";
import "../lib/trw.tri";
import "../lib/time.tri";

fn main(argc: int, argv: **char) -> int {
    var w: int = 8 * 2 * 8 + 16;
    trw_create(w, 44, "Clock");
    var ev: trw_event;
    loop {
        var t: tm;
        gmtime_r(time(null), &t);
        var hms: [27]char;
        var date: [27]char;
        strftime(&hms[0], 27, "%H:%M:%S", &t);
        strftime(&date[0], 27, "%Y-%m-%d UTC", &t);
        trw_fill(0, 0, w, 44, rgb(-12, -12, -6));
        trw_text(8, 6, rgb(13, 13, 2), rgb(-12, -12, -6), 2, &hms[0]);
        trw_text(16, 30, rgb(6, 6, 13), rgb(-12, -12, -6), 1, &date[0]);
        trw_flush();
        var r: int = trw_wait(&ev, -1, 1000);
        if (r < 0) { return 0; }
        if (r == 1 && ev.type == TRW_CLOSE) { return 0; }
    }
}
```

`trw_wait` returns 1 with an event, 2 when only the extra descriptor is
readable, 0 on timeout and −1 when the server has gone. A client exits
when it gets `CLOSE`.

---

## 13. Networking

The full description is [`NETWORK.md`](NETWORK.md). In short:

- **The card.** `TNIC` carries Ethernet II frames, one octet per tryte, by
  DMA, on IRQ 8. Its MAC address is `52:54:00:12:34:56`.
- **The host side.** `setun-net` is a user-mode network behind the card, as
  QEMU's slirp: the gateway is 10.0.2.2, DNS is 10.0.2.3, and the guest is
  10.0.2.15/24. It forwards host ports (`--net`), and connects TCP and UDP
  out through a NAT. ICMP reaches only the gateway.
- **The kernel stack** ([`kernel/net/`](../kernel/net/)): interfaces `lo`
  (127.0.0.1/8) and `eth0`; Ethernet and an ARP cache of 27 entries; IPv4
  without fragmentation; ICMP echo and port unreachable; UDP; TCP after RFC
  9293 with MSS 1460, a one-page (19 683-octet) window, RFC 6298
  retransmission, a 2-second `TIME-WAIT`; raw ICMP sockets for `ping`.
- **Sockets** are anonymous inodes behind descriptors. `AF_INET` only;
  `SOCK_STREAM`, `SOCK_DGRAM`, and `SOCK_RAW` with `IPPROTO_ICMP` (root).
  27 sockets, one-page buffers, backlog at most 27. `struct sockaddr_in`
  is three words (family, port, address as an integer), 9 trytes.
  `htons` and friends return their argument. A tryte outside 0..255 in
  sent data gives `EINVAL`. A write to a closed connection raises
  `SIGPIPE` and returns `EPIPE`.
- **Interfaces** are set through `ioctl` on a datagram socket:
  `SIOCGIFADDR`/`SIOCSIFADDR`, `SIOCGIFNETMASK`/`SIOCSIFNETMASK`,
  `SIOCGIFFLAGS`/`SIOCSIFFLAGS`, `SIOCGIFHWADDR`, and Trinux's
  `SIOCGIFGW`/`SIOCSIFGW` for the default gateway. At boot, with a link,
  `eth0` gets 10.0.2.15/24 and gateway 10.0.2.2.
- **Names** resolve through `resolve_host`: a dotted quad, `/etc/hosts`,
  `localhost`, then DNS to the server in `/etc/resolv.conf`.
- **The clock.** `clock_settime(CLOCK_REALTIME)` lets root set the clock to
  the second; `ntpdate` uses it.

| Program | Role |
|---|---|
| `ifconfig`, `netstat`, `ping`, `nc`, `host` | §4.6 |
| `telnetd` | RFC 854; a pty with `/bin/login` and a relay process per connection; offers `WILL ECHO` and `WILL SUPPRESS-GO-AHEAD` and drops other options |
| `sshd` | `curve25519-sha256` with strict KEX, `ssh-ed25519` host key (`/etc/ssh/ssh_host_ed25519_key`, made on first start), `chacha20-poly1305@openssh.com`; logins by `ssh-ed25519` key in `~/.ssh/authorized_keys` or by password; one session channel per connection; disconnects after 6 failures |
| `ssh`, `ssh-keygen` | the client; host keys in `~/.ssh/known_hosts` (a new host is added, a changed key ends the connection); a private key is its 32-octet seed in hex (0600), the `.pub` file is OpenSSH's format |
| `scp`, `sftp`, `sftp-server` | SFTP version 3; `-r` trees up to 27 levels and 243 names per directory |
| `wget` | HTTP/1.1, and TLS 1.3 with `TLS_CHACHA20_POLY1305_SHA256`, `TLS_AES_128_GCM_SHA256`, `TLS_AES_256_GCM_SHA384`; X25519, P-256, P-384; certificates checked against `/etc/ssl/cert.pem` (27 roots) |
| `httpd` | GET and HEAD, one request per connection, each in its own process; HTTPS with `-s` (ECDSA key, self-signed certificate made on first use); Common Log Format |
| `ntpdate` | SNTP v4 |

---

## 14. Programming for Trinux

Programs are written in Trine ([`LANGUAGE.md`](LANGUAGE.md)), a C-like
language with balanced integer types (`trit`, `tryte`, `int`), `char`,
`real`, pointers, structs and arrays. There is no C compiler.

### 14.1 Conventions

- A program defines `fn main(argc: int, argv: **char) -> int`. Its result
  is the exit status (`exit` takes the low 8 bits).
- It imports tlibc, `import "lib/tlibc.tri";`, and the other modules it
  uses. Imports are resolved next to the importing file first, then in the
  standard library. On the disk the libraries are in `/usr/src/lib`, so a
  program elsewhere imports them by full path:
  `import "/usr/src/lib/tlibc.tri";`.
- **System calls** use `syscall(n, args...)`. The kernel returns `−errno`
  on failure. tlibc's wrappers set the global `errno` and return −1, as
  POSIX specifies. `mmap` returns `MAP_FAILED` (−1) only for results
  −1…−4095, since user addresses are negative.
- **Strings** are NUL-terminated arrays of trytes (`*char`), one code point
  per tryte (three for characters above U+2671).
- **No printf.** Trine has no variadic functions. Output uses typed
  writers: `puts(s)` (which does not add a newline), `put_int(v)`,
  `fputs(fd, s)`, and with `stdio.tri` `fputs_f`, `fput_int` and
  `fput_padded`. `perror2(prog, what)` prints `prog: what: strerror(errno)`.
- **Bit operations.** Trine's `&`, `|` and `~` are tritwise min, max and
  negation, not binary AND, OR and NOT. Flag words therefore use
  arithmetic (`flags / BIT % 2`), or `bin32.tri` for data from binary
  systems.
- **Truth values.** Comparisons return an `int` (`TRUE`/`FALSE`). A
  `trit` cannot be used as a condition; use `sign (t) { neg: … zero: …
  pos: … }`.
- **Structures for the kernel** have one word per field.

### 14.2 A worked example

`count.tri` counts the lines and trytes on standard input:

```
// count: lines and trytes on standard input.
import "/usr/src/lib/tlibc.tri";

fn main(argc: int, argv: **char) -> int {
    var buf: [729]char;
    var lines: int = 0;
    var total: int = 0;
    loop {
        var n: int = read(STDIN_FILENO, &buf[0], 729);
        if (n < 0) { perror2("count", "read"); return 1; }
        if (n == 0) { break; }
        var i: int = 0;
        while (i < n) {
            if (buf[i] == '\n') { lines += 1; }
            i += 1;
        }
        total += n;
    }
    put_int(lines);
    puts(" lines, ");
    put_int(total);
    puts(" trytes\n");
    return 0;
}
```

Write it with `edit count.tri`, then build and run it on Trinux:

```text
$ tric -o count count.tri
$ ./count < /etc/motd
```

### 14.3 Building on Trinux: `tric` and `tas`

`tric` ([`SELFHOST.md`](SELFHOST.md)) compiles a `.tri` file to assembly,
then runs `/bin/tas` to make a TEF executable. Its output matches the host
compiler's, character for character, at the same optimization level.

```text
tric [-S] [-v] [-O level] [-o OUT] FILE.tri
tas [-r] [-o OUT] FILE.s
```

| Option | Meaning |
|---|---|
| `-o OUT` | output file (default `a.out`, or `a.s` with `-S`) |
| `-S` | write the assembly and stop |
| `-O0` … `-O4` | optimization level; `-O0` is the default. Level 1: the peephole pass and jump shapes; 2: register locals and direct operands; 3: operands held in registers and arguments passed straight in registers; 4: inlining of small functions and removal of unreachable functions (the host's default build) |
| `-v` | report the program's size and the memory the compile took |

Programs built with `-O4` run about 5 times fewer instructions than `-O0`.
The assembly of a large program goes next to the output (`OUT.tmp.s`), not
in `/tmp`, and is removed afterwards. A compile error gives the host's
message with line and column, exit status 1, and no output file. `tas`
writes one segment at the user origin with a TEF header, or the bare image
with `-r`.

`tric` rebuilds itself:

```text
$ tric -o /root/tric /usr/src/bin/tric.tri
```

### 14.4 Building on the host

`trine build --user FILE.tri` produces a Trinux program's flat image;
`mkdisk` adds the TEF header. To put a program on the default disk, add it
to `default_files()` in [`crates/trinux/src/lib.rs`](../crates/trinux/src/lib.rs)
and run `mkdisk` again. `mkdisk` compiles every `.tri` source it is given,
copies other files as text (one code point per tryte; a character followed
by U+0305 is stored overlined), and makes a hard link when one source
appears under several names.

`trine build --user -S FILE.s FILE.tri` writes the host compiler's
assembly. `tas` on Trinux turns that assembly into the same executable
`mkdisk` would make, so a `.s` file copied in (for example with `scp`) can
be assembled there.

### 14.5 Using the environment

- `getenv`, `setenv`, `unsetenv` (`stdlib.tri`); `environ` holds the
  environment.
- `execvp` searches `$PATH` (default `/bin:/sbin`).
- `isatty`, `tcgetattr`, `tcsetattr` for terminals; `TIOCGWINSZ` through
  `ioctl` for the size.
- `opendir`/`readdir` read one directory at a time.

---

## 15. System call reference

A system call is the instruction `SYS n`, with arguments in `r1`–`r6` and
the result in `r1`. A failing call returns `−errno`. An unknown number
gives `ENOSYS`. Every user pointer is checked (`EFAULT`), and every
descriptor (`EBADF`). Structures have one word per field. Time values are
`{seconds, nanoseconds}`, one word each.

### 15.1 Files and descriptors

| Nr | Name | Arguments | Returns | Main errors |
|---|---|---|---|---|
| 17 | `getcwd` | `buf, size` | length + 1 | `ERANGE` |
| 23 | `dup` | `fd` | new fd | `EMFILE` |
| 24 | `dup3` | `fd, newfd, flags` | `newfd` | `EINVAL` (same fd, out of range); `O_CLOEXEC` |
| 25 | `fcntl` | `fd, cmd, arg` | depends | `F_DUPFD`, `F_DUPFD_CLOEXEC` (1030), `F_GETFD`/`F_SETFD` (`FD_CLOEXEC`), `F_GETFL`/`F_SETFL` (`O_APPEND`, `O_NONBLOCK`); `EINVAL` |
| 29 | `ioctl` | `fd, cmd, arg` | 0 | `ENOTTY` (§8.2, §12.1, §13) |
| 34 | `mkdirat` | `dirfd, path, mode` | 0 | `EEXIST`, `ENOSPC`, `ENAMETOOLONG`, `EACCES` |
| 35 | `unlinkat` | `dirfd, path, flags` | 0 | `AT_REMOVEDIR` (512); `ENOTEMPTY`, `EBUSY`, `EISDIR`, `ENOTDIR` |
| 37 | `linkat` | `olddirfd, old, newdirfd, new` | 0 | `EPERM` (directory, ramfs), `EXDEV`, `EEXIST`, `EMLINK` |
| 38 | `renameat` | `olddirfd, old, newdirfd, new` | 0 | `EXDEV`, `EINVAL`, `ENOTEMPTY`, `EISDIR`, `ENOTDIR` |
| 39 | `umount2` | `target, flags` | 0 | `EPERM`, `EBUSY` |
| 40 | `mount` | `source, target, type, flags, data` | 0 | `EPERM`, `ENODEV`, `EBUSY` (tfs) |
| 43 | `statfs` | `path, buf` | 0 | one word per field; `f_type` 29798 for tfs |
| 49 | `chdir` | `path` | 0 | `ENOTDIR`, `EACCES` |
| 53 | `fchmodat` | `dirfd, path, mode` | 0 | `EPERM` (not owner, procfs) |
| 54 | `fchownat` | `dirfd, path, uid, gid, flags` | 0 | `EPERM` |
| 55 | `fchown` | `fd, uid, gid` | 0 | `EPERM` |
| 56 | `openat` | `dirfd, path, flags, mode` | fd | flags `O_WRONLY` 1, `O_RDWR` 2, `O_CREAT` 64, `O_EXCL` 128, `O_TRUNC` 512, `O_APPEND` 1024, `O_NONBLOCK` 2048, `O_DIRECTORY` 65536, `O_CLOEXEC` 524288; `ENOENT`, `EEXIST`, `EISDIR`, `ENOTDIR`, `EACCES`, `EMFILE`, `ENFILE`, `EBUSY` (devices) |
| 57 | `close` | `fd` | 0 | |
| 59 | `pipe2` | `fds, flags` | 0 | `O_CLOEXEC`; `EMFILE`, `ENFILE` |
| 61 | `getdents64` | `fd, buf, size` | trytes used | `ENOTDIR`; `EINVAL` (buffer not word-aligned or too small) |
| 62 | `lseek` | `fd, offset, whence` | new offset | `ESPIPE` (pipes, sockets, terminals), `EINVAL` |
| 63 | `read` | `fd, buf, n` | trytes read | `EAGAIN`, `EINTR`, `EIO` |
| 64 | `write` | `fd, buf, n` | trytes written | `EPIPE`, `ENOSPC`, `EAGAIN` |
| 67 | `pread64` | `fd, buf, n, offset` | trytes read | at `offset`, the file position unchanged; `ESPIPE` (pipes, sockets, terminals), `EINVAL` (offset < 0) |
| 68 | `pwrite64` | `fd, buf, n, offset` | trytes written | as `pread64` |
| 73 | `ppoll` | `fds, nfds, timeout, sigmask` | ready count | timeout null = forever; at most 81 fds; `EINVAL`, `EINTR`; `POLLIN` 1, `POLLOUT` 4, `POLLERR` 8, `POLLHUP` 16, `POLLNVAL` 32; `sigmask` ignored |
| 79 | `newfstatat` | `dirfd, path, buf, flags` | 0 | `flags` ignored (no symbolic links) |
| 80 | `fstat` | `fd, buf` | 0 | |
| 81 | `sync` | none | 0 | |
| 88 | `utimensat` | `dirfd, path, times, flags` | 0 | `times` two timespecs (access, modification) or null (both now); `tv_nsec` `UTIME_NOW` (1073741823) or `UTIME_OMIT` (1073741822); a null path means the file open as `dirfd` (`futimens`); now needs ownership or write permission, other times ownership (`EPERM`); kept in whole seconds; `EINVAL`, `EFAULT`, `EPERM` (procfs, devices) |
| 82 | `fsync` | `fd` | 0 | `EIO` |

`struct stat` is: dev, ino, mode, nlink, uid, gid, rdev, size, blksize,
blocks, atime, mtime, ctime (one word each; the three times are all the
modification time). A `linux_dirent64` record is `d_ino`, `d_off`,
`d_reclen` (words), `d_type` (tryte 9), then the NUL-terminated name,
padded to whole words. The `at` calls accept `AT_FDCWD` (−100); `linkat`
and `renameat` accept a relative path only with `AT_FDCWD` (`EINVAL`).

### 15.2 Processes, memory and time

| Nr | Name | Arguments | Returns | Main errors |
|---|---|---|---|---|
| 93 | `exit` | `code` | does not return | |
| 94 | `exit_group` | `code` | does not return | the same as `exit` |
| 98 | `futex` | `uaddr, op, val` | 0, or woken count | `EAGAIN`, `EINVAL`, `EFAULT`, `EINTR` |
| 101 | `nanosleep` | `req, rem` | 0 | `EINVAL`, `EINTR` |
| 102 | `getitimer` | `which, cur` | 0 | `which` must be `ITIMER_REAL` (0); `struct itimerval` = interval, value (each `timeval`: seconds, microseconds); `EINVAL`, `EFAULT` |
| 103 | `setitimer` | `which, new, old` | 0 | as `getitimer`; the value rounds up to a tick; `EINVAL` for microseconds past 999999 or a negative time |
| 112 | `clock_settime` | `clock, ts` | 0 | `CLOCK_REALTIME` only; `EPERM`, `EINVAL` |
| 113 | `clock_gettime` | `clock, ts` | 0 | `CLOCK_REALTIME` 0 (the RTC), `CLOCK_MONOTONIC` 1 (ticks); `EINVAL` |
| 124 | `sched_yield` | none | 0 | |
| 142 | `reboot` | `magic1, magic2, cmd` | does not return | Linux magic numbers; restart, power off, halt; `EPERM`, `EINVAL` |
| 160 | `uname` | `buf` | 0 | six fields of 65 characters |
| 166 | `umask` | `mask` | old mask | |
| 172 | `getpid` | none | pid | |
| 173 | `getppid` | none | parent pid | |
| 179 | `sysinfo` | `info` | 0 | Linux field order; `mem_unit` = 1 tryte |
| 214 | `brk` | `addr` | current break | |
| 215 | `munmap` | `addr, len` | 0 | `EINVAL` (only the framebuffer mapping) |
| 220 | `clone` | `SIGCHLD, …` | child pid / 0 | `SIGCHLD`, or `SIGCHLD \| CLONE_VM \| CLONE_VFORK` (vfork, run as a fork); `EINVAL` (other flags), `EAGAIN`, `ENOMEM` |
| 221 | `execve` | `path, argv, envp` | does not return | `ENOENT`, `EACCES`, `ENOEXEC`, `E2BIG`, `EFAULT`, `ENOMEM` |
| 222 | `mmap` | `addr, len, prot, flags, fd, off` | address | §7.5; `EINVAL`, `ENODEV`, `ENOMEM` |
| 260 | `wait4` | `pid, status, options` | pid, or 0 | `WNOHANG` 1, `WUNTRACED` 2; `ECHILD`, `EINTR` |
| 278 | `getrandom` | `buf, n, flags` | n | `GRND_NONBLOCK` 1, `GRND_RANDOM` 2, `GRND_INSECURE` 4; `EINVAL` |

### 15.3 Signals, groups and credentials

| Nr | Name | Arguments | Returns | Main errors |
|---|---|---|---|---|
| 129 | `kill` | `pid, sig` | 0 | `ESRCH`, `EPERM`, `EINVAL` |
| 133 | `rt_sigsuspend` | `mask` | −1 `EINTR` | |
| 134 | `rt_sigaction` | `sig, act, oact` | 0 | `EINVAL` (`SIGKILL`, `SIGSTOP`, range) |
| 135 | `rt_sigprocmask` | `how, set, oset` | 0 | `EINVAL` |
| 136 | `rt_sigpending` | `set` | 0 | |
| 139 | `rt_sigreturn` | none | restores the frame | a bad frame kills with `SIGSEGV` |
| 143 | `setregid` | `rgid, egid` | 0 | `EPERM` |
| 144 | `setgid` | `gid` | 0 | `EPERM` |
| 145 | `setreuid` | `ruid, euid` | 0 | `EPERM` |
| 146 | `setuid` | `uid` | 0 | `EPERM` |
| 147 | `setresuid` | `ruid, euid, suid` | 0 | `EPERM` |
| 148 | `getresuid` | `ruid*, euid*, suid*` | 0 | |
| 149 | `setresgid` | `rgid, egid, sgid` | 0 | `EPERM` |
| 150 | `getresgid` | `rgid*, egid*, sgid*` | 0 | |
| 154 | `setpgid` | `pid, pgid` | 0 | `ESRCH`, `EPERM`, `EINVAL` |
| 155 | `getpgid` | `pid` | pgid | `ESRCH` |
| 156 | `getsid` | `pid` | sid | `ESRCH` |
| 157 | `setsid` | none | sid | `EPERM` |
| 158 | `getgroups` | `size, list` | count | `EINVAL` |
| 159 | `setgroups` | `n, list` | 0 | `EPERM` (not root), `EINVAL` (more than 9) |
| 174 | `getuid` | none | uid | |
| 175 | `geteuid` | none | euid | |
| 176 | `getgid` | none | gid | |
| 177 | `getegid` | none | egid | |

### 15.4 Sockets

| Nr | Name | Arguments | Returns | Main errors |
|---|---|---|---|---|
| 198 | `socket` | `domain, type, proto` | fd | `AF_INET` 2; `SOCK_STREAM` 1, `SOCK_DGRAM` 2, `SOCK_RAW` 3; `SOCK_NONBLOCK`, `SOCK_CLOEXEC`; `EAFNOSUPPORT`, `EPERM` (raw) |
| 200 | `bind` | `fd, addr, len` | 0 | `EADDRINUSE`, `EACCES` (port < 1024), `EINVAL` (length) |
| 201 | `listen` | `fd, backlog` | 0 | backlog clamped to 27 |
| 202 | `accept` | `fd, addr, len*` | fd | `EAGAIN`, `EINTR` |
| 203 | `connect` | `fd, addr, len` | 0 | `ECONNREFUSED`, `ETIMEDOUT`, `EINPROGRESS` |
| 204 | `getsockname` | `fd, addr, len*` | 0 | |
| 205 | `getpeername` | `fd, addr, len*` | 0 | `ENOTCONN` |
| 206 | `sendto` | `fd, buf, n, flags, addr, len` | octets sent | `MSG_DONTWAIT`; `EINVAL` (tryte > 255), `EPIPE` |
| 207 | `recvfrom` | `fd, buf, n, flags, addr, len*` | octets | `MSG_DONTWAIT`, `MSG_PEEK` |
| 208 | `setsockopt` | `fd, level, opt, val, len` | 0 | `SO_REUSEADDR`, `SO_KEEPALIVE`, `TCP_NODELAY` (accepted) |
| 209 | `getsockopt` | `fd, level, opt, val, len*` | 0 | `SO_ERROR`, `SO_TYPE` |
| 210 | `shutdown` | `fd, how` | 0 | `ENOTCONN` |
| 242 | `accept4` | `fd, addr, len*, flags` | fd | |

`read`, `write`, `close`, `fcntl(O_NONBLOCK)`, `fstat` (`S_IFSOCK`) and
`ppoll` also work on sockets. [`NETWORK.md`](NETWORK.md) §4 has the
details.

Linux system calls not listed here (`symlinkat`, `readlinkat`,
`truncate`, threads, the other timers) do not exist and give `ENOSYS`.
(`alarm` and `vfork` are not system calls in Linux's generic table either:
tlibc builds them on `setitimer` and `clone`.)

---

## 16. Library reference

The libraries live in [`user/lib/`](../user/lib/) and, on the disk, in
`/usr/src/lib`. Like C headers, each is imported where needed. `tlibc.tri`
imports the Trine runtime (`rt.tri`), which provides `memset`, `memcpy`,
`strlen`, `strcmp` (it returns a `trit`) and `itoa`.

### 16.1 tlibc: the core (`tlibc.tri`)

**Descriptors and files:** `read`, `write`, `pread`, `pwrite`, `close`, `lseek`, `dup`,
`dup2`, `pipe`, `open(path, flags, mode)`, `creat`, `fcntl`, `ioctl`,
`stat(path, *stat_t)`, `fstat`, `S_ISDIR`/`S_ISREG`/`S_ISCHR`/`S_ISFIFO`,
`mkdir`, `rmdir`, `unlink`, `rename`, `link`, `chmod`, `chown`, `fchown`,
`umask`, `chdir`, `getcwd(buf, size) -> *char`, `sync`, `fsync`,
`statfs`, `mount`, `umount`, `reboot(cmd)` (`RB_AUTOBOOT`,
`RB_POWER_OFF`, `RB_HALT_SYSTEM`).

**Directories:** `opendir(path) -> *DIR`, `readdir(d) -> *dirent`
(`d_ino`, `d_type`, `d_name`), `closedir`, `getdents64`. One directory can
be open at a time.

**Processes:** `fork`, `vfork` (a fork), `execve`, `execv` (with `environ`), `execvp`
(`$PATH`), `_exit`, `exit` (no `atexit`), `waitpid`, `wait`,
`WIFEXITED`, `WEXITSTATUS`, `WIFSIGNALED`, `WTERMSIG`, `WIFSTOPPED`,
`WSTOPSIG`, `getpid`, `getppid`, `sched_yield`, `brk`, `sbrk`,
`getenv`, `sysinfo`, `uname(*utsname)`.

**Groups, sessions and credentials:** `setpgid`, `getpgid`, `getpgrp`,
`getsid`, `setsid`, `tcgetpgrp`, `tcsetpgrp`, `getuid`, `geteuid`,
`getgid`, `getegid`, `setuid`, `setgid`, `setreuid`, `setregid`,
`setresuid`, `setresgid`, `getresuid`, `getresgid`, `seteuid`,
`setegid`, `getgroups`, `setgroups`.

**Signals:** `sigaction(sig, *sigaction_t, *sigaction_t)` (fills in the
restorer), `signal(sig, handler) -> old`, `sigemptyset`, `sigaddset`,
`sigdelset`, `sigismember`, `sigprocmask`, `sigpending`, `sigsuspend`,
`kill`, `raise`, `pause`, `signame(sig)` (without `SIG`),
`strsignal(sig)`.

**Time:** `nanosleep(*timespec, *timespec)`, `sleep(seconds) -> left`,
`clock_gettime`, `clock_settime`, `alarm(seconds) -> left`, `setitimer`,
`getitimer` (`ITIMER_REAL`; `struct itimerval` of two `timeval`s).

**Terminals:** `isatty`, `tcgetattr`, `tcsetattr(fd, TCSANOW, *termios)`,
`posix_openpt`, `grantpt`, `unlockpt`, `ptsname_r`; `struct termios`,
`struct winsize`, `TIOCGWINSZ`.

**Polling and memory:** `poll(*pollfd, n, timeout_ms)` (−1 = forever),
`mmap`, `munmap`, `getrandom`.

**Futex and the mutex:** `futex_wait(p, val)`, `futex_wake(p, n)`,
`mtx_lock(m)`, `mtx_unlock(m)`. The mutex is one word with one trit of
state: 0 free, +1 held, −1 held with waiters. `mtx_lock` tries `cas3(m, 0,
1)`; failing that it swaps in −1 and waits on the futex until the swap finds
0. `mtx_unlock` swaps in 0 and wakes one waiter if the old value was −1.
The word must be in shared memory to be shared between processes.

**Characters:** `tstr_next(s, *cp) -> length` (1 or 3; 0 at the NUL; −1
if malformed), `tstr_chars(s) -> count` (−1 if malformed), `tchar_put(cp,
out) -> length` (0 if no character has that code point), `big_char_ok(cp)`.

**Errors and output:** `errno`, `strerror(e)`, `perror2(prog, what)`,
`fputs(fd, s)`, `puts(s)`, `put_int(v)`, `streq(a, b)`.

The error and signal constants (`EPERM` … `EINPROGRESS`, `SIGHUP` …
`SIGTTOU`) have Linux's values.

### 16.2 Strings, characters, memory, standard I/O, time

| Module | Functions |
|---|---|
| `string.tri` | `strcpy`, `strncpy`, `strcat`, `strncat`, `strncmp`, `memcmp`, `memmove`, `strchr`, `strrchr`, `strstr`, `strspn`, `strcspn` |
| `ctype.tri` | `isdigit`, `isupper`, `islower`, `isalpha`, `isalnum`, `isspace`, `isxdigit`, `isprint`, `ispunct`, `toupper`, `tolower` (ASCII classes; `isprint` also accepts overlined printable ASCII) |
| `stdlib.tri` | `malloc`, `free`, `calloc`, `realloc`, `malloc_usable_size`, `strdup`, `strndup`, `strtol(s, end, base)` (bases 2–36; `ERANGE` clamps at ±(3^27−1)/2), `atoi`, `abs`, `setenv`, `unsetenv`, `qsort(a, n, cmp)` |
| `stdio.tri` | `FILE` streams: `stdin()`, `stdout()`, `stderr()`, `fopen(path, "r"\|"w"\|"a"\|"r+")`, `fdopen`, `fflush`, `fclose`, `fgetc`, `ungetc`, `fgets`, `feof`, `ferror`, `fputc`, `fwrite_chars`, `fputs_f`, `fput_int`, `fput_padded`, `put_padded`; `BUFSIZ` 2187, `EOF` −1 |
| `time.tri` | `time`, `gmtime_r(t, *tm)` and `timegm(*tm)` (any year, 400-year eras), `time_days(y, m, d)`, `is_leap`, `days_in_month`, `strftime(buf, max, fmt, *tm)` with `%Y %C %y %m %d %e %j %H %I %M %S %p %a %A %b %h %B %u %w %s %Z %T %R %D %F %n %t %%` (UTC only), `parse_date(s, *timespec)`. Precise time is a `long`: t / 3^54 seconds (the top word the seconds, the lower 54 trits a base-3 fraction): `LTIME_SECOND`, `ltime_from_ts`, `ltime_to_ts`, `ltime_now(clock)`, `ltime_seconds`, `ltime_text(t, digits, out)`, `ltime_parse(s, end)` |
| `getopt.tri` | `getopt(argc, argv, opts)` with `optarg`, `optind`, `optopt`; clusters, arguments, a leading `:` for missing arguments, `--` |

`malloc` is first fit over a free list in words, grown by `sbrk` 27 pages
at a time. Its result is word-aligned, like C's. `stdout` and `stderr` are
unbuffered; files opened for writing are buffered and flushed by `fflush`
or `fclose`.

### 16.3 Patterns and expressions

| Module | Functions |
|---|---|
| `regex.tri` | `re_compile(p)`, `re_search(p, s, from)`: POSIX basic regular expressions as `sed` uses them: `c`, `.`, `[set]`, `[^set]`, `e*`, `^`, `$`, `\(` `\)` groups (up to 9), `\1`–`\9` |
| `fnmatch.tri` | `fnmatch(pattern, s)`: `* ? [abc] [a-z] [!x]` and `\` escapes (the shell's globbing and `case`, `find -name`) |
| `testexpr.tri` | `test_eval(argc, argv)`: `! e`, `e -a e`, `e -o e`, `( e )`, `-e -f -d -r -w -x -s -c -p FILE`, `-z -n STRING`, `-t FD`, `s = s`, `s != s`, `-eq -ne -lt -le -gt -ge` |

### 16.4 Users and passwords

| Module | Functions |
|---|---|
| `pwd.tri` | `getpwnam`, `getpwuid` (`*passwd`), `getgrnam`, `getgrgid` (`*group`), `getspnam` (`*spwd`), `user_groups`, `initgroups(user, gid)`, `user_name(uid)`, `group_name(gid)` |
| `crypt.tri` | `crypt(key, setting)`, `crypt_verify(key, stored)`, `crypt_new(key)` (a new hash with the default scheme and a fresh salt), `crypt_register(id, f)`, `crypt_default()` |
| `dbedit.tri` | `db_write`, `db_replace(path, name, line, add, mode)`, `db_drop_member(user)` |

### 16.5 Cryptography

Data are octets, one per tryte. Costs are in [`NETWORK.md`](NETWORK.md) §8
and §14.

| Module | Functions |
|---|---|
| `bin32.tri`, `u32.tri` | 32-bit binary arithmetic on the binary instructions: `xor32`, `and32`, `not32`, `add32`, `rotr32`, `rotl32`, `shr32`, `be32`, `le32`, `put_be32`, `put_le32`, `octets_equal`; `bin32_init()` |
| `sha2.tri` | `sha256`, `sha384`, `sha512` and their `_init`/`_update`/`_final`; a generic `hash_init`/`hash_update`/`hash_final`/`hash_octets` by id |
| `md5.tri` | MD5: `md5_init`, `md5_put_chars`, `md5_final`, `md5_digest_hex`, `md5_hex(s, n, out)`; text is hashed as UTF-8 |
| `hmac.tri` | `hmac(id, key, klen, m, mlen, out)`, `hkdf_extract`, `hkdf_expand`, `hkdf_expand_label` (TLS 1.3) |
| `chacha.tri` | `chacha20_block`, `chacha20_xor` (64-bit nonce, as SSH), the `_ietf` forms (RFC 8439), `poly1305`, `aead_seal`, `aead_open` |
| `aes.tri` | `aes_setkey`, `aes_encrypt` (AES-128/256), `gcm_setkey`, `gcm_seal`, `gcm_open` |
| `c25519.tri` | `x25519`, `x25519_base`, `ed25519_keypair`, `ed25519_sign`, `ed25519_verify` (refuses S ≥ L) |
| `bignum.tri` | big unsigned integers in 16-bit limbs: `bn_*`, Montgomery `mont_init`, `mont_mul`, `mont_pow`, `mont_inv`, `mod_add`, `mod_sub` |
| `ecdsa.tri` | P-256 and P-384: `ecdsa_verify`, `ecdsa_sign` (RFC 6979 nonces), `ec_keygen`, `ec_pubkey`, `ecdh` |
| `rsa.tri` | `rsa_verify(scheme, id, n, nlen, e, elen, sig, slen, h)`: PKCS #1 v1.5 and PSS, moduli up to 4096 bits |

### 16.6 Networking and certificates

| Module | Functions |
|---|---|
| `socket.tri` | `socket`, `bind`, `listen`, `accept`, `accept4`, `connect`, `getsockname`, `getpeername`, `sendto`, `recvfrom`, `send`, `recv`, `setsockopt`, `getsockopt`, `shutdown`; `htons`/`ntohs`/`htonl`/`ntohl` (identity); `sockaddr_set`, `inet_aton`, `inet_addr`, `inet_ntoa`; `resolve_host(name, *addr)`, `dns_server()`, `dns_lookup`; `utf8_encode`, `utf8_decode`, `utf8_to_text`, `text_to_utf8`; `struct ifreq` and the `SIOC*` numbers |
| `base64.tri` | `base64_encode(p, n, out, nopad)`, `base64_decode(s, n, out, max)` |
| `octets.tri` | `slurp(fd, *buf)`, `needs_utf8`, `as_octets`, `utf8_in_place` (§11.2) |
| `x509.tri` | `x509_parse`, `x509_signed_by`, `x509_host_ok`, `x509_read_pem`, `x509_load_store(path)`, `x509_verify_chain(chain, n, host, now)`, `x509_name`, `x509_error` |
| `tls.tri` | `tls_connect(t, fd, host, verify, now)`, `tls_accept(t, fd, identity, timeout_ms)`, `tls_read`, `tls_write`, `tls_close`, `tls_identity_load`, `tls_pending`, `tls_set_timeout`, and names for suites, groups and signatures |
| `certgen.tri` | `ec_key_write`, `ec_key_read` (SEC1 or PKCS #8), `pem_write`, `pem_read`, `x509_selfsigned`, `ecdsa_sign_der` |
| `ssh.tri` | the SSH transport shared by `sshd` and the client: version exchange, packets, KEXINIT, `kex_server`, `kex_client` |
| `sshkey.tri` | `key_load`, `key_create`, `key_blob`, `key_fingerprint` |
| `sshclient.tri` | `client_target("[user@]host")`, `client_connect(identity)`: connect, key exchange, `known_hosts`, login |
| `sftpclient.tri` | `sftp_start`, `sftp_stat`, `sftp_open`, `sftp_read`, `sftp_write`, `sftp_close`, `sftp_mkdir`, `sftp_rmdir`, `sftp_remove`, `sftp_rename`, `sftp_realpath`, `sftp_list`, `sftp_put`, `sftp_get`, `sftp_put_tree`, `sftp_get_tree`, `sftp_error` |

### 16.7 Graphics

| Module | Functions |
|---|---|
| `gfx.tri` | `rgb(r, g, b)` (each −13…13); `Surface`; `clip`, `fill_rect`, `put_pixel`, `get_pixel`, `blit`, `scroll_rect`; `font_load`, `draw_char`, `draw_text(s, x, y, text, fg, bg, scale)`, `text_width`; Unifont: `draw_char16`, `draw_text16`, `text_width16` |
| `trw.tri` | `trw_create`, `trw_fill`, `trw_text`, `trw_text16`, `trw_pixels`, `trw_scroll`, `trw_title`, `trw_spawn`, `trw_flush`, `trw_wait`, `trw_take`; `struct trw_event` (`type`, `a`, `b`, `c`) and the `TRW_*` constants |

### 16.8 Data structures and ternary idioms

| Module | Functions |
|---|---|
| `flags.tri` | Kleene vectors, 27 truth values per word (true +1, unknown 0, false −1; `&`, `\|`, `~` act on all lanes): `tv_get`, `tv_set`, `tv_fill`, `tv_trues`, `tv_falses`, `tv_unknowns`, `tv_all`, `tv_any`. Packed flags, 42 two-state flags per word: `pf_get`, `pf_set`, `pf_count`, `pf_next`, and arrays `pfa_words`, `pfa_get`, `pfa_set`, `pfa_count` |
| `heap3.tri` | a 3-ary min-heap of (key, value): `h3_init`, `h3_push`, `h3_peek`, `h3_pop`, `h3_size`, `h3_free`, `h3_valid` |
| `btree.tri` | an int → int B-tree with up to 27 keys a node: `bt_init`, `bt_get`, `bt_put`, `bt_del`, `bt_count`, `bt_walk`, `bt_height`, `bt_free` |
| `wcwidth.tri` | `tchar_width(cp)`, `tstr_width(s)` |

### 16.9 Data formats

The formats are specified in [FORMATS.md](FORMATS.md).

| Module | Functions |
|---|---|
| `tvarint.tri` | balanced varints, 8-trit groups least significant first with a control trit (+1 more follows, 0 the last, −1 free for containers): `tvar_put(buf, v) -> n`, `tvar_put_long`, `tvar_get(buf, n, *v) -> used` (−1 if cut short, not canonical or past a word), `tvar_get_long`, `tvar_control(t)`, `tvar_group(t)`; and a word as 3 trytes, most significant first: `tw_put(p, v)`, `tw_get(p)` |
| `ttv.tri` | TTV tagged values. Writing into a growing `TtvBuf` (`ttv_init`; `p`, `n`, `err`): `ttv_null`, `ttv_truth`, `ttv_int`, `ttv_long`, `ttv_real`, `ttv_text`, `ttv_octets`, `ttv_list(b, count)`, `ttv_map(b, count)`. Reading with a `TtvReader` (`ttv_reader(r, p, n)`): `ttv_kind`, `ttv_read_null`, `ttv_read_truth` (−2 if invalid), `ttv_read_int`, `ttv_read_long`, `ttv_read_real`, `ttv_read_text`, `ttv_read_octets`, `ttv_read_count(r, TTV_LIST\|TTV_MAP)`, `ttv_skip`, `ttv_at_end`; a failure sets `r.err` and every later read fails |
| `tau.tri` | TAU sound: `tau_header`, `tau_read_header` (checks magic, version, rate, channels 1–9, the length), `tau_sample`, `tau_set_sample` |
| `tim.tri` | TIM pictures: `struct Tim` (`w`, `h`, `ch`, `depth`, `s`), `tim_alloc`, `tim_write(t, TIM_RAW\|TIM_PREDICTED, *TtvBuf)`, `tim_read(p, n, *Tim)` (−1 for anything malformed), `tim_predict` (the median edge predictor), `tim_limit(depth)` |

---

## 17. Departures from POSIX, and limits

### 17.1 Departures

1. **The byte is a tryte.** `CHAR_BIT` is 9, and text is code points, not
   UTF-8. A character above U+2671 takes three trytes.
2. **No C.** There is no C compiler and no `c99`; programs are written in
   Trine.
3. **No printf.** Trine has no variadic functions; typed writers replace
   `printf`, and `puts` does not add a newline.
4. **No threads.** `clone` exists only in its fork form. Processes share
   memory through `MAP_SHARED|MAP_ANONYMOUS` and synchronize with `futex`.
5. **No symbolic links**, and hard links only on tfs. `lstat` is `stat`.
6. **Times** are whole seconds: `utimensat` drops the nanoseconds, and
   `stat` has no `st_atim`/`st_mtim`/`st_ctim`. The access time follows
   Linux's `relatime`: a read sets it only when it is older than the
   modification or change time, or a day old. Only `ITIMER_REAL` exists.
7. **Missing calls.** `truncate`, `select` (use `poll`), real-time
   signals and every timer but `ITIMER_REAL` do not exist. `vfork` is a
   fork (the parent does not wait for the child's `execve`).
   `SIGTTOU` and `TOSTOP` are not implemented. `VMIN` and `VTIME` are
   ignored.
8. **The shell** has no `IFS`; fields split on blanks. It reads no
   profile files.
9. **Scheduling** is round robin without priorities.
10. **`mmap`** maps only the framebuffer and shared anonymous memory, and
    `munmap` releases only the framebuffer.
11. **Utilities** are smaller than their POSIX models: `rm` has no `-r` or
    `-f`, `cp` has no `-r`, `tail` takes one file, `grep` has no `-E`.
12. **Passwords** use a single salted MD5, which is fast and therefore
    weak.
13. **Users.** `opendir` handles one directory at a time in a process.

### 17.2 Limits

| Resource | Limit |
|---|---|
| processes | 26 (27 task slots, one the idle task) |
| descriptors per process | 27 |
| open files, system-wide | 243 |
| in-core inodes | 243 |
| mounts | 9 |
| pipes | 27, each 3^9 trytes; `PIPE_BUF` 2187 |
| sockets | 27, one-page buffers; backlog 27 |
| pseudo-terminals | 9 |
| path | 242 characters |
| file name | 77 (tfs), 26 (ramfs) |
| hard links per file | 19 683 |
| `argv` + `envp` | 81 strings, 2187 trytes |
| supplementary groups | 9 |
| program text | 27 pages |
| heap | 729 pages past the start of the bss |
| stack | 27 pages |
| shared anonymous memory | 243 pages per process |
| `poll` | 81 descriptors |
| terminal line (canonical) | 242 characters |
| buffer cache | 27 sectors |
| shell script or `-c` text | 19 682 characters |
| shell history | 81 lines; function calls nest 81 deep |
| windows | 9 |
| default tfs volume | 486 blocks, 729 inodes |

---

## 18. How Trinux is tested

Every milestone ships with tests that boot Trinux on the emulator, type at
its console (or drive the mouse and keyboard, or a network peer), and
check the output. Most checks are refusals: what must fail, and with which
error. The suites are in [`crates/trinux/tests/`](../crates/trinux/tests/):

| Suite | What it checks |
|---|---|
| `boot.rs` | booting, the shell basics, signals and job control, `k3test`, `sigtest`, kernel panics, lazy FPU switching, clean page tables |
| `fs.rs`, `tfs.rs` | persistence across reboots, shutdown, `fstest`, a full volume, damaged disks, the initramfs fallback, `ps` and `/proc`; the host tfs and its fsck |
| `userland.rs` | the shell language and its errors, aliases, functions, here-documents, line editing, `edit`, the text tools, `calc`, `libctest`, the text cache, `mtxtest`, `vectest`, `chartest`, `dstest` |
| `users.rs` | accounts, set-ID programs, `su`, `passwd`, login failures, every permission refusal |
| `wm.rs` | the window system end to end, read back from the framebuffer; keyboard layouts, AltGr, wide characters, `k8test` |
| `unifont.rs` | every packed glyph reads back; the font fits on the disk |
| `memops.rs` | the kernel's assembly copy and fill routines |
| `net.rs` | ping, `ifconfig`, broken frames, UDP, `nettest`, TCP against smoltcp, a lossy link, port forwards, telnet, `cryptotest`, `pktest` |
| `outbound.rs` | connections out through the NAT, DNS, `wget` over HTTP, UDP out, `ntpdate` |
| `ssh.rs`, `ssh_client.rs`, `sftp.rs`, `scp_client.rs` | the host's OpenSSH against `sshd`; Trinux's `ssh`, `scp` and `sftp` against `sshd` |
| `tls.rs`, `httpd.rs` | certificate chains, `wget` against OpenSSL's server, `httpd` over HTTP and HTTPS, `certgen` |
| `selfhost.rs` | `tas` and `tric` against the host assembler and compiler at every optimization level, their error messages, and `tric` rebuilding itself |

The SSH and TLS suites skip their cases when the host has no `ssh` or
`openssl`. The design log ([`KERNEL.md`](KERNEL.md), [`NETWORK.md`](NETWORK.md),
[`SELFHOST.md`](SELFHOST.md)) lists the cases of each milestone.
