// Web Worker hosting the gedlint engine (issue 15, RFC 014 section 9).
//
// A large GEDCOM must never block the main thread: the files that most need
// checking are the big broken ones. The engine is the wasm32 cdylib exposed
// through a plain C ABI (src/wasm.rs); this file is the only JS that touches
// wasm memory.
//
// Protocol (page -> worker):
//   {type:'check',  id, data: ArrayBuffer, config: string}
//   {type:'registry', id}
//   {type:'edits',  id, data: ArrayBuffer, config: string}
//   {type:'apply',  id, data: ArrayBuffer, mask: ArrayBuffer, config: string}
//   {type:'baseline', id, data: ArrayBuffer, config: string}
//   {type:'baseline-match', id, data: ArrayBuffer, baseline: string, config: string}
// 'edits' and 'apply' must receive the same config string the 'check' used:
// apply matches its mask against the edit list by index, so a different
// config would select the wrong repairs. 'baseline-match' pairs the same
// way: its known flags index the diagnostics list of the check run, so it
// needs the same bytes and the same config.
// (worker -> page)
//   {type:'ready'} | {type:'phase', label} | {id, ok, ...} | {id, ok:false, error}
//
// Memory rules mirror src/wasm.rs: buffers cross as (ptr, len); the callee
// borrows them for the call only; every return is a 16-byte header
// [u64 LE ptr][u64 LE len] that we free along with its payload. Wasm memory
// can grow (detaching the old ArrayBuffer) on any call, so views are always
// rebuilt after a call returns.
'use strict';

importScripts('gedlint-wasm.js');

let ex = null; // wasm instance exports

function b64ToBytes(b64) {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

async function boot() {
  try {
    const { instance } = await WebAssembly.instantiate(b64ToBytes(GEDLINT_WASM_B64), {});
    ex = instance.exports;
    self.postMessage({ type: 'ready' });
  } catch (e) {
    self.postMessage({ type: 'boot-error', error: String(e) });
  }
}

// Copy a return value out of wasm memory and free both allocations.
function takeReturn(ret) {
  const head = new DataView(ex.memory.buffer, ret, 16);
  const ptr = Number(head.getBigUint64(0, true));
  const len = Number(head.getBigUint64(8, true));
  const out = new Uint8Array(ex.memory.buffer, ptr, len).slice();
  ex.gedlint_dealloc(ret, 16);
  ex.gedlint_dealloc(ptr, len);
  return out;
}

// Write bytes into wasm memory; returns 0 for empty (callee treats len 0).
function writeIn(bytes) {
  if (!bytes.length) return 0;
  const ptr = ex.gedlint_alloc(bytes.length);
  new Uint8Array(ex.memory.buffer, ptr, bytes.length).set(bytes);
  return ptr;
}

function phase(label) {
  self.postMessage({ type: 'phase', label });
}

function reply(msg, payload) {
  self.postMessage(Object.assign({ id: msg.id, ok: true }, payload), payload.transfer || []);
}

function fail(msg, error) {
  self.postMessage({ id: msg.id, ok: false, error: String(error) });
}

self.onmessage = (ev) => {
  const m = ev.data;
  if (!ex) {
    fail(m, 'the engine is not loaded yet');
    return;
  }
  try {
    switch (m.type) {
      case 'registry': {
        reply(m, { json: new TextDecoder().decode(takeReturn(ex.gedlint_registry())) });
        break;
      }
      case 'check': {
        phase('analyzing');
        const data = new Uint8Array(m.data);
        const cfg = new TextEncoder().encode(m.config || '');
        const dp = writeIn(data);
        const cp = writeIn(cfg);
        const ret = ex.gedlint_lint(dp, data.length, cp, cfg.length);
        if (dp) ex.gedlint_dealloc(dp, data.length);
        if (cp) ex.gedlint_dealloc(cp, cfg.length);
        reply(m, { json: new TextDecoder().decode(takeReturn(ret)) });
        break;
      }
      case 'edits': {
        phase('computing repairs');
        const data = new Uint8Array(m.data);
        const cfg = new TextEncoder().encode(m.config || '');
        const dp = writeIn(data);
        const cp = writeIn(cfg);
        const ret = ex.gedlint_edits(dp, data.length, cp, cfg.length);
        if (dp) ex.gedlint_dealloc(dp, data.length);
        if (cp) ex.gedlint_dealloc(cp, cfg.length);
        reply(m, { json: new TextDecoder().decode(takeReturn(ret)) });
        break;
      }
      case 'apply': {
        phase('applying repairs');
        const data = new Uint8Array(m.data);
        const mask = new Uint8Array(m.mask);
        const cfg = new TextEncoder().encode(m.config || '');
        const dp = writeIn(data);
        const mp = writeIn(mask);
        const cp = writeIn(cfg);
        const ret = ex.gedlint_apply(dp, data.length, mp, mask.length, cp, cfg.length);
        if (dp) ex.gedlint_dealloc(dp, data.length);
        if (mp) ex.gedlint_dealloc(mp, mask.length);
        if (cp) ex.gedlint_dealloc(cp, cfg.length);
        // Payload layout: [u64 LE json_len][summary JSON][repaired file bytes].
        const payload = takeReturn(ret);
        const jsonLen = Number(new DataView(payload.buffer, 0, 8).getBigUint64(0, true));
        const json = new TextDecoder().decode(payload.subarray(8, 8 + jsonLen));
        const file = payload.slice(8 + jsonLen); // own buffer, transferable
        reply(m, { json, file, transfer: [file.buffer] });
        break;
      }
      case 'baseline': {
        phase('writing the baseline');
        const data = new Uint8Array(m.data);
        const cfg = new TextEncoder().encode(m.config || '');
        const dp = writeIn(data);
        const cp = writeIn(cfg);
        const ret = ex.gedlint_baseline(dp, data.length, cp, cfg.length);
        if (dp) ex.gedlint_dealloc(dp, data.length);
        if (cp) ex.gedlint_dealloc(cp, cfg.length);
        // The payload is the baseline file text itself, ready to download.
        reply(m, { json: new TextDecoder().decode(takeReturn(ret)) });
        break;
      }
      case 'baseline-match': {
        phase('matching the baseline');
        const data = new Uint8Array(m.data);
        const baseline = new TextEncoder().encode(m.baseline || '');
        const cfg = new TextEncoder().encode(m.config || '');
        const dp = writeIn(data);
        const bp = writeIn(baseline);
        const cp = writeIn(cfg);
        const ret = ex.gedlint_baseline_match(dp, data.length, bp, baseline.length, cp, cfg.length);
        if (dp) ex.gedlint_dealloc(dp, data.length);
        if (bp) ex.gedlint_dealloc(bp, baseline.length);
        if (cp) ex.gedlint_dealloc(cp, cfg.length);
        reply(m, { json: new TextDecoder().decode(takeReturn(ret)) });
        break;
      }
      default:
        fail(m, 'unknown message type ' + m.type);
    }
  } catch (e) {
    fail(m, e);
  }
};

boot();
