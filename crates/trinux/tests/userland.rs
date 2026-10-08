//! Trinux K7: the shell language, the core utilities and tlibc, each with
//! its error cases.

/// Boot the default disk and type each line at a prompt ("$ " or the
/// continuation prompt "> "). A line starting with `~` is sent as it is
/// (no newline added) once the machine has had 3 M steps since the last
/// input, for full-screen programs that print no prompt. Returns the
/// transcript and the machine.
fn session_m(lines: &[&str], max_steps: u64) -> (String, setun_core::Machine) {
    let disk = trinux::build_disk(&trinux::kernel_dir()).unwrap_or_else(|e| panic!("{e}"));
    let mut m = trinux::boot_trytes(&disk.trytes, 729, 0, "");
    let mut out = String::new();
    let (mut next, mut steps, mut idle) = (0, 0, 0);
    let mut logged_in = false;
    while steps < max_steps && !m.powered_off {
        m.run(1_000_000);
        steps += 1_000_000;
        idle += 1_000_000;
        out.push_str(&m.dev.serial.take_output());
        if out.contains("Kernel panic") {
            break;
        }
        // the first login is root's (password "root"); later ones are the test's
        if !logged_in && out.ends_with("login: ") {
            m.dev.serial_input("root\n");
            continue;
        }
        if !logged_in && out.ends_with("Password: ") {
            m.dev.serial_input("root\n");
            logged_in = true;
            continue;
        }
        let Some(line) = lines.get(next) else {
            if out.contains("init: login session exited") && out.ends_with("login: ") {
                break;
            }
            continue;
        };
        if let Some(raw) = line.strip_prefix('~') {
            if idle >= 3_000_000 {
                m.dev.serial_input(raw);
                next += 1;
                idle = 0;
            }
        } else if out.ends_with("$ ") || out.ends_with("> ") {
            m.dev.serial_input(&format!("{line}\n"));
            next += 1;
            idle = 0;
        }
    }
    (out, m)
}

fn session(lines: &[&str], max_steps: u64) -> String {
    session_m(lines, max_steps).0
}

fn assert_has(out: &str, wants: &[&str]) {
    for w in wants {
        assert!(out.contains(w), "missing {w:?} in:\n{out}");
    }
}

fn assert_lacks(out: &str, nots: &[&str]) {
    for w in nots {
        assert!(!out.contains(w), "unexpected {w:?} in:\n{out}");
    }
}

#[test]
fn k7_shell_language() {
    let out = session(
        &[
            "x=5; echo $x ${x} \"$x\" '$x' \\$x",
            "a='one  two'; for w in $a; do echo [$w]; done; for w in \"$a\"; do echo \"[$w]\"; done",
            "i=0; while [ $i -lt 3 ]; do echo w$i; i=$((i+1)); done; until [ $i -eq 0 ]; do i=$((i-1)); done; echo i=$i",
            "for n in 1 2 3 4 5; do if [ $n = 2 ]; then continue; elif [ $n = 4 ]; then break; else echo n$n; fi; done",
            "for a in x y; do for b in 1 2; do if [ $b = 2 ]; then continue 2; fi; echo $a$b; done; done",
            "case abc.txt in *.c) echo C;; *.txt|*.md) echo text;; *) echo other;; esac",
            "echo $(echo nested $(echo deep)) \"$(echo '  spaced  ')\"",
            "echo $((2+3*4)) $(( (2+3)*4 )) $((10/3)) $((10%3)) $((-7+2)) $((3>2)) $((1&&0))",
            "unset u; echo ${u:-dflt} ${u-dash} [${u:+alt}] ${#x}; echo ${u:=assigned} $u",
            "set -- alpha 'beta gamma' delta; echo $# $1; for a in \"$@\"; do echo \"<$a>\"; done; shift; echo $1",
            "cd /bin; echo s?ee* m? [cd][dp] *ort; echo nomatch*; cd /",
            "echo ~ ~/x",
            "FOO=prefix env | grep FOO; echo \"after: [$FOO]\"",
            "export E1=exported; sh -c 'echo child sees $E1'; unset E1; sh -c 'echo child sees [$E1]'",
            "while read line; do echo \"<$line>\"; done < /etc/motd",
            "{ echo a; echo b; } > /tmp/grp; cat /tmp/grp; read first second < /tmp/grp; echo $first",
            "(cd /tmp; pwd); pwd",
            "! false && echo negated; false || echo or-ran; true && false; echo $?",
            "echo 'echo script $0 got $# args: $*' > /tmp/s.sh; sh /tmp/s.sh p q r",
            "sh -c 'set -e; echo before; false; echo never'; echo status $?",
            "sh -c 'set -x; echo traced' 2>&1",
            "for i in 1 2",
            "do echo multi$i",
            "done",
            "if true",
            "then echo cont-if",
            "fi",
            "type cd ls nosuch",
            "exit",
        ],
        700_000_000,
    );
    assert_has(
        &out,
        &[
            "5 5 5 $x $x\n",
            "[one]\n[two]\n[one  two]\n",
            "w0\nw1\nw2\ni=0\n",
            "n1\nn3\n",
            "x1\ny1\n",
            "text\n",
            "nested deep   spaced  \n",
            "14 20 3 1 -5 1 0\n",
            "dflt dash [] 1\nassigned assigned\n",
            "3 alpha\n<alpha>\n<beta gamma>\n<delta>\nbeta gamma\n",
            "sleep mv cp sort\n",
            "nomatch*\n",
            "$ echo ~ ~/x\n/root /root/x\n",
            "FOO=prefix\nafter: []\n",
            "child sees exported\nchild sees []\n",
            "<Welcome to Trinux on Setun-11̅.>\n<Everything here is a power of 3.>\n",
            "a\nb\na\n",
            "/tmp\n/\n",
            "negated\nor-ran\n1\n",
            "script /tmp/s.sh got 3 args: p q r\n",
            "before\nstatus 1\n",
            "+ echo traced\ntraced\n",
            "> do echo multi$i\n> done\nmulti1\nmulti2\n",
            "> then echo cont-if\n> fi\ncont-if\n",
            "cd is a shell builtin\nls is /bin/ls\nnosuch: not found\n",
        ],
    );
}

