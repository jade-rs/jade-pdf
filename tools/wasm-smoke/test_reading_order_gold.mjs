import assert from 'node:assert/strict';
import {checkGold,checkPageGold,goldOutcomeAccepted,goldByCohort} from './semantic_gold.mjs';

const gold={kind:'reading_order',anchors:['F01','F08','F01'],
  scope:'Three printed field labels only; not page prose or geometry'};
assert.deepEqual(checkGold('prefix F01\nF08 suffix F01',gold),{
  kind:'reading_order',scope:gold.scope,matches:true,expected_anchors:gold.anchors,matched_prefix:3});
for(const [actual,prefix] of [['F08 F01',1],['F01 F08',2],['image only',0]]) {
  const result=checkGold(actual,gold);
  assert.equal(result.matches,false);
  assert.equal(result.matched_prefix,prefix);
}
const unicode={kind:'reading_order',anchors:['é A','𐐀 B'],scope:'Two labels'};
assert.equal(checkGold('e\u0301\u0085A then 𐐀\nB',unicode).matches,true);
for(const actual of ['e A 𐐀 B','é\ufeffA 𐐀 B','é A 𐐀\u200dB','𐐀 B é A'])
  assert.equal(checkGold(actual,unicode).matches,false);
for(const change of [
  ...[null,'A',[],[null],[''],['\u0085'],['x'.repeat(2049)],Array(257).fill('A'),Array(17).fill('x'.repeat(2048))].map(anchors=>({anchors})),
  ...[null,'','\u001c','x'.repeat(2049)].map(scope=>({scope}))])
  assert.throws(()=>checkGold('',{...gold,...change}));
const result=checkPageGold({pages:[{blocks:[{text:'image only'}]}]},{...gold,page:1});
assert.equal(result.matches,false);
assert.equal(goldOutcomeAccepted('documents',result.matches,true),false);
assert.equal(goldOutcomeAccepted('pending_semantic',result.matches,true),true);
assert.equal(goldOutcomeAccepted('pending_semantic',result.matches,false),false);
assert.deepEqual(goldByCohort([{cohort:'pending_semantic',kind:result.kind,matches:result.matches}]),
  {pending_semantic:{reading_order:{cases:1,correct:0,failed:1}}});
console.log('Reading-order gold: bounded selected anchors; missing/reordered occurrences stay failures');
