import assert from 'node:assert/strict';
import {checkGold,pageHasTextReview,compactFragmentMatches} from './semantic_gold.mjs';

assert.equal(compactFragmentMatches('…ℓ',{}),true);
assert.equal(compactFragmentMatches('0 ≤ t\n≤ 79',{compact_fragment:'0≤t≤79'}),true);
assert.equal(compactFragmentMatches('0≤t<79',{compact_fragment:'0≤t≤79'}),false);
for(const value of ['', ' \n', null, 4])
  assert.throws(()=>compactFragmentMatches('anything',{compact_fragment:value}));

const clean={blocks:[{text:'wrong'}],user_unit:null};
assert.equal(pageHasTextReview(clean),false);
for(const field of ['sdk_unicode_mapping_errors','sdk_text_objects_without_characters',
  'source_text_characters_missing','source_unicode_scalars_unmatched'])
  assert.equal(pageHasTextReview({...clean,[field]:1}),true);
for(const field of ['source_text_accounting_unavailable','source_unicode_accounting_unavailable','reading_order_requires_review'])
  assert.equal(pageHasTextReview({...clean,[field]:true}),true);
assert.equal(pageHasTextReview({blocks:[]}),true);
assert.equal(pageHasTextReview({blocks:[{text:' \n'}]}),true);

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
const selected={kind:'scalar_inventory',ranges:[[0x2264,0x2264]],count_each:1,
  scope:'One visible operator',forbidden_scalars:[0xf0a3]};
assert.equal(checkGold('≤\uf04b',selected).matches,true);
assert.equal(checkGold('≤\uf0a3',selected).matches,false);
for(const value of [null,'x',[true],[0xd800],[0x110000],[0xf0a3,0xf0a3]])
  assert.throws(()=>checkGold('≤',{...selected,forbidden_scalars:value}));
assert.equal(checkGold('A',math).matches,false);
assert.equal(checkGold('𝐀',math).matches,true);
for(const change of [{ranges:[]},{ranges:[[2,1]]},{ranges:[[0xd7ff,0xe000]]},
  {ranges:[[0,0x110000]]},{ranges:[[0,4096]]},{ranges:[[1,3],[3,5]]},
  {ranges:[[true,2]]},{count_each:0},{count_each:true},{count_each:30_000_000},
  {count_each:Number.MAX_SAFE_INTEGER+1},{scope:''},{kind:'guess'}])
  assert.throws(()=>checkGold('',{...gold,...change}));
console.log('Semantic gold: explicit full-text/inventory scope, supplementary counts, literal identity and bounded validation passed');