#[test]
fn scripts_run_their_interpreter() {
    // "#!interpreter [argument]" (the kernel, as Linux's binfmt_script):
    // argv = { interpreter, [argument,] the script, its arguments }; an
    // interpreter may itself be a script, four deep; a text file without
    // "#!" (ENOEXEC) runs in sh, as POSIX has it
    let out = session(
        &[
            "echo '#!/bin/sh' > /tmp/s1; echo 'echo zero=$0 one=$1 two=$2' >> /tmp/s1",
            "echo '#!/bin/echo hello' > /tmp/s2",
            "echo '#!/tmp/s2' > /tmp/s3",
            "echo '#!  /bin/echo   one arg  ' > /tmp/s4",
            "echo '#!/tmp/lp' > /tmp/lp",
            "echo '#!' > /tmp/bare; echo 'echo bare-fallback' >> /tmp/bare",
            "echo 'echo plain $1' > /tmp/plain",
            "echo '#!/nope/sh' > /tmp/miss",
            "echo '#!/bin/sh' > /tmp/noexec; echo 'echo NEVER' >> /tmp/noexec",
            "chmod 755 /tmp/s1 /tmp/s2 /tmp/s3 /tmp/s4 /tmp/lp /tmp/bare /tmp/plain /tmp/miss",
            "/tmp/s1 a b",
            "/tmp/s2 x",
            "/tmp/s3 y",
            "/tmp/s4",
            "/tmp/lp; echo status $?",
            "/tmp/bare; echo status $?",
            "/tmp/plain q",
            "/tmp/miss; echo status $?",
            "/tmp/noexec; echo status $?",
            "cd /tmp; ./s1 rel",
        ],
        3_000_000_000,
    );
    assert_has(
        &out,
        &[
            "zero=/tmp/s1 one=a two=b",
            "hello /tmp/s2 x",
            "hello /tmp/s2 /tmp/s3 y",
            "one arg /tmp/s4",
            "/tmp/lp: Too many levels of symbolic links\nstatus 126",
            "bare-fallback\nstatus 0",
            "plain q",
            "/tmp/miss: not found\nstatus 127",
            "/tmp/noexec: Permission denied\nstatus 126",
            "zero=./s1 one=rel",
        ],
    );
    assert_lacks(&out, &["\nNEVER"]);
}

#[test]
fn time_tools() {
    // touch with dates, stamps and a reference; stat; ls -l times and -t;
    // date -d / -r and %N; find -newer, -mtime, -mmin; sleep with a
    // fraction; time. The clock starts at 2026-09-21 14:13:20 UTC.
    let out = session(
        &[
            "touch -d '2024-10-08 15:06:40' /tmp/a; stat -c '%X %Y' /tmp/a",
            "touch -t 202001021530.45 /tmp/b; stat -c '%X %Y %y' /tmp/b",
            "touch -m -d @1000000000 /tmp/a; stat -c 'A %X M %Y' /tmp/a",
            "touch -a -r /tmp/b /tmp/a; stat -c 'A %X M %Y' /tmp/a",
            "touch /tmp/c",
            "touch -c /tmp/none; ls /tmp/none",
            "touch -d bogus /tmp/a; echo status $?",
            "touch -t 2020013 /tmp/a; echo status $?",
            "ls -l /tmp/a /tmp/c",
            "ls -t /tmp; ls -tr /tmp",
            "ls -lu /tmp/a",
            "date -d @1728400000 +%F_%T; date -d @1.5 +%s.%N; date -r /tmp/a +%s",
            "date -d nope; echo status $?",
            "stat /tmp/b",
            "find /tmp -type f -newer /tmp/b",
            "find /tmp -type f -mtime +1000",
            "find /tmp -type f -mtime -1; find /tmp -type f -mmin -5",
            "find /tmp -mtime x; echo status $?",
            "sleep 0.3; echo slept",
            "sleep -1; echo status $?; sleep 1x; echo status $?",
            "time sleep 0.5",
            "time false; echo status $?",
            "time nosuchcmd; echo status $?",
            // a non-owner: now needs write permission, other times ownership
            "touch /tmp/own; chmod 644 /tmp/own; su user -c 'touch /tmp/own'; echo status $?",
            "su user -c 'touch -d @5 /tmp/own'; echo status $?",
            "chmod 666 /tmp/own; su user -c 'touch /tmp/own'; echo now-ok $?; su user -c 'touch -d @5 /tmp/own'; echo status $?",
        ],
        3_000_000_000,
    );
    assert_has(
        &out,
        &[
            "1728400000 1728400000",
            "1577979045 1577979045 2020-01-02 15:30:45 UTC",
            "A 1728400000 M 1000000000",
            "A 1577979045 M 1000000000",
            "ls: /tmp/none: No such file or directory",
            "touch: invalid date: bogus\nstatus 1",
            "touch: invalid date: 2020013\nstatus 1",
            "Sep  9  2001 /tmp/a",
            "Sep 21 14:1",
            "ls -tr /tmp\nc\nb\na\na\nb\nc\n",
            "Jan  2  2020 /tmp/a",
            "2024-10-08_15:06:40\n1.500000000\n1000000000",
            "date: invalid date: nope\nstatus 1",
            "  File: /tmp/b",
            "regular file",
            "Modify: 2020-01-02 15:30:45 UTC",
            "-type f -newer /tmp/b\n/tmp/c\n",
            "-mtime +1000\n/tmp/b\n/tmp/a\n", // (directory order)
            "-mmin -5\n/tmp/c\n/tmp/c\n",
            "expected a number after -mtime\nstatus 1",
            "slept",
            "sleep: invalid time\nstatus 1\nsleep: invalid time\nstatus 1",
            "real 0.5",
            "real 0.",
            "status 1",
            "status 127",
            "touch: /tmp/own: Permission denied\nstatus 1",
            "touch: /tmp/own: Operation not permitted\nstatus 1",
            "now-ok 0\ntouch: /tmp/own: Operation not permitted\nstatus 1",
        ],
    );
}

#[test]
fn k7_shell_errors() {
    let out = session(
        &[
            "fi",
            "echo status $?",
            "sh -c 'if true; then echo x'; echo status $?",
            "sh -c \"echo 'unterminated\"; echo status $?",
            "echo $((1/0)); echo SAME\"\"LINE",
            "echo status $?",
            "echo $((2+)); echo $((1 2))",
            "unset q; echo ${q:?is required}; echo NEVER\"\"HERE",
            "echo ${%}",
            "cd /nope",
            "export 1x=2; export ok=1 bad-name; echo status $?",
            "read 1bad < /etc/motd; echo status $?",
            "shift 5; echo status $?",
            "nosuchcmd; echo status $?",
            "/etc/motd; echo status $?",
            "break; continue; echo loops-ok",
            "sh -c 'exit abc'; echo status $?",
            "echo x > /nope/f; echo status $?",
            "cat < /nope; echo status $?",
            "wait %5; echo status $?",
            "sh -c 'echo $((1/0)); echo NOT\"\"REACHED'; echo status $?",
            "for 1v in a; do echo x; done",
            "case x in esac; echo empty-case $?",
            "echo ok-after-errors",
            "exit 4",
        ],
        400_000_000,
    );
    assert_has(
        &out,
        &[
            "sh: syntax error near unexpected token `fi'\n$ echo status $?\nstatus 2\n",
            "sh: syntax error: unexpected end of file\nstatus 2\n",
            "sh: syntax error: unterminated quote\nstatus 2\n",
            "sh: arithmetic: division by zero\n$ echo status $?\nstatus 1\n",
            "sh: arithmetic: syntax error\n",
            "sh: q: is required\n",
            "sh: bad substitution\n",
            "sh: cd: /nope: No such file or directory\n",
            "sh: export: `1x=2': not a valid identifier\nsh: export: `bad-name': not a valid identifier\nstatus 1\n",
            "sh: read: `1bad': not a valid identifier\nstatus 2\n",
            "sh: shift: can't shift that many\nstatus 1\n",
            "sh: nosuchcmd: not found\nstatus 127\n",
            "sh: /etc/motd: Permission denied\nstatus 126\n",
            "loops-ok\n",
            "sh: exit: numeric argument required: abc\nstatus 2\n",
            "sh: /nope/f: No such file or directory\nstatus 1\n",
            "sh: /nope: No such file or directory\nstatus 1\n",
            "sh: wait: no such job\nstatus 127\n",
            "sh: arithmetic: division by zero\nstatus 1\n",
            "sh: for: bad variable name: 1v\n",
            "empty-case 0\n",
            "ok-after-errors\n",
            "init: login session exited with 4",
        ],
    );
    // (the commands are echoed as typed, so the markers are split by quotes)
    assert_lacks(&out, &["SAMELINE", "NEVERHERE", "NOTREACHED"]);
}

