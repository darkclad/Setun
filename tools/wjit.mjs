// The browser build's JIT under node: boot Trinux, log in, run commands,
// with the JIT off and on; the transcripts must match, and the times show
// what the JIT gains (synchronous linking, and asynchronous as the page does
// it). Usage: node tools/wjit.mjs [web/setun.wasm]
import fs from 'fs';

const path = process.argv[2] || 'target/wasm32-unknown-unknown/release/setun_web.wasm';
const bytes = fs.readFileSync(path);
const mod = await WebAssembly.compile(bytes);

async function session(jit) {
  const { exports: e } = await WebAssembly.instantiate(mod, {});
  const table = e.__indirect_function_table;
  let compiled = 0, failed = 0, compileMs = 0;
  function service() {
    for (;;) {
      const n = e.setun_jit_take();
      if (!n) return;
      const code = new Uint8Array(e.memory.buffer, e.setun_jit_ptr(), n).slice();
      const id = e.setun_jit_id();
      const t = performance.now();
      try {
        const m = new WebAssembly.Module(code);
        const inst = new WebAssembly.Instance(m, { env: { memory: e.memory, table } });
        const idx = table.grow(1);
        table.set(idx, inst.exports.run);
        e.setun_jit_done(id, idx);
        compiled++;
      } catch (err) {
        if (failed++ < 3) {
          console.log('module', id, 'failed:', err.message);
          fs.writeFileSync(`target/wjit-bad-${id}.wasm`, code);
        }
        e.setun_jit_done(id, 0);
      }
      compileMs += performance.now() - t;
    }
  }
  // as the page does it: compiled in the background, linked when ready
  let inflight = 0;
  function serviceAsync() {
    for (;;) {
      const n = e.setun_jit_take();
      if (!n) return;
      const code = new Uint8Array(e.memory.buffer, e.setun_jit_ptr(), n).slice();
      const id = e.setun_jit_id();
      inflight++;
      WebAssembly.instantiate(code, { env: { memory: e.memory, table } })
        .then(({ instance }) => { const idx = table.grow(1); table.set(idx, instance.exports.run); e.setun_jit_done(id, idx); compiled++; })
        .catch((err) => { failed++; console.log('async module failed', err.message); e.setun_jit_done(id, 0); })
        .finally(() => inflight--);
    }
  }
  e.setun_jit_enable(jit ? 1 : 0);
  e.setun_boot(0, 2187, 1790000000);
  const dec = new TextDecoder();
  let out = '', steps = 0;
  const script = ['root', 'root', 'i=0; while [ $i -lt 20 ]; do md5sum /bin/sh > /dev/null; i=$((i+1)); done; echo loops $i', 'md5sum /etc/passwd /etc/motd', 'ls /bin | wc -l', 'calc "2^20 + 7"', 'sort /etc/passwd | head -n 3', 'exit'];
  let next = 0;
  const t0 = performance.now();
  while (steps < 4e9) {
    e.setun_run(1000000); steps += 1000000;
    if (jit === 'async') { serviceAsync(); await new Promise((r) => setImmediate(r)); }
    else if (jit) service();
    const n = e.setun_serial_take();
    if (n) out += dec.decode(new Uint8Array(e.memory.buffer, e.setun_serial_ptr(), n));
    const prompt = next < 2 ? (next === 0 ? 'login: ' : 'Password: ') : '$ ';
    if (next < script.length && out.endsWith(prompt)) {
      for (const ch of script[next] + '\n') e.setun_serial_char(ch.codePointAt(0));
      next++;
    }
    if (next === script.length && out.includes('login session exited')) break;
  }
  const secs = (performance.now() - t0) / 1000;
  return { out, steps, secs, compiled, failed, compileMs };
}

const off = await session(false);
console.log(`JIT off: ${off.steps} steps, ${off.secs.toFixed(2)} s, ${(off.steps / off.secs / 1e6).toFixed(1)} MIPS`);
const on = await session(true);
const as = await session('async');
console.log(`JIT async: ${as.steps} steps, ${as.secs.toFixed(2)} s, ${(as.steps / as.secs / 1e6).toFixed(1)} MIPS; ${as.compiled} modules (${as.failed} failed)`);
if (as.out !== off.out) { console.log('ASYNC TRANSCRIPT DIFFERS'); process.exit(1); }
console.log(`JIT on:  ${on.steps} steps, ${on.secs.toFixed(2)} s, ${(on.steps / on.secs / 1e6).toFixed(1)} MIPS; ${on.compiled} modules (${on.failed} failed), compiling ${on.compileMs.toFixed(0)} ms`);
if (off.out !== on.out) {
  let i = 0;
  while (i < off.out.length && off.out[i] === on.out[i]) i++;
  console.log('TRANSCRIPTS DIFFER at', i, '\n--- off:\n' + off.out.slice(Math.max(0, i - 200), i + 300) + '\n--- on:\n' + on.out.slice(Math.max(0, i - 200), i + 300));
  process.exit(1);
}
console.log('transcripts match:', on.out.length, 'chars');
console.log(on.out.slice(-600));
