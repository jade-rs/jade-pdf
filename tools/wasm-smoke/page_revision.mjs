// 固定goldを読み、全実行環境へ同じ本文・原本範囲の監査を適用する。
import path from 'node:path';
import fs from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';

export async function loadPageRevisionGold(root,ref) {
  const file=path.resolve(root,ref.path);
  if(!file.startsWith(path.resolve(root,'corpus')+path.sep)) throw new Error('Page gold path escapes corpus');
  const bytes=await fs.readFile(file);
  if(createHash('sha256').update(bytes).digest('hex')!==ref.sha256) throw new Error('Page gold SHA mismatch');
  const gold=JSON.parse(bytes);
  for(const key of ['id','old_id','new_id','old_sha256','new_sha256'])
    if(gold[key]!==ref[key]) throw new Error('Page gold registration disagrees with its source identity');
  return gold;
}

export function checkPageRevision(root,gold,diff) {
  const run=spawnSync('python3',[path.join(root,'tools/audit_page_revision.py'),'--check-json'],
    {input:JSON.stringify({gold,diff}),encoding:'utf8',timeout:30000,maxBuffer:4*1024*1024});
  if(![0,1].includes(run.status)||!run.stdout.trim())
    throw new Error('Independent page checker failed: '+(run.stderr||String(run.error)).slice(0,2000));
  const result=JSON.parse(run.stdout);
  if(result.passed!==(run.status===0)) throw new Error('Page checker status disagrees with evidence');
  return result;
}
