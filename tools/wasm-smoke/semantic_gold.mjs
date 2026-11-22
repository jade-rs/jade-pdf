// Report the check's actual scope; scalar inventories do not validate page prose or order.
export function semanticEntries(manifest) {
  return ['documents','pending_semantic'].flatMap(cohort=>(manifest[cohort]??[]).map(entry=>{
    const contexts=Object.hasOwn(entry,'context_gold')?entry.context_gold:[];
    if(!Array.isArray(contexts)||contexts.length>32||contexts.some(g=>!g||g.kind!=='text_context'))
      throw new Error('Invalid bounded text contexts');
    if(Object.hasOwn(entry,'gold')&&Object.hasOwn(entry,'full_page_text'))
      throw new Error('Ambiguous page gold declarations');
    let gold=entry.gold;
    if(Object.hasOwn(entry,'full_page_text')) {
      if(entry.pages!==1) throw new Error('Legacy full_page_text requires one page');
      gold={page:1,kind:'full_page_text',text:entry.full_page_text};
    }
    return {...entry,semantic_gold_cohort:cohort,...(gold?{gold}:{})};
  }));
}

export function goldOutcomeAccepted(cohort,matches,guarded) {
  if(!['documents','pending_semantic'].includes(cohort)) throw new Error('Unknown semantic gold cohort');
  return matches===true || cohort==='pending_semantic'&&guarded===true;
}

export function checkPageGold(ir,gold) {
  if(!Number.isSafeInteger(gold.page)||gold.page<1||gold.page>ir.pages.length)
    throw new Error('Gold page must be a one-based integer within the extracted document');
  return checkGold(ir.pages[gold.page-1].blocks.map(b=>b.text).join('\n'),gold);
}

export function goldByCohort(rows) {
  const result={};
  for(const {cohort,kind,matches} of rows) {
    if(!['documents','pending_semantic'].includes(cohort)) throw new Error('Unknown semantic gold cohort');
    const kinds=result[cohort]??={};
    const count=kinds[kind]??={cases:0,correct:0,failed:0};
    count.cases++;count[matches?'correct':'failed']++;
  }
  return result;
}

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
  if(kind==='text_context') {
    const values=['text','anchor','scope'].map(key=>gold[key]);
    if(values.some(value=>typeof value!=='string'||!normalizeText(value)||[...value].length>2048))
      throw new Error('Text context requires bounded nonempty text, anchor and scope');
    const [expected,anchor]=values.slice(0,2).map(normalizeText);
    const count=(text,part)=>text.split(part).length-1;
    if(count(expected,anchor)!==1) throw new Error('Text context anchor must occur once within expected context');
    const actualText=normalizeText(actual),contexts=count(actualText,expected),anchors=count(actualText,anchor);
    return {kind,scope:gold.scope,matches:contexts===1&&anchors===1,
      expected_context:gold.text,context_occurrences:contexts,anchor_occurrences:anchors};
  }
  if(kind==='full_page_text') {
    if(typeof gold.text!=='string') throw new Error('Full-page text gold requires expected text');
    return {kind,matches:normalizeText(actual)===normalizeText(gold.text),expected_text:gold.text,actual_text:actual};
  }
  if(kind!=='scalar_inventory') throw new Error('Unknown semantic gold kind');
  const {codes,count}=inventorySpec(gold), selected=new Set(codes), observed=new Map();
  const forbidden=Object.hasOwn(gold,'forbidden_scalars')?gold.forbidden_scalars:[];
  if(!Array.isArray(forbidden)||forbidden.length>4096||forbidden.some(c=>!Number.isSafeInteger(c)||c<0||c>0x10ffff||c>=0xd800&&c<=0xdfff)||new Set(forbidden).size!==forbidden.length)
    throw new Error('Forbidden scalars require a bounded unique scalar list');
  for(const scalar of actual) {
    const code=scalar.codePointAt(0);
    if(selected.has(code)) observed.set(code,(observed.get(code)??0)+1);
  }
  const key=c=>'U+'+c.toString(16).toUpperCase().padStart(5,'0');
  const forbiddenInventory=Object.fromEntries(forbidden.map(c=>[key(c),[...actual].filter(s=>s.codePointAt(0)===c).length]));
  return {kind,scope:gold.scope,matches:codes.every(c=>(observed.get(c)??0)===count)&&Object.values(forbiddenInventory).every(n=>n===0),
    expected_inventory:Object.fromEntries(codes.map(c=>[key(c),count])),
    actual_inventory:Object.fromEntries(codes.map(c=>[key(c),observed.get(c)??0])),forbidden_inventory:forbiddenInventory};
}
export function compactFragmentMatches(actual,gold) {
  if(!Object.hasOwn(gold,'compact_fragment')) return true;
  if(typeof gold.compact_fragment!=='string')
    throw new Error('Compact fragment requires nonempty text');
  const compact=text=>text.replace(/[\u0009-\u000d\u001c-\u0020\u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]+/gu,'');
  const expected=compact(gold.compact_fragment);
  if(!expected.length) throw new Error('Compact fragment requires nonempty text');
  return compact(actual).includes(expected);
}
