// Vite's library build wraps the worker's URL (new Worker(new URL(new URL("assets/…", import.meta.url).href, …))),
// which a host's bundler takes for a plain asset: it copies (or inlines) the worker without resolving the worker's
// import of @draftwright/specify-pmi-wasm. Rewritten to the standard form, the host's bundler (Vite, webpack 5)
// builds the worker as an entry of its own and resolves the engine and its .wasm, so a host need pass nothing.
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';

// The worker is created in the chunk the entries share (the engine client), wherever Vite puts it.
const wrapped = /new Worker\(\s*new URL\(\s*(?:\/\*\s*@vite-ignore\s*\*\/\s*)?new URL\("assets\/intent\.worker\.js", import\.meta\.url\)\.href,\s*"" \+ import\.meta\.url\s*\)/g;
const standard = 'new Worker(new URL("./assets/intent.worker.js", import.meta.url), { type: "module" })';
let found = 0;
for (const name of readdirSync('dist').filter(n => n.endsWith('.js'))) {
  const file = `dist/${name}`;
  const text = readFileSync(file, 'utf8');
  const out = text.replace(wrapped, 'new Worker(new URL("./assets/intent.worker.js", import.meta.url)');
  const count = out.match(/new Worker\(/g)?.length ?? 0;
  if (count === 0) continue;
  if (count !== 1 || !out.includes(standard)) {
    throw new Error(`${file}: the intent worker's URL is not in the form a host's bundler builds as a worker; check the Vite output.`);
  }
  writeFileSync(file, out);
  found++;
}
if (found !== 1) throw new Error(`expected the intent worker's URL in one dist chunk, found it in ${found}`);