#[test]
fn sh_functions_eval_heredocs() {
    let out = session(
        &[
            "greet() { echo \"hello $1, $# args\"; }; greet world a b",
            "fact() {",
            "  if [ $1 -le 1 ]; then echo 1; return; fi",
            "  echo $(( $1 * $(fact $(( $1 - 1 ))) ))",
            "}",
            "fact 6",
            "f() { return 3; echo ne\"\"ver; }; f; echo rc=$?",
            "set -- outer1 outer2; f2() { echo in:$1; }; f2 inner; echo out:$1 $#",
            "g() { for i in 1 2 3; do if [ $i = 2 ]; then return 7; fi; echo g$i; done; }; g; echo g-rc=$?",
            "h() { break; }; for i in a b; do h; echo loop$i; done",
            "show() { echo v=$V; sh -c 'echo child=$V'; }; V=temp show; echo after=[$V]",
            "greet redirected > /tmp/fo; echo saved; cat /tmp/fo",
            "greet() { echo redefined; }; greet; unset -f greet; greet",
            "type fact cd eval",
            "cmd='echo evaluated $((1+2))'; eval $cmd",
            "eval 'x=1; y=2'; echo $x$y; v=name; eval \"$v=dyn\"; echo [$name]",
            "eval 'f3() { echo from-eval; }'; f3; eval false; echo e=$?",
            "cat <<EOF",
            "line $x and $(echo sub) $((2+5))",
            "  \"quotes\" \\$HOME",
            "EOF",
            "while read -r l; do echo \"got:$l\"; done <<'END'",
            "no $x here",
            "END",
            "read -r s <<-EOT; echo \"[$s]\"",
            "\tstripped",
            "\tEOT",
            "read -r p <<A; read -r q <<B; echo \"$q/$p\"",
            "first",
            "A",
            "second",
            "B",
            "hf() {",
            "cat <<EOF",
            "in-func $1",
            "EOF",
            "}",
            "hf arg",
            "cat > /tmp/hs.sh <<'S'",
            "cat <<E",
            "script-heredoc $1",
            "E",
            "S",
            "sh /tmp/hs.sh ok",
            "echo 'dotvar=sourced; dotfn() { echo dotfn-ran; return 4; }' > /tmp/lib.sh; . /tmp/lib.sh; echo [$dotvar]; dotfn; echo d=$?",
            "echo 'echo before-ret; return 0; echo after\"\"-ret' > /tmp/r.sh; . /tmp/r.sh; echo dot-done",
            "exit",
        ],
        900_000_000,
    );
    assert_has(
        &out,
        &[
            "hello world, 3 args\n",
            "$ fact 6\n720\n",
            "rc=3\n",
            "in:inner\nout:outer1 2\n",
            "g1\ng-rc=7\n",
            "loopa\nloopb\n",
            "v=temp\nchild=temp\nafter=[]\n",
            "saved\nhello redirected, 1 args\n",
            "redefined\nsh: greet: not found\n",
            "fact is a function\ncd is a shell builtin\neval is a special shell builtin\n",
            "evaluated 3\n",
            "12\n[dyn]\n",
            "from-eval\ne=1\n",
            "line 1 and sub 7\n  \"quotes\" $HOME\n",
            "got:no $x here\n",
            "[stripped]\n",
            "second/first\n",
            "in-func arg\n",
            "script-heredoc ok\n",
            "[sourced]\ndotfn-ran\nd=4\n",
            "before-ret\ndot-done\n",
        ],
    );
    assert_lacks(&out, &["never", "g3", "after-ret"]);
}

#[test]
fn sh_functions_eval_heredocs_errors() {
    let out = session(
        &[
            "f() echo hi",
            "1bad() { echo; }; echo status $?",
            "eval() { :; }",
            "return 1; echo status $?",
            "r() { return abc; }; r; echo status $?",
            "inf() { inf; }; inf; echo survived $?",
            "eval 'if true'; echo status $?",
            "eval 'echo \"open'; echo status $?",
            "sh -c 'cat <<NOEND'; echo status $?",
            "cat <<",
            ". /nope/lib.sh; echo status $?",
            ". nosuchlib; echo status $?",
            ".; echo status $?",
            "unset -f nosuch; echo unset-ok",
            "cat <<EOF; echo status $?",
            "${undefined_var:?boom}",
            "EOF",
            "echo ok-after-errors",
            "exit 5",
        ],
        500_000_000,
    );
    assert_has(
        &out,
        &[
            "sh: syntax error near unexpected token `echo'\n",
            "sh: 1bad: bad function name\nstatus 2\n",
            "sh: eval: bad function name\n",
            "sh: return: not in a function or . file\nstatus 1\n",
            "sh: return: numeric argument required: abc\nstatus 2\n",
            "sh: inf: function nesting too deep\nsurvived 2\n",
            "sh: syntax error: unexpected end of file\nstatus 2\n",
            "sh: syntax error: unterminated quote\nstatus 2\n",
            "sh: syntax error: unterminated here-document\nstatus 2\n",
            "sh: syntax error near unexpected token `newline'\n",
            "sh: .: /nope/lib.sh: No such file or directory\nstatus 1\n",
            "sh: nosuchlib: not found\nstatus 1\n",
            "sh: .: filename argument required\nstatus 2\n",
            "unset-ok\n",
            "sh: undefined_var: boom\nstatus 1\n",
            "ok-after-errors\n",
            "init: login session exited with 5",
        ],
    );
}

