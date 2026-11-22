// Node-side provenance gate: keep every independently declared rule, including failures.
import fs from 'node:fs/promises';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
import {checkGold} from './semantic_gold.mjs';

export async function validateSelectedOrderReference(entry,corpus) {
 if(!Object.hasOwn(entry,'selected_order_reference')) return;
 const refs=entry.selected_order_reference;
 if(!refs||typeof refs!=='object'||Array.isArray(refs)) throw new Error('Invalid selected-order references');
 const values={},root=await fs.realpath(corpus);
 for(const kind of ['gold','review']) {
  const name=refs[kind+'_file'];
  if(typeof name!=='string'||! /^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}\.json$/.test(name))
   throw new Error('Invalid selected-order reference path');
  const file=path.join(corpus,name);
  if(path.dirname(await fs.realpath(file))!==root||(await fs.stat(file)).size>2_000_000)
   throw new Error('Selected-order reference is outside corpus or oversized');
  const data=await fs.readFile(file);
  if(createHash('sha256').update(data).digest('hex')!==refs[kind+'_sha256'])
   throw new Error('Selected-order reference SHA-256 mismatch');
  values[kind]=JSON.parse(data);
 }
 const {gold,review}=values,{pages}=entry;
 if(!gold||typeof gold!=='object'||Array.isArray(gold)||!review||typeof review!=='object'||Array.isArray(review)
   ||!Number.isSafeInteger(pages)||pages<1||pages>10_000
   ||gold.source_id!==entry.id||gold.source_sha256!==entry.sha256||gold.source_url!==entry.url
   ||gold.product_output_inspected_before_initial_declaration!==false
   ||review.id!==entry.id||review.source_sha256!==entry.sha256
   ||review.product_output_inspected_before_declaration!==false
   ||!isDeepStrictEqual(review.original_pages_reviewed,Array.from({length:pages},(_,i)=>i+1)))
  throw new Error('Selected-order source or original-page review mismatch');
 const expected=gold.reading_order,primary=entry.gold,
   additional=Object.hasOwn(entry,'reading_order')?entry.reading_order:[];
 if(!Array.isArray(expected)||expected.length<1||expected.length>32
   ||!primary||primary.kind!=='reading_order'||!Array.isArray(additional)||additional.length>31)
  throw new Error('Selected-order reference requires bounded primary and additional rules');
 const signature=rule=>{
  if(!rule||typeof rule!=='object'||!Number.isSafeInteger(rule.page)||rule.page<1||rule.page>pages
    ||typeof rule.case_id!=='string'||! /^[A-Za-z0-9_-]{1,128}$/.test(rule.case_id))
   throw new Error('Invalid selected-order rule identity or page');
  checkGold('',{...rule,kind:'reading_order'});
  return Object.fromEntries(['case_id','page','anchors','scope'].map(key=>[key,rule[key]]));
 };
 const declared=expected.map(signature),registered=[primary,...additional].map(signature);
 if(new Set(declared.map(rule=>rule.case_id)).size!==declared.length||!isDeepStrictEqual(registered,declared))
  throw new Error('Registered selected-order rules differ from complete original declaration');
}
