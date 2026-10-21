// The browser build's network card through a relay, under node: boot
// Trinux with the link up, carry its frames over a WebSocket to setun-relay
// as web/emu.html does, log in on the serial console and check that names
// resolve and the web answers, and that the relay's own host and a private
// address are refused. Usage: node tools/wnet.mjs [web/setun.wasm] [ws://127.0.0.1:8027/]
import fs from 'fs';

const path = process.argv[2] || 'web/setun.wasm';
const url = process.argv[3] || 'ws://127.0.0.1:8027/';
const { instance } = await WebAssembly.instantiate(fs.readFileSync(path), {});
const e = instance.exports;

const ws = new WebSocket(url);
ws.binaryType = 'arraybuffer';
const inbox = [];
let sent = 0, got = 0;
ws.onmessage = (m) => { inbox.push(new Uint8Array(m.data)); got++; };
await new Promise((ok, fail) => { ws.onopen = ok; ws.onerror = () => fail(new Error('no relay at ' + url)); });

e.setun_net_link(1);
e.setun_boot(0, 2187, Date.now() / 1000);
const dec = new TextDecoder();
let out = '';
const script = [
  'root', 'root',
  'host example.com; echo host-status $?',
  'wget -q -O - http://example.com/ | head -n 6; echo wget-status $?',
  'nc -z -w 3 192.168.1.1 80; echo private-status $?',
  'nc -z -w 3 10.0.2.2 22; echo gateway-status $?',
  'exit',
];
let next = 0;
const t0 = Date.now();
while (Date.now() - t0 < 120000) {
  e.setun_run(300000);
  for (;;) {
    const n = e.setun_net_take();
    if (!n) break;
    ws.send(new Uint8Array(e.memory.buffer, e.setun_net_ptr(), n).slice());
    sent++;
  }
  while (inbox.length) {
    const f = inbox.shift();
    new Uint8Array(e.memory.buffer, e.setun_net_buf(), f.length).set(f);
    e.setun_net_receive(f.length);
  }
  const n = e.setun_serial_take();
  if (n) out += dec.decode(new Uint8Array(e.memory.buffer, e.setun_serial_ptr(), n));
  const prompt = next < 2 ? (next === 0 ? 'login: ' : 'Password: ') : '$ ';
  if (next < script.length && out.endsWith(prompt)) {
    for (const ch of script[next] + '\n') e.setun_serial_char(ch.codePointAt(0));
    next++;
  }
  if (next === script.length && out.includes('login session exited')) break;
  await new Promise((r) => setImmediate(r));       // let the socket's messages in
}
ws.close();
const after = out.slice(out.indexOf('host example.com'));
console.log(after);
console.log(`frames: ${sent} out, ${got} in`);
const status = (name) => Number((after.match(new RegExp(name + '-status (\\d+)')) || [])[1]);
const checks = [
  ['the link came up at boot', out.includes('link up')],
  ['a name resolves through the relay', status('host') === 0 && /has address/.test(after)],
  ['the web answers through the relay', status('wget') === 0 && /Example Domain/.test(after)],
  ['a private address is refused', status('private') !== 0 && !Number.isNaN(status('private'))],
  ["the relay's own host (10.0.2.2) is refused", status('gateway') !== 0 && !Number.isNaN(status('gateway'))],
];
let bad = 0;
for (const [what, ok] of checks) { console.log((ok ? 'ok   ' : 'FAIL ') + what); if (!ok) bad++; }
process.exit(bad ? 1 : 0);
