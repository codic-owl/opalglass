// Capture the installed reference renderer in an isolated vault/profile.
// Usage: node scripts/capture_obsidian_reference.mjs /path/to/obsidian [output-dir]
// Requires Node's built-in WebSocket (Node 22+) and a desktop session.
import { spawn, spawnSync } from 'node:child_process';
import http from 'node:http';
import { promises as fs } from 'node:fs';
import path from 'node:path';
import net from 'node:net';

const binary = process.argv[2];
if (!binary) throw new Error('Pass the Obsidian executable path.');
const root = path.resolve(process.argv[3] ?? `target/obsidian-reference/capture-${Date.now()}`);
const vault = path.join(root, 'vault');
const profile = path.join(root, 'profile');
await fs.mkdir(path.join(vault, '.obsidian'), { recursive: true });
await fs.mkdir(profile, { recursive: true });
const coverage = JSON.parse(await fs.readFile('tests/fixtures/obsidian-coverage.json', 'utf8'));
const config = { vaults: { '0123456789abcdef': { path: vault, ts: Date.now(), open: true } } };
await fs.writeFile(path.join(profile, 'obsidian.json'), JSON.stringify(config));
await fs.mkdir(path.join(root, 'config', 'obsidian'), { recursive: true });
await fs.writeFile(path.join(root, 'config', 'obsidian', 'obsidian.json'), JSON.stringify(config));
await fs.writeFile(path.join(vault, '.obsidian', 'app.json'), JSON.stringify({ readableLineLength: true, strictLineBreaks: false, showLineNumber: true }));
await fs.writeFile(path.join(vault, '.obsidian', 'core-plugins.json'), '[]');
await fs.writeFile(path.join(vault, 'Coverage.md'), '# Rendering reference\n\nAn isolated test vault.\n');
await fs.writeFile(path.join(vault, 'Target.md'), '# Target\n\n## Section\n\nAn embedded paragraph. ^sample\n');
await fs.writeFile(path.join(vault, 'Pixel.svg'), '<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><rect width="200" height="100" fill="#73b4a2"/><circle cx="50" cy="50" r="30" fill="#b39df3"/></svg>');
await fs.writeFile(path.join(vault, 'Sample.canvas'), JSON.stringify({ nodes: [{ id: 'a', type: 'text', text: 'A card', x: 0, y: 0, width: 200, height: 100 }], edges: [] }));
const wav = Buffer.alloc(44 + 1600);
wav.write('RIFF'); wav.writeUInt32LE(wav.length - 8, 4); wav.write('WAVEfmt ', 8); wav.writeUInt32LE(16, 16); wav.writeUInt16LE(1, 20); wav.writeUInt16LE(1, 22); wav.writeUInt32LE(8000, 24); wav.writeUInt32LE(16000, 28); wav.writeUInt16LE(2, 32); wav.writeUInt16LE(16, 34); wav.write('data', 36); wav.writeUInt32LE(1600, 40);
await fs.writeFile(path.join(vault, 'Sample.wav'), wav);
// Original PDF/video fixtures keep the reference audit independent of user data.
const stream = 'BT /F1 24 Tf 40 120 Td (Opalglass PDF fixture) Tj ET';
const objects = [
  '<< /Type /Catalog /Pages 2 0 R >>',
  '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
  '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 360 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>',
  `<< /Length ${stream.length} >>\nstream\n${stream}\nendstream`,
  '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
];
let pdf = '%PDF-1.4\n';
const offsets = [0];
for (const [index, object] of objects.entries()) { offsets.push(Buffer.byteLength(pdf)); pdf += `${index + 1} 0 obj\n${object}\nendobj\n`; }
const xref = Buffer.byteLength(pdf);
pdf += `xref\n0 ${offsets.length}\n0000000000 65535 f \n`;
for (const offset of offsets.slice(1)) pdf += `${String(offset).padStart(10, '0')} 00000 n \n`;
pdf += `trailer\n<< /Size ${offsets.length} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
await fs.writeFile(path.join(vault, 'Sample.pdf'), pdf);
const video = spawnSync('ffmpeg', ['-v', 'error', '-f', 'lavfi', '-i', 'color=c=teal:s=160x90:d=1', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-y', path.join(vault, 'Sample.mp4')]);
const fixtureLimitations = video.status === 0 ? [] : ['Video fixture could not be generated; ffmpeg with libx264 is required'];
const remoteImage = await fs.readFile(path.join(vault, 'Pixel.svg'));
const imageServer = http.createServer((_, response) => { response.writeHead(200, {'Content-Type': 'image/svg+xml'}); response.end(remoteImage); });
await new Promise(resolve => imageServer.listen(0, '127.0.0.1', resolve));
const remoteUri = `http://127.0.0.1:${imageServer.address().port}/image.svg`;

