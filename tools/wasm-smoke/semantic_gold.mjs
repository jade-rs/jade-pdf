// Report the check's actual scope; scalar inventories do not validate page prose or order.
function inventorySpec(gold) {
  const {ranges,count_each:count,scope}=gold;
  if(!Array.isArray(ranges)||!ranges.length||ranges.length>4096||!Number.isSafeInteger(count)
    ||count<1||typeof scope!=='string'||!scope.trim())
    throw new Error('Scalar inventory requires bounded ranges, a positive count and scope');
  const codes=new Set();
  for(const pair of ranges) {
    if(!Array.isArray(pair)||pair.length!==2||pair.some(c=>!Number.isSafeInteger(c)))
      throw new Error('Invalid scalar range');
    const [start,end]=pair;
    if(start<0||end>0x10ffff||start>end||(start<=0xdfff&&end>=0xd800)
      ||codes.size+end-start+1>4096)
      throw new Error('Scalar range is invalid or exceeds the 4096-scalar budget');
    for(let c=start;c<=end;c++) {
      if(codes.has(c)) throw new Error('Overlapping scalar ranges');
      codes.add(c);
    }
  }
  if(codes.size*count>2_000_000) throw new Error('Expected inventory exceeds the scalar accounting budget');
  return {codes:[...codes].sort((a,b)=>a-b),count};
}

// Match Python str.split whitespace while retaining U+FEFF and all joiners.
const normalizeText=text=>text.normalize('NFC').replace(/[\u0009-\u000d\u001c-\u0020\u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]+/gu,' ').replace(/^ | $/gu,'');

export function pageHasTextReview(page) {
  return !page.blocks.some(b=>normalizeText(b.text).length>0)
    ||['sdk_unicode_mapping_errors','sdk_text_objects_without_characters',
      'source_text_characters_missing','source_unicode_scalars_unmatched'].some(key=>(page[key]??0)>0)
    ||['source_text_accounting_unavailable','source_unicode_accounting_unavailable',
      'reading_order_requires_review'].some(key=>page[key]===true);
}

export function checkGold(actual,gold) {
  const kind=gold.kind??'full_page_text';
  if(kind==='full_page_text') {
    if(typeof gold.text!=='string') throw new Error('Full-page text gold requires expected text');
    return {kind,matches:normalizeText(actual)===normalizeText(gold.text),expected_text:gold.text,actual_text:actual};
  }
  if(kind!=='scalar_inventory') throw new Error('Unknown semantic gold kind');
  const {codes,count}=inventorySpec(gold), selected=new Set(codes), observed=new Map();
  for(const scalar of actual) {
    const code=scalar.codePointAt(0);
    if(selected.has(code)) observed.set(code,(observed.get(code)??0)+1);
  }
  const key=c=>'U+'+c.toString(16).toUpperCase().padStart(5,'0');
  return {kind,scope:gold.scope,matches:codes.every(c=>(observed.get(c)??0)===count),
    expected_inventory:Object.fromEntries(codes.map(c=>[key(c),count])),
    actual_inventory:Object.fromEntries(codes.map(c=>[key(c),observed.get(c)??0]))};
}