#[test]
fn sh_aliases_and_local() {
    let out = session(
        &[
            "alias hi='echo hi-from-alias'; hi there",
            "alias hi",
            "alias q=\"echo \\\"it's\\\"\"; q; alias q",
            "type hi",
            "alias echo='echo [aliased]'; echo x; \\echo y; 'echo' z; unalias echo; echo w",
            "alias run='env '; alias greet2='echo ch\"\"ained'; run greet2",
            "alias up='grep -c .'; echo n=$(cat /etc/motd | up)",
            "af() { hi inside; }; af",
            "alias c=cat",
            "c <<EOF",
            "via-alias $((1+1))",
            "EOF",
            "unalias -a; hi",
            "x=global; lf() { local x=inner y; x=changed; echo in:$x; y=set; }; lf; echo out:$x [$y]",
            "outer() { local v=outer; inner; echo back:$v; }; inner() { echo sees:$v; v=modified; }; outer; echo top:[$v]",
            "cnt() { local n=$1; if [ $n -gt 0 ]; then cnt $((n-1)); fi; echo n=$n; }; cnt 2",
            "sp() { local s=$1; echo \"[$s]\"; }; sp 'a  b'",
            "w='p  q'; export E3=$w; sh -c 'echo \"<$E3>\"'",
            "export EX=outer; ef() { local EX=in; sh -c 'echo child:$EX'; }; ef; sh -c 'echo after:$EX'",
            "exit",
        ],
        700_000_000,
    );
    assert_has(
        &out,
        &[
            "hi-from-alias there\n",
            "$ alias hi\nalias hi='echo hi-from-alias'\n",
            "it's\nalias q='echo \"it'\\''s\"'\n",
            "hi is aliased to `echo hi-from-alias'\n",
            "[aliased] x\ny\nz\nw\n",
            "chained\n",
            "n=2\n",
            "hi-from-alias inside\n",
            "via-alias 2\n",
            "sh: hi: not found\n",
            "in:changed\nout:global []\n",
            "sees:outer\nback:modified\ntop:[]\n",
            "n=0\nn=1\nn=2\n",
            "[a  b]\n",
            "<p  q>\n",
            "child:in\nafter:outer\n",
        ],
    );
}

#[test]
fn sh_aliases_and_local_errors() {
    let out = session(
        &[
            "local z=1; echo status $?",
            "bad() { local 1x=2; echo st=$?; }; bad",
            "alias 'bad name=x'; echo status $?",
            "alias nope; echo status $?",
            "unalias nope; echo status $?",
            "unalias; echo status $?",
            "alias loop=loop2; alias loop2=loop; loop; echo status $?",
            "alias self='self'; self; echo status $?",
            "echo ok-after-errors",
            "exit 6",
        ],
        400_000_000,
    );
    assert_has(
        &out,
        &[
            "sh: local: not in a function\nstatus 1\n",
            "sh: local: `1x=2': not a valid identifier\nst=1\n",
            "sh: alias: `bad name=x': invalid alias name\nstatus 1\n",
            "sh: alias: nope: not found\nstatus 1\n",
            "sh: unalias: nope: not found\nstatus 1\n",
            "sh: unalias: usage: unalias [-a] name ...\nstatus 2\n",
            "sh: loop: not found\nstatus 127\n",
            "sh: self: not found\nstatus 127\n",
            "ok-after-errors\n",
            "init: login session exited with 6",
        ],
    );
}

#[test]
fn sh_line_editing() {
    let out = session(
        &[
            "echo helo\u{1b}[Dl",                         // Left, then insert
            "echo first",
            "\u{1b}[A",                                   // Up: the last line again
            "ec\thi",                                     // Tab completes a command
            "cat /etc/mo\t",                              // and a file name
            "true /usr/lib/trwtest/\t\t",                 // the second Tab lists the choices
            "echo abc def\u{17}ghi",                      // ^W deletes a word
            "garbage\u{15}echo ok-after-kill",            // ^U the line so far
            "cho xy\u{1}e\u{5}z",                         // ^A to the start, ^E to the end
            "echo abd\u{7f}c",                            // Backspace
            "echo abXc\u{1b}[D\u{1b}[D\u{1b}[3~",         // Delete at the cursor
            "echo mid\u{1b}[H\u{1b}[F!",                  // Home and End keys
            "echo kept\u{1b}[A\u{1b}[B",                  // Up then Down brings the typed line back
            "echo xy\u{1b}[D\u{4}",                       // ^D inside a line deletes
            "echo \u{1b}[Zok",                            // an unknown sequence is ignored
            "echo /nonexist\t",                           // Tab without a match does nothing
            "history",
            "exit",
        ],
        500_000_000,
    );
    assert_has(
        &out,
        &[
            "\nhello\n",
            "$ echo first\nfirst\n$ \r$ echo first\u{1b}[K\nfirst\n",
            "$ echo hi\nhi\n",
            "$ cat /etc/motd \nWelcome to Trinux",
            "\nbigwin  crash  huge  nocreate  unknown\n",
            "\nabc ghi\n",
            "\nok-after-kill\n",
            "\nxyz\n",
            "$ echo abd\u{8} \u{8}c\nabc\n",
            "\nabc\n$ echo mid",
            "\nmid!\n",
            "\nkept\n",
            "\nx\n",
            "\nok\n",
            "\n/nonexist\n",
            "    1  echo hello\n    2  echo first\n    3  echo hi\n",
        ],
    );
}

#[test]
fn the_editor() {
    let out = session(
        &[
            "edit /tmp/e.txt",
            "~one\rtwo\rthree",
            "~\u{1b}[A\u{1b}[A",                          // up to "one"
            "~\u{b}",                                      // ^K cuts it
            "~\u{1b}[B",                                   // down to "three"
            "~\u{15}",                                     // ^U pastes it above
            "~\u{17}",                                     // ^W find
            "~hre\r",
            "~X",                                          // typed where "hre" starts
            "~\u{7}",                                      // ^G go to line
            "~9\r",                                        // past the end: refused
            "~\u{7}",
            "~3\r",
            "~#",
            "~\u{13}",                                     // ^S save
            "~\u{18}",                                     // ^X exit
            "cat /tmp/e.txt",
            "edit /tmp/e.txt",
            "~ZZZ\u{18}",                                  // changed, then ^X: asked
            "~n\r",                                        // no: the file stays
            "cat /tmp/e.txt; echo kept",
            "edit /tmp/e.txt",
            "~\u{1b}[F!\u{1b}[B\u{1b}[H\u{7f}",            // End, "!", down, Home, Backspace joins
            "~\u{18}",
            "~y\r",                                        // yes: saved
            "cat /tmp/e.txt; echo saved",
            "edit /tmp/new.txt",
            "~привет\tмир\u{13}\u{18}",
            "cat /tmp/new.txt",
            "exit",
        ],
        700_000_000,
    );
    assert_has(
        &out,
        &[
            "[ New file ]",
            "Invalid line number",
            "Wrote 3 lines",
            "$ cat /tmp/e.txt\ntwo\none\n#tXhree\n",
            "Save the changes? (y/n) ",
            "$ cat /tmp/e.txt; echo kept\ntwo\none\n#tXhree\nkept\n",
            "$ cat /tmp/e.txt; echo saved\ntwo!one\n#tXhree\nsaved\n",
            "$ cat /tmp/new.txt\nпривет\tмир\n",
        ],
    );
}

