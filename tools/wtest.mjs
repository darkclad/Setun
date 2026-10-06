// crates/core/tests/fast_path.rs in WebAssembly (crates/wtest): each test in
// a fresh instance, the JIT's modules compiled synchronously when queued.
// Build: cargo build --release -p setun-wtest --target wasm32-unknown-unknown
// Usage: node tools/wtest.mjs [name filter]
import fs from 'fs';
const mod = await WebAssembly.compile(fs.readFileSync('target/wasm32-unknown-unknown/release/setun_wtest.wasm'));
const filter = process.argv[2] || '';
const dec = new TextDecoder();
let e, modules = 0, bad = 0;
const imports = { env: { setun_jit_service() {
  const table = e.__indirect_function_table;
  for (;;) {
    const n = e.wtest_jit_take();
    if (!n) return;
    const code = new Uint8Array(e.memory.buffer, e.wtest_jit_ptr(), n).slice();
    const id = e.wtest_jit_id();
    try {
      const inst = new WebAssembly.Instance(new WebAssembly.Module(code), { env: { memory: e.memory, table } });
      const idx = table.grow(1);
      table.set(idx, inst.exports.run);
      e.wtest_jit_done(id, idx);
      modules++;
    } catch (err) {
      if (bad++ < 3) { console.log('  module failed:', err.message); fs.writeFileSync(`target/wtest-bad-${bad}.wasm`, code); }
      e.wtest_jit_done(id, 0);
    }
  }
} } };
const first = (await WebAssembly.instantiate(mod, imports)).exports;
const count = first.wtest_count();
let pass = 0, fail = 0;
for (let i = 0; i < count; i++) {
  e = (await WebAssembly.instantiate(mod, imports)).exports;
  const name = dec.decode(new Uint8Array(e.memory.buffer, e.wtest_name_ptr(i), e.wtest_name_len(i)));
  if (!name.includes(filter)) continue;
  const m0 = modules;
  try {
    e.wtest_run(i);
    pass++;
    console.log(`ok    ${name} (${modules - m0} modules)`);
  } catch (err) {
    fail++;
    const msg = dec.decode(new Uint8Array(e.memory.buffer, e.wtest_msg_ptr(), e.wtest_msg_len()));
    console.log(`FAIL  ${name}: ${msg || err.message}`);
  }
}
console.log(`${pass} passed, ${fail} failed; ${modules} modules compiled, ${bad} failed to compile`);
process.exit(fail || bad ? 1 : 0);
