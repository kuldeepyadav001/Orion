#!/usr/bin/env node
//
// Check the IPC contract between the frontend and the backend.
//
// WHY THIS EXISTS
//
// The application has never been run: there is no display server in the
// build environment, so the window, the tray and the hotkey have never
// been seen. A Tauri command that the frontend calls with the wrong
// argument name fails at *runtime*, not at compile time, and the
// frontend's catch block usually swallows it. That is how the email
// assistant's "generate reply" button ended up silently falling back to
// a canned template for months.
//
// This checker reads both sides and compares them:
//
//   - every command the frontend invokes must be registered in the
//     Tauri handler;
//   - every argument the frontend passes must match a parameter of the
//     command, in Tauri v2's camelCase convention;
//   - every required parameter must be supplied;
//   - every event the frontend listens for must be emitted by the
//     backend (Tauri's own tauri:// events are exempt).
//
// Usage:
//   node scripts/check-ipc.mjs
//
// Exit code 1 when something does not line up.

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const RS_DIR = join(ROOT, "src-tauri", "src");
const JS_DIR = join(ROOT, "src");

function walk(dir, exts) {
  const out = [];
  let entries;
  try {
    entries = readdirSync(dir);
  } catch {
    return out;
  }
  for (const e of entries) {
    const p = join(dir, e);
    const st = statSync(p);
    if (st.isDirectory()) out.push(...walk(p, exts));
    else if (exts.some((x) => p.endsWith(x))) out.push(p);
  }
  return out;
}

const rsFiles = walk(RS_DIR, [".rs"]);
const jsFiles = walk(JS_DIR, [".jsx", ".js"]);

const read = (p) => readFileSync(p, "utf-8");

/// Split a parameter list on top-level commas.
function splitTop(text) {
  const parts = [];
  let depth = 0;
  let current = "";
  for (const ch of text) {
    if ("([{<".includes(ch)) depth++;
    else if (")]}>".includes(ch)) depth--;
    if (ch === "," && depth === 0) {
      parts.push(current);
      current = "";
    } else {
      current += ch;
    }
  }
  if (current.trim()) parts.push(current);
  return parts.map((s) => s.trim()).filter(Boolean);
}

/// The matching close parenthesis for the one at `open`.
function matchParen(text, open) {
  let depth = 0;
  for (let i = open; i < text.length; i++) {
    if (text[i] === "(") depth++;
    else if (text[i] === ")") {
      depth--;
      if (depth === 0) return i;
    }
  }
  return -1;
}

function toCamel(snake) {
  return snake.replace(/_([a-z0-9])/g, (_, c) => c.toUpperCase());
}

// --- the backend ---------------------------------------------------------

/** command name -> { params: [{name, required}], file } */
const commands = new Map();