#[test]
fn the_editors_with_wide_characters() {
    // A6: 3-tryte characters moved over and deleted whole; a wide one takes
    // two columns (the shell moves the cursor by two, edit counts them)
    let out = session(
        &[
            "echo 日本\u{1b}[D語",                            // Left over 本 (two columns back)
            "echo 日本\u{7f}x",                               // Backspace takes 本 whole
            "edit /tmp/w.txt",
            "~日本語\u{1b}[D\u{1b}[D\u{7f}X",                 // Left twice (before 本), Backspace (日), insert
            "~\u{1b}[F\u{7f}😀\u{1b}[H\u{1b}[3~",            // End, Backspace (語), 😀, Home, Delete (X)
            "~\u{13}\u{18}",
            "cat /tmp/w.txt",
            "exit",
        ],
        500_000_000,
    );
    assert_has(&out, &["\u{1b}[2D", "\n日語本\n", "\n日x\n", "line 1/1, col 7", "line 1/1, col 3", "Wrote 1 line", "$ cat /tmp/w.txt\n本😀\n"]);
}

#[test]
fn the_editor_errors() {
    let out = session(
        &[
            "edit /tmp; echo st=$?",
            "edit a b; echo st=$?",
            "echo | edit /tmp/x; echo st=$?",
            "edit /nope/file",
            "~text\u{13}",                                 // ^S: cannot write there
            "~\u{18}",
            "~n\r",
            "echo st=$?",
            "exit",
        ],
        300_000_000,
    );
    assert_has(
        &out,
        &[
            "edit: /tmp: Is a directory\nst=1\n",
            "usage: edit [file]\nst=2\n",
            "edit: standard input is not a terminal\nst=1\n",
            "Cannot write: No such file or directory",
            "st=0\n",
        ],
    );
}

#[test]
fn text_tools() {
    let out = session(
        &[
            "echo 'hello world' | sed 's/world/there/'",
            "echo aaa | sed 's/a/b/g'; echo aaa | sed 's/a/b/2'; echo aaa | sed 's/a/b/2g'; echo abc | sed 's/b*/X/g'",
            "echo 'key=value' | sed 's/\\(.*\\)=\\(.*\\)/\\2=\\1/'; echo 'aXa' | sed 's/a/[&]/g'",
            "cat > /tmp/f <<EOF",
            "one",
            "two",
            "three",
            "four",
            "EOF",
            "sed -n '2p' /tmp/f; sed -n '/t/p' /tmp/f",
            "sed '2,3d' /tmp/f; sed -n '$p' /tmp/f; sed -n '$=' /tmp/f",
            "sed '/two/,/three/s/^/> /' /tmp/f",
            "sed '1!d' /tmp/f; sed 2q /tmp/f; echo cab | sed 'y/abc/xyz/'",
            "sed '1i top' /tmp/f | head -n 2; sed '$a end' /tmp/f | tail -n 1; sed '2c changed' /tmp/f",
            "sed -e 's/o/0/' -e 's/e/3/' /tmp/f; sed -n 'n;p' /tmp/f",
            "sed -i 's/one/ONE/' /tmp/f; head -n 1 /tmp/f",
            "echo hello | tr a-z A-Z; echo 'a  b   c' | tr -s ' '; echo hello | tr -d l",
            "echo привет | tr а-я А-Я; echo hello | tr '[:lower:]' '[:upper:]'; echo abc123 | tr -cd 0-9",
            "echo a:b:c:d | cut -d: -f2,4; echo abcdef | cut -c2-4; echo abcdef | cut -c-2,5-",
            "echo nodelim | cut -d: -f2; echo nodelim | cut -s -d: -f2; echo cut-done",
            "echo AB | od -c; echo 1̅ | od -c; echo A | od -t; echo A | od -d; echo A | od -3",
            "cat > /tmp/a <<EOF",
            "1",
            "2",
            "3",
            "4",
            "EOF",
            "cat > /tmp/b <<EOF",
            "1",
            "X",
            "3",
            "4",
            "5",
            "EOF",
            "diff /tmp/a /tmp/b; echo st=$?; diff /tmp/a /tmp/a; echo st=$?; diff -q /tmp/a /tmp/b",
            "find /usr/lib -type f -name 'n*'; find /etc -maxdepth 0; find /usr ! -type d -name 'b*'",
            "find /bin -name sed -exec echo found {} ';'",
            "echo a b c | xargs echo x; echo 1 2 3 4 5 | xargs -n 2 echo; echo /etc/motd | xargs wc -l",
            "find /usr/lib -name 'c*' | xargs -I F echo [F]",
            "echo -n abc > /tmp/abc; md5sum /tmp/abc; echo Привет | md5sum",
            // past one block, and a two-byte character across the block boundary
            "echo Съешь же ещё этих мягких французских булок, да выпей чаю. The quick brown fox jumps over the lazy dog 0123456789 Ёё | md5sum",
            "echo aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaЖbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb | md5sum",
            "exit",
        ],
        900_000_000,
    );
    assert_has(
        &out,
        &[
            "hello there\n",
            "bbb\naba\nabb\nXaXcX\n",
            "value=key\n[a]X[a]\n",
            "two\ntwo\nthree\n",
            "one\nfour\nfour\n4\n",
            "one\n> two\n> three\nfour\n",
            "one\none\ntwo\nzxy\n",
            "top\none\nend\none\nchanged\nthree\nfour\n",
            "0n3\ntw0\nthr3e\nf0ur\ntwo\nfour\n",
            "ONE\n",
            "HELLO\na b c\nheo\n",
            "ПРИВЕТ\nHELLO\n123$ ",
            "b:d\nbcd\nabef\n",
            "nodelim\ncut-done\n",
            // 'A' = 65 = 2*27 + 11 is 02B, newline 00A; in base 3, 65 = 81 - 27 + 9 + 3 - 1
            "0000000   A   B  \\n\n0000003\n0000000   1\u{305}  \\n\n0000002\n0000000  02B  00A\n0000002\n0000000     65     10\n0000002\n",
            "0000000  000011\u{305}111\u{305}  000000101\n0000002\n",
            "2c2\n< 2\n---\n> X\n4a5\n> 5\nst=1\nst=0\nFiles /tmp/a and /tmp/b differ\n",
            "/usr/lib/trwtest/nocreate\n/etc\n/usr/lib/trwtest/bigwin\n",
            "found /bin/sed\n",
            "x a b c\n1 2\n3 4\n5\n      2 /etc/motd\n",
            "[/usr/lib/trwtest/crash]\n",
            "900150983cd24fb0d6963f7d28e17f72  /tmp/abc\n5f0650b247a646355dfec2d2610a960c  -\n",
            "10f151c678f0c290b6741be0f86798e2  -\n",
            "007acfc60d776106ef64134342370347  -\n",
        ],
    );
}

