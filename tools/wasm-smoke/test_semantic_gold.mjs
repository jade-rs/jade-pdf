import assert from 'node:assert/strict';
import {checkGold} from './semantic_gold.mjs';

assert.equal(checkGold(' e\u0301\ntext ',{text:'é text'}).matches,true);
for(const [actual,text] of [['خل','لخ'],['²','2'],['بَ','ب'],['ب\u200cت','بت'],['A\ufeffB','AB']])
  assert.equal(checkGold(actual,{text}).matches,false);
const gold={kind:'scalar_inventory',ranges:[[0x10400,0x10401]],count_each:1,scope:'Two printed grid cells only'};
const result=checkGold('title\n𐐁𐐀\nprose',gold);
assert.equal(result.matches,true);
assert.deepEqual(result.expected_inventory,{'U+10400':1,'U+10401':1});
assert.equal(result.scope,gold.scope);
assert.equal('actual_text' in result,false);
for(const actual of ['𐐀','𐐀𐐁𐐁','\u0400\u0401']) assert.equal(checkGold(actual,gold).matches,false);
const math={kind:'scalar_inventory',ranges:[[0x1d400,0x1d400]],count_each:1,scope:'One math scalar'};
assert.equal(checkGold('A',math).matches,false);
assert.equal(checkGold('𝐀',math).matches,true);
for(const change of [{ranges:[]},{ranges:[[2,1]]},{ranges:[[0xd7ff,0xe000]]},
  {ranges:[[0,0x110000]]},{ranges:[[0,4096]]},{ranges:[[1,3],[3,5]]},
  {ranges:[[true,2]]},{count_each:0},{count_each:true},{count_each:30_000_000},
  {count_each:Number.MAX_SAFE_INTEGER+1},{scope:''},{kind:'guess'}])
  assert.throws(()=>checkGold('',{...gold,...change}));
console.log('Semantic gold: explicit full-text/inventory scope, supplementary counts, literal identity and bounded validation passed');
