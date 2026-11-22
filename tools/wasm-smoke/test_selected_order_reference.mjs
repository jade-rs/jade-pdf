import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {validateSelectedOrderReference} from './selected_order_reference.mjs';

const root=await fs.mkdtemp(path.join(os.tmpdir(),'jade-selected-order-ref-'));
try {
 const rules=[{case_id:'image',page:1,anchors:['F01','F08'],scope:'Image labels'},
   {case_id:'table',page:2,anchors:['Type','Name'],scope:'Table labels'}];
 const gold={source_id:'owned',source_sha256:'a'.repeat(64),source_url:'https://example.test/a.pdf',
   product_output_inspected_before_initial_declaration:false,reading_order:rules};
 const review={id:'owned',source_sha256:'a'.repeat(64),original_pages_reviewed:[1,2],
   product_output_inspected_before_declaration:false};
 const entry={id:'owned',sha256:'a'.repeat(64),url:gold.source_url,pages:2,
   gold:{...rules[0],kind:'reading_order'},reading_order:[rules[1]],selected_order_reference:{}};
 const pin=async()=>{
  for(const [kind,value] of [['gold',gold],['review',review]]) {
   const data=Buffer.from(JSON.stringify(value));await fs.writeFile(path.join(root,kind+'.json'),data);
   entry.selected_order_reference[kind+'_file']=kind+'.json';
   entry.selected_order_reference[kind+'_sha256']=createHash('sha256').update(data).digest('hex');
  }
 };
 await pin();await validateSelectedOrderReference(entry,root);
 await validateSelectedOrderReference({id:'legacy'},root);
 for(const change of [{reading_order:[]},{gold:{...entry.gold,anchors:['F08','F01']}},
   {gold:{...entry.gold,kind:'scalar_inventory'}},{sha256:'b'.repeat(64)},
   {pages:1},{url:'https://example.test/b.pdf'}])
  await assert.rejects(()=>validateSelectedOrderReference({...entry,...change},root));
 for(const change of [{gold_file:'../gold.json'},{review_file:'/review.json'},
   {gold_sha256:'b'.repeat(64)},{review_sha256:'b'.repeat(64)}])
  await assert.rejects(()=>validateSelectedOrderReference({...entry,
   selected_order_reference:{...entry.selected_order_reference,...change}},root));
 for(const invalid of [[],[...rules,...rules],[{...rules[0],page:true}]]) {
  gold.reading_order=invalid;await pin();
  await assert.rejects(()=>validateSelectedOrderReference(entry,root));
 }
 gold.reading_order=rules;review.original_pages_reviewed=[1];await pin();
 await assert.rejects(()=>validateSelectedOrderReference(entry,root));
 console.log('Selected-order source references: omitted failures, changed anchors, source/hash and incomplete reviews rejected');
} finally {await fs.rm(root,{recursive:true,force:true});}
