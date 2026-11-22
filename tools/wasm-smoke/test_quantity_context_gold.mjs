import assert from 'node:assert/strict';
import {checkGold} from './semantic_gold.mjs';
const gold={kind:'quantity_context',context:'units of material',numerator:1,denominator:8,scope:'One printed quantity only'};
for(const number of ['⅛','1/8','1 / 8','1⁄8','2/16','0.125','+1/8']) {
 const result=checkGold(number+' units\nof material',gold);assert.equal(result.matches,true,number);assert.deepEqual(result.actual_values,['1/8']);assert.equal(result.expected_value,'1/8');
}
for(const number of ['¼','1/4','11/8','-1/8','−1/8','1 ⅛','1⅛','1 1/8','1.125','1/0','0.124999999999','v1/8','1/8\ufeff','11111111111111111111111111111111111111111111111111111111111111111/8']) assert.equal(checkGold(number+' units of material',gold).matches,false,number);
for(const text of ['1/8 units of material 1/8 units of material','1/8 units of materials','1/8 units of mater\u200dial','1/8 UNITS of material']) assert.equal(checkGold(text,gold).matches,false,text);
assert.equal(checkGold('⅛ cafe\u0301 ℓ',{...gold,context:'café ℓ'}).matches,true);assert.equal(checkGold('⅛ cafe l',{...gold,context:'café ℓ'}).matches,false);
for(const number of ['1 ⅛','1⅛','1 1/8','9/8','1.125']) assert.equal(checkGold(number+' units of material',{...gold,numerator:9}).matches,true,number);
assert.equal(checkGold('0 units of material',{...gold,numerator:0}).matches,true);
for(let denominator=1;denominator<=32;denominator++) for(let numerator=0;numerator<=16;numerator++) for(const multiple of [1,2,7]) assert.equal(checkGold(`${numerator*multiple}/${denominator*multiple} units of material`,{...gold,numerator,denominator}).matches,true);
const changes=[...[null,'',' ','x'.repeat(257)].map(context=>({context})),...[null,'','x'.repeat(2049)].map(scope=>({scope})),...[true,-1,1.5,'1',1000001].map(numerator=>({numerator})),...[true,0,-1,1.5,1000001].map(denominator=>({denominator}))];for(const change of changes) assert.throws(()=>checkGold('',{...gold,...change}));
for(const text of ['x'.repeat(2000001),'1/8 units of material '.repeat(33)]) assert.throws(()=>checkGold(text,gold));
console.log('quantity_context: equivalent numeric values, boundaries and bounded work passed');
