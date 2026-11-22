"""選択ページの論理的な本文編集を全件goldと照合し、未宣言の差分も数える。"""
import math

from semantic_gold import normalize_text


def positive(value):
    """boolをページ番号・ブロック番号として受け入れない。"""
    return type(value) is int and value > 0


def rectangle(value):
    """非有限・反転した座標を位置の証拠にしない。"""
    try:
        values=[value[k] for k in ['x0','y0','x1','y1']]
        if any(type(v) not in (int,float) or not math.isfinite(v) for v in values):raise ValueError()
        if values[0]>values[2] or values[1]>values[3]:raise ValueError()
        return values
    except (TypeError,KeyError):raise ValueError('Invalid text rectangle') from None


def observations(page):
    """全文を再構成できる内訳だけを検査し、追加行と変更区間を区別して保持する。"""
    result=[];texts={side:{} for side in ['old','new']};bounds={side:{} for side in texts};budget=0
    blocks=page.get('blocks')
    if not isinstance(blocks,list) or len(blocks)>100_000:raise ValueError('Invalid block inventory')
    for block in blocks:
        kind=block.get('kind')
        if kind not in ['added','removed','changed','moved','resized','formatted','unchanged']:raise ValueError('Unknown block kind')
        for side in texts:
            text=block.get(side+'_text');index=block.get(side+'_index');box=block.get(side+'_bounds')
            absent=(side=='old' and kind=='added') or (side=='new' and kind=='removed')
            if absent:
                if text is not None or index is not None or box is not None:raise ValueError('Unexpected absent side')
            else:
                if not positive(index) or index in texts[side] or not isinstance(text,str):raise ValueError('Invalid or duplicate text index')
                rectangle(box);texts[side][index]=text;bounds[side][index]=box;budget+=len(text)
        if budget>4_000_000:raise ValueError('Page text budget exceeded')
        base={side+'_index':block.get(side+'_index') for side in texts}
        if kind in ['added','removed']:
            if block.get('segments',[])!=[]:raise ValueError('Unexpected one-sided segments')
            result.append(dict(base,kind=kind,old=normalize_text(block.get('old_text') or ''),new=normalize_text(block.get('new_text') or '')))
            continue
        segments=block.get('segments',[])
        if not isinstance(segments,list):raise ValueError('Invalid segment inventory')
        if kind!='changed':
            if block['old_text']!=block['new_text'] or segments:raise ValueError('Unreported text edit')
            continue
        reconstructed={side:[] for side in texts};pending={side:[] for side in texts}
        def finish():
            """共通部分を挟まない削除・追加だけを一つの置換区間として数える。"""
            old,new=(normalize_text(''.join(pending[side])) for side in ['old','new'])
            if old or new:result.append(dict(base,kind='hunk',old=old,new=new))
            pending['old'].clear();pending['new'].clear()
        for segment in segments:
            if not isinstance(segment,dict) or segment.get('kind') not in ['equal','insert','delete'] or not isinstance(segment.get('text'),str):raise ValueError('Invalid segment')
            typ=segment['kind'];text=segment['text']
            for side in reconstructed:
                if typ=='equal' or typ==dict(old='delete',new='insert')[side]:reconstructed[side].append(text)
            if typ=='equal':finish()
            else:pending['old' if typ=='delete' else 'new'].append(text)
        finish()
        if any(''.join(reconstructed[side])!=block[side+'_text'] for side in texts):raise ValueError('Segments do not reconstruct full text')
    return [r for r in result if r['old'] or r['new']],texts,bounds


def ir_text(text):
    """公開IRの既定の合字・正準表記・空白の正規化を独立に解釈する。"""
    replacements={'\u00ad':'','ﬀ':'ff','ﬁ':'fi','ﬂ':'fl','ﬃ':'ffi','ﬄ':'ffl','ﬅ':'st','ﬆ':'st','ſ':'s'}
    return normalize_text(''.join(replacements.get(c,c) for c in text))


def flattened(texts,keys):
    """全文を保持し、正規化後の各文字が参照する元ブロックも返す。"""
    parts=[];owners=[]
    for key in keys:
        value=ir_text(texts[key])
        if not value:continue
        if parts:parts.append(' ');owners.append(owners[-1])
        parts.append(value);owners.extend([key]*len(value))
    return ''.join(parts),owners