#[test]
fn text_tools_errors() {
    let out = session(
        &[
            "sed 's/a/b' /etc/motd; echo st=$?",
            "sed k /etc/motd; echo st=$?",
            "sed 's/\\(a/b/' /etc/motd; echo st=$?",
            "sed 'y/ab/x/' /etc/motd; echo st=$?",
            "sed -n p /nope; echo st=$?",
            "tr; echo st=$?",
            "tr a; echo st=$?",
            "echo x | tr z-a q; echo st=$?",
            "echo x | tr '[:bogus:]' q; echo st=$?",
            "cut /etc/motd; echo st=$?",
            "cut -f 0 /etc/motd; echo st=$?",
            "cut -d ab -f1 /etc/motd; echo st=$?",
            "od -q; echo st=$?",
            "od /nope; echo st=$?",
            "diff /etc/motd; echo st=$?",
            "diff /etc/motd /nope; echo st=$?",
            "find /nope; echo st=$?",
            "find /etc -bogus; echo st=$?",
            "find /etc -type z; echo st=$?",
            "find /etc -exec echo; echo st=$?",
            "echo x | xargs nosuchcmd; echo st=$?",
            "echo a | xargs false; echo st=$?",
            "echo \"'a\" | xargs; echo st=$?",
            "md5sum /nope; echo st=$?",
            "exit",
        ],
        600_000_000,
    );
    assert_has(
        &out,
        &[
            "sed: -e expression, char 6: unterminated `s' command\nst=1\n",
            "sed: -e expression, char 1: unknown command\nst=1\n",
            "sed: -e expression, char 9: unmatched \\(\nst=1\n",
            "sed: -e expression, char 8: strings for `y' command are different lengths\nst=1\n",
            "sed: /nope: No such file or directory\nst=0\n",
            "usage: tr [-cds] set1 [set2]\nst=2\n$ tr a; echo st=$?\nusage: tr [-cds] set1 [set2]\nst=2\n",
            "tr: range-endpoints are in reverse collating sequence order\nst=1\n",
            "tr: invalid character class: bogus\nst=1\n",
            "usage: cut -c list | -f list [-d c] [-s] [file ...]\nst=2\n",
            "cut: invalid list: 0\nst=1\n",
            "cut: the delimiter must be a single character\nst=1\n",
            "usage: od [-c|-d|-o|-x|-t|-3] [file]\nst=2\n",
            "od: /nope: No such file or directory\nst=1\n",
            "usage: diff [-q] file1 file2\nst=2\n",
            "diff: /nope: No such file or directory\nst=2\n",
            "find: /nope: No such file or directory\nst=1\n",
            "find: unknown predicate: -bogus\nst=1\n",
            "find: unknown argument to -type: z\nst=1\n",
            "find: missing argument to -exec\nst=1\n",
            "xargs: nosuchcmd: No such file or directory\nst=127\n",
            "st=123\n",
            "xargs: unmatched quote\nst=1\n",
            "md5sum: /nope: No such file or directory\nst=1\n",
        ],
    );
}

#[test]
fn k7_utilities() {
    let out = session(
        &[
            "seq 10 > /tmp/n; head -n 3 /tmp/n; tail -n 2 /tmp/n; wc /tmp/n; wc -l < /tmp/n",
            "printf=x; echo banana apple cherry apple | tee /tmp/fruit | wc -w",
            "for f in banana apple Cherry apple date; do echo $f; done > /tmp/f",
            "sort /tmp/f; sort -r -u /tmp/f | head -n 2",
            "echo 10 9 100 | tr_missing 2>/dev/null; for v in 10 9 100 1; do echo $v; done | sort -n",
            "sort /tmp/f | uniq -c",
            "grep -n apple /tmp/f; grep -c -i cherry /tmp/f; grep -v a /tmp/f; grep '^b.n' /tmp/f; grep 'e$' /tmp/f",
            "grep '[cd][ha]' /tmp/f; grep -q apple /tmp/f && echo found; grep 'x*y*a' /tmp/f | head -n 1",
            "cp /tmp/f /tmp/g; cp /tmp/f /home; cat /home/f | head -n 1; ls /home",
            "ln /home/f /home/f2; ls -l /home/f; chmod 600 /home/f; ls -l /home/f2; chmod u+x,go+r /home/f; ls -l /home/f",
            "basename /usr/lib/file.txt .txt; basename /x/y/; dirname /usr/lib/file; dirname file; dirname /",
            "seq 3 5; env -i A=1 B=2 env; printenv HOME; printenv NONE; echo printenv-status $?",
            "test -f /etc/motd && echo is-file; [ -d /etc ] && echo is-dir; [ 3 -gt 2 -a abc != abd ] && echo compound",
            "/bin/test 1 -eq 1; echo $?; /bin/[ x = y ]; echo $?",
            "yes trit | head -n 3",
            "uname; uname -s -m; date +%Y-%m-%d | wc -c",
            "sleep 30 & kill $!; wait; kill -0 $! 2>/dev/null || echo gone",
            "touch /tmp/d1; mkdir /tmp/dd; rmdir /tmp/dd; ls /tmp",
            "exit",
        ],
        700_000_000,
    );
    assert_has(
        &out,
        &[
            "1\n2\n3\n9\n10\n",
            "     10      10      21 /tmp/n\n     10\n",
            "4\n",
            "Cherry\napple\napple\nbanana\ndate\n",
            "date\nbanana\n",
            "1\n9\n10\n100\n",
            "      1 Cherry\n      2 apple\n      1 banana\n      1 date\n",
            "2:apple\n4:apple\n1\nCherry\nban",
            "date\nfound\nbanana\n",
            "banana\nf\n",
            "-rw-r--r-- 2 root     root          31 Sep 21 14:13 /home/f\n-rw------- 2 root     root          31 Sep 21 14:13 /home/f2\n-rwxr--r-- 2 root     root          31 Sep 21 14:13 /home/f\n",
            "file\ny\n/usr/lib\n.\n/\n",
            "3\n4\n5\nA=1\nB=2\n/root\nprintenv-status 1\n",
            "is-file\nis-dir\ncompound\n",
            "0\n1\n",
            "trit\ntrit\ntrit\n",
            "Trinux\nTrinux setun26\n      11\n",
            "gone\n",
            "d1\n",
        ],
    );
}