for (const file of rsFiles) {
  const src = read(file);
  const re = /#\[tauri::command\]/g;
  let m;
  while ((m = re.exec(src)) !== null) {
    const after = src.slice(m.index, m.index + 400);
    const fnMatch = after.match(/\b(?:pub\s+)?(?:async\s+)?fn\s+([a-z_0-9]+)\s*\(/);
    if (!fnMatch) continue;
    const name = fnMatch[1];
    const parenAt = m.index + fnMatch.index + fnMatch[0].indexOf("(");
    const close = matchParen(src, parenAt);
    if (close < 0) continue;
    const raw = src.slice(parenAt + 1, close);
    const params = [];
    for (const p of splitTop(raw)) {
      const colon = p.indexOf(":");
      if (colon < 0) continue;
      const pname = p.slice(0, colon).trim();
      const ptype = p.slice(colon + 1).trim();
      // The injected arguments are not the caller's business.
      if (/AppHandle|State\s*<|Window|Webview/i.test(ptype)) continue;
      const required = !/^Option\s*</.test(ptype) && !pname.endsWith("_");
      params.push({ name: pname, camel: toCamel(pname), required });
    }
    commands.set(name, { params, file: file.slice(ROOT.length + 1) });
  }
}

// The registered handler list.
const libSrc = read(join(RS_DIR, "lib.rs"));
const handlerMatch = libSrc.match(/generate_handler!\s*\[([\s\S]*?)\]/);
const registered = new Set();
if (handlerMatch) {
  for (const m of handlerMatch[1].matchAll(/([a-z_0-9]+)/g)) registered.add(m[1]);
}

// The events the backend emits.
const emitted = new Set();
for (const file of rsFiles) {
  const src = read(file);
  for (const m of src.matchAll(/\.emit\(\s*"([^"]+)"/g)) emitted.add(m[1]);
  for (const m of src.matchAll(/\.emit\(\s*"([^"]+)"\s*,/g)) emitted.add(m[1]);
}

// --- the frontend --------------------------------------------------------

/** command -> [{key, file, line}] */
const invoked = new Map();
const listened = new Set();

for (const file of jsFiles) {
  const src = read(file);
  const rel = file.slice(ROOT.length + 1);

  for (const m of src.matchAll(/invoke\(\s*"([a-z_0-9]+)"/g)) {
    const cmd = m[1];
    const args = [];
    // The optional second argument: an object literal.
    const after = src.slice(m.index + m[0].length);
    const objStart = after.search(/\s*\{/);
    if (objStart >= 0 && objStart < 40) {
      const braceAt = m.index + m[0].length + after.indexOf("{", objStart);
      let depth = 0;
      let end = -1;
      for (let i = braceAt; i < src.length; i++) {
        if (src[i] === "{") depth++;
        else if (src[i] === "}") {
          depth--;
          if (depth === 0) {
            end = i;
            break;
          }
        }
      }
      if (end > 0) {
        const body = src.slice(braceAt + 1, end);
        let d = 0;
        let chunk = "";
        const chunks = [];
        for (const ch of body) {
          if ("([{".includes(ch)) d++;
          else if (")]}".includes(ch)) d--;
          if (ch === "," && d === 0) {
            chunks.push(chunk);
            chunk = "";
          } else {
            chunk += ch;
          }
        }
        if (chunk.trim()) chunks.push(chunk);
        for (const c of chunks) {
          const t = c.trim();
          if (!t) continue;
          const km = t.match(/^["']?([A-Za-z_$][\w$]*)["']?\s*:/);
          if (km) {
            args.push({ key: km[1], file: rel, line: src.slice(0, m.index).split("\n").length });
          } else {
            const sm = t.match(/^([A-Za-z_$][\w$]*)\s*$/);
            if (sm) {
              args.push({
                key: sm[1],
                file: rel,
                line: src.slice(0, m.index).split("\n").length,
              });
            }
          }
        }
      }
    }
    if (!invoked.has(cmd)) invoked.set(cmd, []);
    invoked.get(cmd).push(...args);
  }

  for (const m of src.matchAll(/listen\(\s*"([^"]+)"/g)) listened.add(m[1]);
}

// --- the comparison ------------------------------------------------------

const problems = [];

for (const [cmd, args] of [...invoked].sort()) {
  if (!registered.has(cmd)) {
    problems.push(`the frontend invokes "${cmd}", which is not in generate_handler!`);
    continue;
  }
  const def = commands.get(cmd);
  if (!def) continue;
  for (const a of args) {
    if (!def.params.some((p) => p.camel === a.key)) {
      problems.push(
        `${a.file}: invoke("${cmd}") passes "${a.key}", which the backend does not accept ` +
          `(it accepts ${def.params.map((p) => p.camel).join(", ") || "nothing"})`
      );
    }
  }
  const supplied = new Set(args.map((a) => a.key));
  for (const p of def.params) {
    if (p.required && !supplied.has(p.camel)) {
      problems.push(
        `invoke("${cmd}") never supplies the required parameter "${p.camel}" (${def.file})`
      );
    }
  }
}

for (const event of [...listened].sort()) {
  // Tauri emits its own drag-and-drop and lifecycle events in the webview.
  if (event.startsWith("tauri://")) continue;
  if (!emitted.has(event)) {
    problems.push(`the frontend listens for "${event}", which the backend never emits`);
  }
}

// --- the report ----------------------------------------------------------

console.log(`backend commands : ${commands.size}`);
console.log(`registered       : ${registered.size}`);
console.log(`frontend invokes : ${invoked.size}`);
console.log(`events emitted   : ${emitted.size}, listened for: ${listened.size}`);
console.log();

if (problems.length === 0) {
  console.log("IPC contract OK: every command the frontend calls exists, every");
  console.log("argument matches a parameter, and every event is emitted.");
  process.exit(0);
}

console.log(`${problems.length} problem(s):`);
for (const p of problems) console.log(`  - ${p}`);
process.exit(1);