def page_observations(page,texts):
    """行データとの全文照合・未変更範囲の同一性を確認してから、ページ本文の内訳を使う。"""
    view=page['text_diff']
    if not isinstance(view,dict) or type(view.get('alignment_limited',False)) is not bool:raise ValueError('Invalid page text view')
    groups=view.get('groups')
    if not isinstance(groups,list) or len(groups)>100_000:raise ValueError('Invalid page text groups')
    keys={side:sorted(texts[side]) for side in texts};cursor={side:0 for side in texts};rows=[]
    for side in texts:
        if keys[side]!=list(range(1,len(keys[side])+1)):raise ValueError('Incomplete source block inventory')
    for group in groups:
        values={};owners={};ends={};gaps={}
        for side in texts:
            span=group.get(side+'_block_range')
            if not isinstance(span,list) or len(span)!=2 or any(type(v) is not int for v in span):raise ValueError('Invalid page text range')
            start,end=span
            if not cursor[side]<=start<=end<=len(keys[side]):raise ValueError('Overlapping or invalid page text range')
            declared=group.get(side+'_blocks');selected=keys[side][start:end]
            if not isinstance(declared,list) or any(not positive(v) for v in declared) or declared!=selected:raise ValueError('Page text block references differ from source')
            gaps[side]=flattened(texts[side],keys[side][cursor[side]:start])[0]
            values[side],owners[side]=flattened(texts[side],selected);ends[side]=end
            if group.get(side+'_text')!=values[side]:raise ValueError('Page text omits or substitutes source characters')
        if gaps['old']!=gaps['new']:raise ValueError('Uncovered ordered text edit before group')
        if values['old']==values['new']:raise ValueError('Page text group has no edit')
        segments=group.get('segments')
        if not isinstance(segments,list):raise ValueError('Invalid page text segments')
        reconstructed={side:[] for side in texts};at={side:0 for side in texts};starts=at.copy();pending={side:[] for side in texts};active=False
        def finish():
            """本文区間の各側を、対応する元の行位置へ戻して文脈と位置を検証する。"""
            row=dict(kind='hunk')
            for side in texts:
                raw=''.join(pending[side]);row[side]=normalize_text(raw)
                offset=starts[side]+len(raw)-len(raw.lstrip())
                row[side+'_index']=owners[side][min(offset,len(owners[side])-1)] if owners[side] else None
                pending[side].clear()
            if row['old'] or row['new']:rows.append(row)
        for segment in segments:
            if not isinstance(segment,dict) or segment.get('kind') not in ['equal','insert','delete'] or not isinstance(segment.get('text'),str):raise ValueError('Invalid page text segment')
            kind=segment['kind'];value=segment['text']
            if kind=='equal':
                if active:finish()
                active=False
            elif not active:starts=at.copy();active=True
            for side in texts:
                consumes=kind=='equal' or kind==dict(old='delete',new='insert')[side]
                if consumes:
                    reconstructed[side].append(value);at[side]+=len(value)
                    if kind!='equal':pending[side].append(value)
        if active:finish()
        if any(''.join(reconstructed[side])!=values[side] for side in texts):raise ValueError('Page text segments do not reconstruct source')
        cursor=ends
    trailing={side:flattened(texts[side],keys[side][cursor[side]:])[0] for side in texts}
    if trailing['old']!=trailing['new']:raise ValueError('Uncovered ordered text edit after groups')
    return rows,view.get('alignment_limited',False)


