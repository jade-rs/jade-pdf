import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {selectBrowser} from './browser_engine.mjs';
const engines=Object.fromEntries(['chromium','firefox','webkit'].map(name=>[name,{name:()=>name}]));
assert.equal(selectBrowser(engines,{}).engine,'chromium');
for(const engine of Object.keys(engines)) {
  const selected=selectBrowser(engines,{JADE_PDF_BROWSER_ENGINE:engine});
  assert.equal(selected.launcher,engines[engine]);
  assert.equal(selected.engine,engine);
  assert.deepEqual(selected.options,{headless:true});
}
assert.deepEqual(selectBrowser(engines,{JADE_PDF_BROWSER_ENGINE:'firefox',JADE_PDF_BROWSER:'/custom/firefox'}).options,
  {headless:true,executablePath:'/custom/firefox'});
for(const engine of ['', 'chrome', 'unknown', 'constructor'])
  assert.throws(()=>selectBrowser(engines,{JADE_PDF_BROWSER_ENGINE:engine}),/Unsupported browser engine/);
// CDPによる終了証明を、Firefox/WebKitで実行した証明として扱わない。
for(const engine of ['firefox','webkit']) {
  const worker=spawnSync(process.execPath,[fileURLToPath(new URL('./worker_smoke.mjs',import.meta.url))],
    {env:{...process.env,JADE_PDF_BROWSER_ENGINE:engine},encoding:'utf8',timeout:10000});
  assert.equal(worker.status,1);
  assert.match(worker.stderr,/Full Worker termination qualification requires Chromium CDP/);
}
console.log('Browser selection: explicit engine identity, default, custom executable and invalid engines passed');