#[test]
fn k7_utility_errors() {
    let out = session(
        &[
            "echo x > /tmp/f; grep nope /tmp/f; echo status $?",
            "grep x /nope; echo status $?",
            "grep 'a[bc' /tmp/f; echo status $?",
            "grep; echo status $?",
            "head -n abc /tmp/f; echo status $?",
            "tail /nope; echo status $?",
            "sort /nope; echo status $?",
            "wc /nope /tmp/f; echo status $?",
            "cp /etc /tmp/x; echo status $?",
            "cp /tmp/f /tmp/f; echo status $?",
            "cp /tmp/f /etc/motd /tmp/f; echo status $?",
            "ln /etc /home/e; echo status $?",
            "ln /tmp/f /home/f; echo status $?",
            "chmod 999 /tmp/f; echo status $?",
            "chmod u+q /tmp/f; echo status $?",
            "chmod 644 /nope; echo status $?",
            "uname -q; echo status $?",
            "seq x; echo status $?",
            "env nosuchcmd; echo status $?",
            "/bin/kill 99999; echo status $?",
            "/bin/kill abc; echo status $?",
            "/bin/kill -BOGUS 1; echo status $?",
            "/bin/test 1 -eq x; echo status $?",
            "/bin/[ 1 = 1; echo status $?",
            "basename; echo status $?",
            "rmdir /tmp; echo status $?",
            "rmdir /nope; echo status $?",
            "date tomorrow; echo status $?",
            "exit",
        ],
        500_000_000,
    );
    assert_has(
        &out,
        &[
            "grep nope /tmp/f; echo status $?\nstatus 1\n",
            "grep: /nope: No such file or directory\nstatus 2\n",
            "grep: invalid pattern: a[bc\nstatus 2\n",
            "usage: grep [-ivcnq] pattern [file ...]\nstatus 2\n",
            "head: invalid number of lines: 'abc'\nstatus 1\n",
            "tail: /nope: No such file or directory\nstatus 1\n",
            "sort: /nope: No such file or directory\nstatus 2\n",
            "wc: /nope: No such file or directory\n      1       1       2 /tmp/f\n      1       1       2 total\nstatus 1\n",
            "cp: -r not supported; omitting directory '/etc'\nstatus 1\n",
            "cp: '/tmp/f' and '/tmp/f' are the same file\nstatus 1\n",
            "cp: target '/tmp/f' is not a directory\nstatus 1\n",
            "ln: /home/e: Operation not permitted\nstatus 1\n",
            "ln: /home/f: Invalid cross-device link\nstatus 1\n",
            "chmod: invalid mode: '999'\nstatus 1\n",
            "chmod: invalid mode: 'u+q'\nstatus 1\n",
            "chmod: /nope: No such file or directory\nstatus 1\n",
            "uname: invalid option -- 'q'\nstatus 1\n",
            "seq: invalid number: x\nstatus 1\n",
            "env: nosuchcmd: No such file or directory\nstatus 127\n",
            "kill: 99999: No such process\nstatus 1\n",
            "kill: invalid pid: abc\nstatus 1\n",
            "kill: invalid signal\nstatus 1\n",
            "test: integer expression expected: x\nstatus 2\n",
            "[: missing ']'\nstatus 2\n",
            "usage: basename path [suffix]\nstatus 1\n",
            "rmdir: /tmp: Device or resource busy\nstatus 1\n",
            "rmdir: /nope: No such file or directory\nstatus 1\n",
            "usage: date [-u] [-d date | -r file] [+format]\nstatus 1\n",
        ],
    );
}

#[test]
fn k7_libctest_passes() {
    let out = session(&["libctest", "exit"], 500_000_000);
    assert!(!out.contains("FAILED"), "{out}");
    assert_has(
        &out,
        &[
            "libctest: all passed",
            "memcpy by words ok",
            "memset by words ok",
            "memmove by words ok",
            "memcmp by words ok",
        ],
    );
    let n = out.matches(" ok\n").count();
    assert!(n >= 75, "only {n} checks:\n{out}");
}

#[test]
fn k7_clear_uses_the_vt100_subset() {
    let (out, m) = session_m(&["clear; echo TOP", "true"], 60_000_000);
    assert_has(&out, &["\u{1b}[H\u{1b}[2J"]);
    // The text screen: row 0 now starts with "TOP"; the boot messages are gone.
    let fb = m.mem.framebuffer();
    let row = |r: usize| -> String { (0..80).map(|c| char::from_u32(fb[3 * (r * 80 + c)] as u32).unwrap_or('?')).collect() };
    assert!(row(0).starts_with("TOP"), "row 0: {:?}", row(0));
    assert!(row(1).starts_with("$ true"), "row 1: {:?}", row(1));
    assert!(!(0..30).any(|r| row(r).contains("Trinux version")), "the screen was not cleared");
}

#[test]
fn calc_on_the_fpu() {
    let out = session(
        &[
            "calc '1/3'",
            "calc 2^0.5",
            "calc 'sin(pi/6)'",
            "calc '0t1T0 + 0zD'",
            "calc 'atan2(1, -1) * 4 / pi'",
            "calc '2^3^2'; calc '-2^2'; calc '7 % 3'",
            "calc 1/0; calc 'ln(-1)'; calc 'log(1000) + log3(1/27)'",
            "calc <<EOF\nx = 2*pi\nx / 4\n:t\n1/3\n0.5\nans*2\n-8\n:t\n:d 5\ne\n:q\n1\nEOF",
            "echo st=$?",
        ],
        600_000_000,
    );
    assert_has(
        &out,
        &[
            "$ calc '1/3'\n0.333333333333\n",
            "$ calc 2^0.5\n1.41421356237\n",
            "$ calc 'sin(pi/6)'\n0.5\n",
            "$ calc '0t1T0 + 0zD'\n19\n",
            "$ calc 'atan2(1, -1) * 4 / pi'\n3\n",
            "512\n-4\n1\n",
            "inf\nnan\n0\n",
            "6.28318530718\n1.57079632679\nternary on\n0.333333333333   0t0.1\n0.5   0t0.111111111111111111111111111...\n1   0t1\n-8   0tT01\nternary off\n2.7183\n$ echo st=$?\nst=0\n",
        ],
    );
}

#[test]
fn calc_errors() {
    let out = session(
        &[
            "calc '2 +'; echo st=$?",
            "calc 'foo(1)'; echo st=$?",
            "calc 'y + 1'; echo st=$?",
            "calc '0t12'; echo st=$?",
            "calc '0zE'; echo st=$?",
            "calc '(1+2'; echo st=$?",
            "calc 'pi = 3'; echo st=$?",
            "calc '1.5q'; echo st=$?",
            "calc '1 2'; echo st=$?",
            "calc <<EOF\n:x\n:d 0\nz = 1 +\nz\n2\nEOF",
            "echo st=$?",
        ],
        600_000_000,
    );
    assert_has(
        &out,
        &[
            "calc: unexpected end at column 4\nst=1\n",
            "calc: unknown function at column 1\nst=1\n",
            "calc: unknown variable at column 1\nst=1\n",
            "calc: bad 0t number (digits 1 0 T) at column 4\nst=1\n",
            "calc: bad 0z number (digits 0-9 A-D) at column 3\nst=1\n",
            "calc: expected ) at column 5\nst=1\n",
            "calc: cannot assign to a constant at column 1\nst=1\n",
            "calc: bad number at column 4\nst=1\n",
            "calc: syntax error at column 3\nst=1\n",
            // a failed assignment makes no variable; later lines still run
            "calc: commands are :t :d N :q\ncalc: digits are 1..12\ncalc: unexpected end at column 8\ncalc: unknown variable at column 1\n2\n$ echo st=$?\nst=1\n",
        ],
    );
}

