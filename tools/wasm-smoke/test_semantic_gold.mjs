import assert from 'node:assert/strict';
import './test_reading_order_gold.mjs';
import './test_selected_order_reference.mjs';
import {checkGold,pageHasTextReview,compactFragmentMatches,semanticEntries,checkPageGold,goldOutcomeAccepted,goldByCohort,operatorGoldCaseId} from './semantic_gold.mjs';

const declared=semanticEntries({documents:[{id:'regular',pages:1,full_page_text:'text'},
  {id:'five-pages',pages:5,gold:{page:5,text:'Disclaimer'}}],
  pending_semantic:[{id:'pending',gold:{page:1,text:'not recovered'}}]});
assert.equal(declared[0].semantic_gold_cohort,'documents');
assert.deepEqual(declared[0].gold,{page:1,kind:'full_page_text',text:'text'});
assert.equal(declared[1].gold.page,5);
assert.equal(declared[2].semantic_gold_cohort,'pending_semantic');
assert.throws(()=>semanticEntries({documents:[{pages:2,full_page_text:'fragment'}]}));
assert.throws(()=>semanticEntries({documents:[{pages:1,full_page_text:'old',gold:{page:1,text:'new'}}]}));
assert.equal(goldOutcomeAccepted('documents',false,true),false);
assert.equal(goldOutcomeAccepted('pending_semantic',false,true),true);
assert.equal(goldOutcomeAccepted('pending_semantic',false,false),false);
assert.equal(goldOutcomeAccepted('documents',true,false),true);
assert.throws(()=>goldOutcomeAccepted('unknown',false,true));
const pages={pages:[{blocks:[{text:'other'}]},{blocks:[{text:'Disclaimer'},{text:'exact text'}]}]};
assert.equal(checkPageGold(pages,{page:2,text:'Disclaimer exact text'}).matches,true);
assert.equal(checkPageGold(pages,{page:2,text:'Disclaimer'}).matches,false);
for(const page of [0,-1,3,true,'1',null,1.5]) assert.throws(()=>checkPageGold(pages,{page,text:'other'}));
assert.deepEqual(goldByCohort([
  {cohort:'documents',kind:'full_page_text',matches:true},
  {cohort:'pending_semantic',kind:'full_page_text',matches:false}]),{
  documents:{full_page_text:{cases:1,correct:1,failed:0}},
  pending_semantic:{full_page_text:{cases:1,correct:0,failed:1}}});

assert.equal(compactFragmentMatches('…ℓ',{}),true);
assert.equal(compactFragmentMatches('0 ≤ t\n≤ 79',{compact_fragment:'0≤t≤79'}),true);
assert.equal(compactFragmentMatches('0≤t<79',{compact_fragment:'0≤t≤79'}),false);
assert.equal(compactFragmentMatches('A\ufeffB',{compact_fragment:'AB'}),false);
assert.equal(compactFragmentMatches('A\ufeffB',{compact_fragment:'\ufeff'}),true);
for(const value of ['', ' \n', '\u0085', '\u001c', null, 4])
  assert.throws(()=>compactFragmentMatches('anything',{compact_fragment:value}));

const clean={blocks:[{text:'wrong'}],user_unit:null};
const context={kind:'text_context',text:'replaces FIPS 180-3 and specifies',anchor:'replaces FIPS',scope:'One printed line, not page prose or geometry'};
assert.equal(checkGold('prefix\nreplaces FIPS 180-3 and specifies\nsuffix',context).matches,true);
for(const text of ['replaces FIPS 1 80-3 and specifies','replaces FIPS 180-3 and\u200dspecifies',
  context.text+'\nreplaces FIPS 1 80-3 and specifies',context.text+'\n'+context.text])
  assert.equal(checkGold(text,context).matches,false);
for(const change of [{text:''},{text:' '},{text:'a'.repeat(2049)},{anchor:''},{anchor:'absent'},{scope:''},{scope:'\u0085'},{scope:'\u001c'},{text:null}])
  assert.throws(()=>checkGold('',{...context,...change}));
for(const context_gold of [null,{},Array(33).fill(context),[{kind:'full_page_text'}]])
  assert.throws(()=>semanticEntries({documents:[{id:'bad',context_gold}]}));
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

const legacyOperator={id:"nist-legacy",page:15};
assert.equal(operatorGoldCaseId(legacyOperator),"symbol-operator-gold-nist-legacy-page-15");
const plusOperator={id:"nist-fips204-2024",page:15,case_id:"nist-fips204-p15-plus"};
const minusOperator={...plusOperator,case_id:"nist-fips204-p15-minus"};
assert.notEqual(operatorGoldCaseId(plusOperator),operatorGoldCaseId(minusOperator));
assert.equal(operatorGoldCaseId(plusOperator),"symbol-operator-gold-nist-fips204-p15-plus");
for(const case_id of [null,"",42,"a/b","x".repeat(129)])
  assert.throws(()=>operatorGoldCaseId({...legacyOperator,case_id}));
