// 各実行環境の出力へ、同じ独立gold検査をJSON経由で適用する。
import path from 'node:path';
import {spawnSync} from 'node:child_process';

export function checkScalarRevision(root,gold,output) {
  const run=spawnSync('python3',[path.join(root,'tools/audit_scalar_revision.py'),'--check-json'],
    {input:JSON.stringify({gold,...output}),encoding:'utf8',timeout:30000,maxBuffer:2*1024*1024});
  if(![0,1].includes(run.status)||!run.stdout.trim())
    throw new Error('Independent revision checker failed: '+(run.stderr||String(run.error)).slice(0,2000));
  const result=JSON.parse(run.stdout);
  if(result.passed!==(run.status===0)) throw new Error('Revision checker status disagrees with its evidence');
  return result;
}