#[test]
fn exec_sees_a_rewritten_program_not_its_cached_text() {
    // exec keeps text frames after a program exits; overwriting, truncating
    // or replacing the file must not run the old code
    let out = session(
        &[
            "cp /bin/echo /tmp/p; /tmp/p one; /tmp/p two",
            "cp /bin/true /tmp/p; /tmp/p three; echo st=$?",
            "cat /bin/echo > /tmp/p; /tmp/p four",
            "rm /tmp/p; cp /bin/false /tmp/p; /tmp/p five; echo st=$?",
            "/bin/echo six",
            "exit",
        ],
        400_000_000,
    );
    assert_has(&out, &["one\n", "two\n", "st=0\n", "four\n", "st=1\n", "six\n"]);
    assert_lacks(&out, &["three\n", "five\n"]); // (the echoed command lines have "three;")
}


#[test]
fn memory_runs_short_and_the_text_cache_gives_its_frames_back() {
    // ten programs run and exit (their text stays cached), then children
    // that need nearly all memory at once: free + (-22) pages of heap, plus
    // their own tables and stacks. Measured here: up to about free - 30
    // fits without the cache's frames, up to about free - 15 with them.
    let out = session(
        &[
            "ls /etc > /dev/null; wc /etc/motd > /dev/null; sort /etc/motd > /dev/null; head /etc/motd > /dev/null",
            "tail /etc/motd > /dev/null; uniq /etc/motd > /dev/null; grep a /etc/motd > /dev/null; df > /dev/null",
            "md5sum /etc/motd > /dev/null; ps > /dev/null",
            "k3test hog -22",
            "exit",
        ],
        1_500_000_000,
    );
    assert_has(&out, &["k3test: hog touched every page\n"]);
}

#[test]
fn sh_backquotes_and_prompt_expansion() {
    let out = session(
        &[
            // `...` as $(...): split unquoted, kept whole in quotes, escapes
            "echo `echo a  b` c",
            "for w in `echo 1 2 3`; do echo w$w; done",
            r#"echo "[`echo '1   2'`]" `echo '1   2'`"#,
            r#"X=v; echo `echo \$X` `echo \`echo deep\``"#,
            r#"echo "`echo \"q\"`" `echo 'it''s'`"#,
            "echo `nosuchcmd`; echo st=$?",
            // the prompt is expanded each time it is shown
            r"X=one; PS1='<\w|$X|$(echo sub)|\u|\W> $ '",
            "cd /usr/lib",
            "X=two",
            // $? in the prompt is the last command's, and the prompt's own
            // command substitution leaves it alone
            "PS1='s=$?:`true` $ '",
            "false",
            "echo st=$?",
            r"PS1='\$x\nnext $ '",
            "echo two-lines",
            "\u{1b}[A", // Up: the redraw repeats only the prompt's last line
            // an unfinished `...` continues on the next line
            "echo `echo open",
            "echo shut`",
            // double quotes expand once, at the assignment (as in bash)
            r#"PS1="$(pwd) $ ""#,
            "cd /",
            "exit",
        ],
        500_000_000,
    );
    assert_has(
        &out,
        &[
            "\na b c\n",
            "\nw1\nw2\nw3\n",
            "\n[1   2] 1 2\n",
            "\nv deep\n",
            "\nq its\n",
            "nosuchcmd: not found",
            "\n\nst=0\n",
            "\n<~|one|sub|root|~> $ cd /usr/lib\n</usr/lib|one|sub|root|lib> $ X=two\n</usr/lib|two|sub|root|lib> $ ",
            "\ns=0: $ false\ns=1: $ echo st=$?\nst=1\ns=0: $ ",
            "\n#x\nnext $ echo two-lines\ntwo-lines\n#x\nnext $ ",
            "\n#x\nnext $ \rnext $ echo two-lines\u{1b}[K\ntwo-lines\n",
            "\n> echo shut`\nopen shut\n",
            "\n/usr/lib $ cd /\n/usr/lib $ exit",
        ],
    );
}

#[test]
fn mutex_futex_and_shared_memory() {
    // TERNARY_WORLD A4: /bin/mtxtest (shared anonymous memory across fork,
    // freed with its last sharer; futex wait/wake and their errors; the
    // one-trit mutex under preemption, against a run without it)
    let out = session(&["mtxtest", "exit"], 3_000_000_000);
    assert!(!out.contains("FAILED"), "{out}");
    assert_has(&out, &["mtxtest: all passed", "with the mutex, 4 x 300 increments make 1200 ok", "the race is real) ok"]);
    let n = out.matches(" ok\n").count();
    assert!(n >= 18, "only {n} checks:\n{out}");
}

#[test]
fn vector_unit_from_user_mode() {
    // TERNARY_WORLD A5: /bin/vectest (the Trine vector builtins; lazy
    // switching keeps each process's v registers across switches, fork,
    // execve and signal handlers; the faults arrive as signals)
    let out = session(&["vectest", "echo st=$?", "grep Vector /proc/1/status", "exit"], 3_000_000_000);
    assert!(!out.contains("FAILED") && !out.contains("Kernel panic"), "{out}");
    for check in [
        "first use: the registers start zeroed",
        "lane-wise operations, reductions",
        "every builtin, tryte and word lanes, fills and a merge",
        "registers survive switching (parent)",
        "registers survive switching (child)",
        "fork copies the vector registers",
        "execve resets the vector registers",
        "handler starts with clean vector registers",
        "interrupted vector state restored",
        "a misaligned VLD is SIGBUS",
        "a VST into read-only memory is SIGSEGV",
        "an unknown vector function is SIGILL",
        "parent unaffected",
    ] {
        assert!(out.contains(&format!("vectest: {check} ok\n")), "missing {check:?} in:\n{out}");
    }
    assert_has(&out, &["vectest: all passed", "st=0\n", "Vector:\tunused\n"]);
}

#[test]
fn text_beyond_one_tryte() {
    // TERNARY_WORLD A6: /bin/chartest (3-tryte characters, tstr_next,
    // tstr_chars, tchar_put, UTF-8 both ways, the malformed forms), and the
    // console carrying them both ways: typed in, echoed back
    // (head reads through the kernel's line discipline: Backspace there
    // takes 本 whole)
    let out = session(&["chartest", "echo st=$?", "echo 日本̅ 😀", "head -n 1 > /tmp/h", "~日本\u{7f}x\n", "cat /tmp/h; echo end", "exit"], 3_000_000_000);
    assert_has(&out, &["\n日x\nend\n"]);
    assert!(!out.contains("FAILED"), "{out}");
    assert_has(&out, &["chartest: all passed", "st=0", "chartest: 日本語 Ω̅ 😀 ok", "日本̅ 😀"]);
    let n = out.matches(" ok\n").count();
    assert!(n >= 18, "only {n} checks:\n{out}");
}

#[test]
fn ternary_data_structures() {
    // TERNARY_WORLD phase A: /bin/dstest (Kleene vectors and packed flags,
    // the 3-ary heap, the 27-key B-tree, each against a plain model)
    let out = session(&["dstest", "echo st=$?", "exit"], 6_000_000_000);
    assert!(!out.contains("FAILED"), "{out}");
    assert_has(&out, &["dstest: all passed", "st=0", "3000 keys in order: 3 levels ok"]);
    let n = out.matches(" ok\n").count();
    assert!(n >= 25, "only {n} checks:\n{out}");
}