def check_page_revision(diff,gold):
    """明示された全編集と一意に対応する差分だけを正解とし、残りを誤検出に残す。"""
    kind=gold.get('kind','atomic_hunks')
    if kind not in ['atomic_hunks','source_regions']:raise ValueError('Unknown page revision gold kind')
    if not positive(gold.get('old_page')) or not positive(gold.get('new_page')):raise ValueError('Invalid selected pages')
    pages=[p for p in diff.get('pages',[]) if positive(p.get('old_index')) and positive(p.get('new_index')) and p['old_index']==gold['old_page'] and p['new_index']==gold['new_page']]
    if len(pages)!=1:raise ValueError('Selected page pair is missing or ambiguous')
    edits=gold.get('edits')
    if not isinstance(edits,list) or not 1<=len(edits)<=256 or not isinstance(gold.get('scope'),str) or not gold['scope'].strip():raise ValueError('Invalid exhaustive gold scope')
    identifiers=set()
    for e in edits:
        if not isinstance(e,dict) or not isinstance(e.get('id'),str) or not e['id'] or e['id'] in identifiers:raise ValueError('Invalid or duplicate edit identity')
        identifiers.add(e['id'])
        if any(not isinstance(e.get(s),str) or len(e[s])>4096 for s in ['old','new']):raise ValueError('Unbounded expected edit')
        if normalize_text(e['old'])==normalize_text(e['new']):raise ValueError('Gold must declare a logical text edit')
        if e.get('zone') not in [None,'upper_quarter','lower_quarter']:raise ValueError('Unknown source zone')
    page=pages[0];rows,texts,bounds=observations(page);raw_count=len(rows);limited=False;view='blocks'
    if kind=='source_regions':
        from page_region_gold import check_regions
        return check_regions(page,gold,rows,texts,bounds)
    if page.get('text_diff') is not None:
        rows,limited=page_observations(page,texts);view='page_text'
    proposals=[];work=0
    def context(side,index):
        """同じ側の近傍ブロックも含め、太字などによる文字列の分割を許す。"""
        keys=sorted(texts[side]);at=keys.index(index)
        return normalize_text(' '.join(texts[side][k] for k in keys[max(0,at-2):at+3]))
    def located(edit,indices):
        """原本で区別した上端・下端と文脈を、対応した出力位置で検証する。"""
        for side in ['old','new']:
            indexes=[rows[i][side+'_index'] for i in indices if rows[i][side+'_index'] is not None]
            if edit.get(side+'_context'):
                if not indexes or normalize_text(edit[side+'_context']) not in context(side,indexes[0]):return False
            if edit.get('zone') and indexes:
                size=page.get(side+'_size',{})
                if size.get('rotation')!=0:raise ValueError('Source zone requires explicitly unrotated page')
                box=rectangle(size.get('view_box'));height=box[3]-box[1]
                if height<=0:raise ValueError('Invalid source viewport')
                for index in indexes:
                    r=rectangle(bounds[side][index]);y=((r[1]+r[3])/2-box[1])/height
                    if not (y>=.75 if edit['zone']=='upper_quarter' else y<=.25):return False
        return True
    for edit in edits:
        expected={side:normalize_text(edit[side]) for side in ['old','new']};matches=[]
        for i,row in enumerate(rows):
            work+=1
            if work>2_000_000:raise ValueError('Gold matching budget exceeded')
            if all(row[s]==expected[s] for s in expected) and located(edit,[i]):matches.append([i])
        # 新しく追加・削除された段落は、連続する行への分割だけを許す。
        for side,typ in [('new','added'),('old','removed')]:
            other='old' if side=='new' else 'new'
            if expected[other] or not expected[side]:continue
            ordered=sorted((i for i,r in enumerate(rows) if r['kind']==typ),key=lambda i:rows[i][side+'_index'])
            for at,start in enumerate(ordered):
                group=[start];text=rows[start][side]
                for nxt in ordered[at+1:]:
                    work+=1
                    if work>2_000_000:raise ValueError('Gold matching budget exceeded')
                    if rows[nxt][side+'_index']!=rows[group[-1]][side+'_index']+1:break
                    text=normalize_text(text+' '+rows[nxt][side]);group.append(nxt)
                    if text==expected[side] and located(edit,group):matches.append(group.copy())
                    if len(text)>=len(expected[side]):break
        proposals.append(dict(id=edit['id'],matches=matches))
    claims={}
    for n,p in enumerate(proposals):
        if len(p['matches'])==1:
            for i in p['matches'][0]:claims.setdefault(i,[]).append(n)
    accepted=[p for p in proposals if len(p['matches'])==1 and all(len(claims[i])==1 for i in p['matches'][0])]
    used={i for p in accepted for i in p['matches'][0]};unexpected=[r for i,r in enumerate(rows) if i not in used]
    tp=len(accepted);fp=len(unexpected);fn=len(edits)-tp
    return dict(passed=fp==fn==0 and not limited,gold_kind='atomic_hunks',view=view,alignment_limited=limited,
                raw_block_observations=raw_count,true_positives=tp,false_positives=fp,false_negatives=fn,
                precision=tp/(tp+fp) if tp+fp else None,recall=tp/len(edits),
                raw_observations=len(rows),expected_edit_regions=len(edits),
                matched_edits=[p['id'] for p in accepted],unexpected_edits=unexpected,
                unmatched_edits=[p['id'] for p in proposals if p not in accepted],
                scope='Exhaustive logical text-edit regions on the selected page pair only. Validated page text groups must reconstruct all source characters and leave only identical ordered text in their gaps. Raw line evidence is retained. Positional/line-wrap moves, formatting, forms and other pages are separate; no cancellation by inventory equality.')