const port = await new Promise(resolve => {
  const s = net.createServer(); s.listen(0, '127.0.0.1', () => { const port = s.address().port; s.close(() => resolve(port)); });
});
const env = { ...process.env, XDG_CONFIG_HOME: path.join(root, 'config'), XDG_CACHE_HOME: path.join(root, 'cache') };
delete env.WAYLAND_DISPLAY;
const log = await fs.open(path.join(root, 'app.log'), 'w');
const child = spawn(path.resolve(binary), [`--user-data-dir=${profile}`, '--no-sandbox', '--ozone-platform=x11', `--remote-debugging-port=${port}`, '--remote-debugging-address=127.0.0.1'], { env, stdio: ['ignore', log.fd, log.fd] });
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
let socket;
let id = 0;
const pending = new Map();
async function rpc(method, params = {}) {
  const key = ++id;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => { pending.delete(key); reject(new Error(`Timed out: ${method}`)); }, 15000);
    pending.set(key, { resolve: v => { clearTimeout(timer); resolve(v); }, reject });
    socket.send(JSON.stringify({ id: key, method, params }));
  });
}
async function evaluate(expression) {
  const result = await rpc('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
  return result.result.value;
}
try {
  let page;
  for (let i = 0; i < 150; i++) {
    if (child.exitCode !== null) throw new Error(`Reference app exited: ${child.exitCode}`);
    try {
      const pages = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      page = pages.find(p => p.type === 'page');
      if (page) break;
    } catch {}
    await wait(200);
  }
  if (!page) throw new Error('Reference app did not expose a test page.');
  socket = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  socket.onmessage = event => {
    const data = JSON.parse(event.data);
    const call = pending.get(data.id);
    if (call) { pending.delete(data.id); if (data.error) call.reject(new Error(JSON.stringify(data.error))); else call.resolve(data.result); }
  };
  await rpc('Runtime.enable');
  let ready = false;
  for (let i = 0; i < 100; i++) {
    ready = await evaluate('typeof app !== "undefined" && !!app.workspace && !!app.workspace.layoutReady');
    if (ready) break;
    await wait(200);
  }
  if (!ready) throw new Error('Reference vault did not finish loading.');
  const version = { release: coverage.reference_version, executable: path.resolve(binary), ...(await evaluate('({electron: process.versions.electron, vault: app.vault.getName()})')) };
  await rpc('Emulation.setDeviceMetricsOverride', { width: 1000, height: 800, deviceScaleFactor: 1, mobile: false });
  const records = [];
  for (const fixture of coverage.cases.filter(c => !process.env.OPALGLASS_REFERENCE_CASES || process.env.OPALGLASS_REFERENCE_CASES.split(',').includes(c.id))) {
    const result = await evaluate(`(async () => {
      const file = app.vault.getAbstractFileByPath('Coverage.md');
      await app.vault.modify(file, ${JSON.stringify(fixture.markdown.replaceAll('REMOTE_IMAGE_URI', remoteUri))});
      const leaf = app.workspace.getLeaf(false);
      await leaf.setViewState({type:'markdown',state:{file:'Coverage.md',mode:'preview'},active:true});
      leaf.view.previewMode.rerender(true);
      await new Promise(resolve => setTimeout(resolve, 800));
      const content = leaf.view.previewMode.containerEl;
      if (content.querySelector('.pdf-embed')) {
        for (let i = 0; i < 50 && !content.querySelector('.pdfViewer .page'); i++) await new Promise(resolve => setTimeout(resolve, 100));
      }
      // Only approve our own authored diagram in this isolated fixture vault.
      const allow = [...content.querySelectorAll('button')].find(button => button.textContent.trim() === 'Allow');
      if (allow && content.innerText.includes('Display Mermaid diagrams in this vault?')) {
        allow.click();
        await new Promise(resolve => setTimeout(resolve, 1200));
      }
      return {ready:{pdf:!content.querySelector('.pdf-embed') || !!content.querySelector('.pdfViewer .page'), images:[...content.querySelectorAll('img')].every(i => i.complete && i.naturalWidth > 0)}, html:content.innerHTML, text:content.innerText, elements:[...content.querySelectorAll('*')].map(e=>({tag:e.tagName,classes:e.className?.baseVal??e.className,text:e.childElementCount?undefined:e.textContent,href:e.getAttribute('href'),src:e.getAttribute('src')}))};
    })()`);
    if (!result.ready.pdf) fixtureLimitations.push(`${fixture.id}: PDF page did not finish rendering within the capture deadline`);
    if (!result.ready.images) fixtureLimitations.push(`${fixture.id}: one or more images did not load`);
    const dir = path.join(root, fixture.id);
    await fs.mkdir(dir, { recursive: true });
    await fs.writeFile(path.join(dir, 'render.json'), JSON.stringify(result, null, 2));
    const png = await rpc('Page.captureScreenshot', { format: 'png' });
    await fs.writeFile(path.join(dir, 'reading.png'), Buffer.from(png.data, 'base64'));
    records.push({ id: fixture.id, source: fixture.markdown, result: path.relative(root, path.join(dir, 'render.json')), screenshot: path.relative(root, path.join(dir, 'reading.png')) });
    console.log(`Captured ${fixture.id}`);
  }
  await fs.writeFile(path.join(root, 'manifest.json'), JSON.stringify({ version, mode: 'Actual MarkdownView Reading output; live preview not yet audited', fixtureLimitations, records }, null, 2));
  console.log(`Reference artifacts: ${root}`);
} finally {
  imageServer.close();
  socket?.close();
  child.kill('SIGTERM');
  await Promise.race([new Promise(resolve => child.once('exit', resolve)), wait(3000)]);
  if (child.exitCode === null) child.kill('SIGKILL');
  await log.close();
}
