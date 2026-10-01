//! Trinux K7: the shell language, the core utilities and tlibc, each with
//! its error cases.

/// Boot the default disk and type each line at a prompt ("$ " or the
/// continuation prompt "> "). Returns the transcript and the machine.
fn session_m(lines: &[&str], max_steps: u64) -> (String, setun_core::Machine) {
    let disk = trinux::build_disk(&trinux::kernel_dir()).unwrap_or_else(|e| panic!("{e}"));
    let mut m = trinux::boot_trytes(&disk.trytes, 729, 0, "");
    let mut out = String::new();
    let (mut next, mut steps) = (0, 0);
    while steps < max_steps && !m.powered_off {
        m.run(1_000_000);
        steps += 1_000_000;
        out.push_str(&m.dev.serial.take_output());
        if out.contains("Kernel panic") {
            break;
        }
        if next < lines.len() && (out.ends_with("$ ") || out.ends_with("> ")) {
            m.dev.serial_input(&format!("{}\n", lines[next]));
            next += 1;
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
            "$ echo ~ ~/x\n/ //x\n",
            "FOO=prefix\nafter: []\n",
            "child sees exported\nchild sees []\n",
            "<Welcome to Trinux on Setun-386.>\n<Everything here is a power of 3.>\n",
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
            "init: /bin/sh exited with 4",
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
            "init: /bin/sh exited with 5",
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
            "init: /bin/sh exited with 6",
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
            "-rw-r--r-- 2 root root      31 /home/f\n-rw------- 2 root root      31 /home/f2\n-rwxr--r-- 2 root root      31 /home/f\n",
            "file\ny\n/usr/lib\n.\n/\n",
            "3\n4\n5\nA=1\nB=2\n/\nprintenv-status 1\n",
            "is-file\nis-dir\ncompound\n",
            "0\n1\n",
            "trit\ntrit\ntrit\n",
            "Trinux\nTrinux setun386\n      11\n",
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
            "usage: date [+format]\nstatus 1\n",
        ],
    );
}

#[test]
fn k7_libctest_passes() {
    let out = session(&["libctest", "exit"], 500_000_000);
    assert!(!out.contains("FAILED"), "{out}");
    assert_has(&out, &["libctest: all passed"]);
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
