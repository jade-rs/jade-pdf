"""独立した原文範囲の内訳を許しつつ、範囲外の全誤編集を残すページgold。"""
from bisect import bisect_right
from page_revision_gold import flattened,ir_text,page_observations,rectangle
from semantic_gold import normalize_text


def source_layout(texts):
    """正規化済みのページ全文と元ブロック開始位置・文字の所有を求める。"""
    keys=sorted(texts)
    if keys!=list(range(1,len(keys)+1)):raise ValueError('Incomplete source block inventory')
    text,owners=flattened(texts,keys);starts={};offset=0;nonempty=False
    for key in keys:
        value=ir_text(texts[key])
        if value and nonempty:offset+=1
        starts[key]=offset
        offset+=len(value);nonempty|=bool(value)
    if offset!=len(text):raise ValueError('Source layout reconstruction failed')
    return dict(text=text,owners=owners,starts=starts,keys=keys,values={k:ir_text(v) for k,v in texts.items()})


def events(page,texts,layouts):
    """全文検証済みの差分から、実際に削除・追加とされた原文の文字範囲を得る。"""
    result=[];view=page.get('text_diff');limited=False
    def emit(segments,bases):
        """共通文字も消費し、部分列検索で反復語を別の場所へ移さない。"""
        at=bases.copy()
        for segment in segments:
            kind=segment['kind'];text=segment['text']
            for side in ['old','new']:
                if kind=='equal' or kind==dict(old='delete',new='insert')[side]:
                    if kind!='equal' and normalize_text(text):
                        result.append(dict(side=side,start=at[side],end=at[side]+len(text),text=text))
                    at[side]+=len(text)
    if view is not None:
        _,limited=page_observations(page,texts)
        for group in view['groups']:
            bases={}
            for side in ['old','new']:
                nonempty=[i for i in group[side+'_blocks'] if ir_text(texts[side][i])]
                bases[side]=layouts[side]['starts'][nonempty[0]] if nonempty else 0
            emit(group['segments'],bases)
    else:
        # 旧CLIの行本文は既定IRで正規化済み。非正準な任意JSONの位置は推測しない。
        if any(value!=ir_text(value) for inventory in texts.values() for value in inventory.values()):
            raise ValueError('Legacy region evaluation requires normalized IR block text')
        for block in page['blocks']:
            bases={s:layouts[s]['starts'].get(block[s+'_index'],0) for s in ['old','new']}
            kind=block['kind']
            if kind=='changed':emit(block['segments'],bases)
            elif kind in ['added','removed']:
                side='old' if kind=='removed' else 'new'
                emit([dict(kind='delete' if side=='old' else 'insert',text=block[side+'_text'])],bases)
    return result,limited


def in_zone(page,bounds,side,index,zone):
    """独立に読んだ上端・下端を、有限な原文ブロック座標で区別する。"""
    if not zone:return True
    size=page.get(side+'_size',{})
    if size.get('rotation')!=0:raise ValueError('Source zone requires explicitly unrotated page')
    box=rectangle(size.get('view_box'));height=box[3]-box[1]
    if height<=0:raise ValueError('Invalid source viewport')
    cell=rectangle(bounds[side][index]);y=((cell[1]+cell[3])/2-box[1])/height
    return y>=.75 if zone=='upper_quarter' else y<=.25


def find_region(page,edit,side,layout,bounds,remaining):
    """全文の中で一意な正確な文字列と、宣言済みの位置・文脈だけを使う。"""
    expected=ir_text(edit[side])
    if not expected:return None,True
    candidates=[];at=0;work=0
    while True:
        at=layout['text'].find(expected,at)
        if at<0:break
        work+=1
        if work>100_000:raise ValueError('Source region candidate budget exceeded')
        remaining[0]-=len(expected)
        if remaining[0]<0:raise ValueError('Source region search budget exceeded')
        end=at+len(expected);owners=set(layout['owners'][at:end])
        context=edit.get(side+'_context')
        contextual=True
        if context:
            first=min(owners);keys=layout['keys'];position=keys.index(first)
            selected=keys[max(0,position-2):position+3]
            a=layout['starts'][selected[0]];last=selected[-1]
            b=layout['starts'][last]+len(layout['values'][last])
            contextual=ir_text(context) in layout['text'][a:b]
        if contextual and all(in_zone(page,bounds,side,i,edit.get('zone')) for i in owners):candidates.append((at,end))
        at+=1
    return (candidates[0],True) if len(candidates)==1 else (None,False)


def check_regions(page,gold,raw_rows,texts,bounds):
    """内部の共通文字は許すが、範囲外の非空白編集と不足した範囲を残す。"""
    layouts={side:source_layout(texts[side]) for side in ['old','new']}
    actual,limited=events(page,texts,layouts);regions=[];declared={side:[] for side in layouts};remaining=[2_000_000]
    for edit in gold['edits']:
        found={};valid=True
        for side in layouts:
            span,unique=find_region(page,edit,side,layouts[side],bounds,remaining);found[side]=span;valid&=unique
            if span is not None:declared[side].append((*span,edit['id']))
        regions.append(dict(id=edit['id'],found=found,unique=valid))
    for spans in declared.values():
        spans.sort()
        if any(a[1]>b[0] for a,b in zip(spans,spans[1:])):raise ValueError('Overlapping source region claims')
    outside={}
    for side,layout in layouts.items():
        pieces=[];at=0
        for start,end,_ in declared[side]:pieces.append(layout['text'][at:start]);at=end
        pieces.append(layout['text'][at:]);outside[side]=normalize_text(''.join(pieces))
    outside_equal=outside['old']==outside['new'];touched=set();unexpected=[]
    starts={side:[s[0] for s in spans] for side,spans in declared.items()}
    for event in actual:
        side=event['side'];pending=[];start=None
        def finish():
            """宣言範囲の境界を越えた未申告区間を、別の誤編集として保持する。"""
            if pending:unexpected.append(dict(side=side,start=start,text=''.join(pending)))
            pending.clear()
        for offset,char in enumerate(event['text'],event['start']):
            if char.isspace():continue
            slot=bisect_right(starts[side],offset)-1
            owner=declared[side][slot][2] if slot>=0 and offset<declared[side][slot][1] else None
            if owner is not None:finish();start=None;touched.add(owner)
            else:
                if start is None:start=offset
                pending.append(char)
        finish()
    accepted=[r['id'] for r in regions if r['unique'] and r['id'] in touched]
    tp=len(accepted);fp=len(unexpected);fn=len(regions)-tp
    return dict(passed=fp==fn==0 and outside_equal and not limited,gold_kind='source_regions',
        view='page_text' if page.get('text_diff') is not None else 'blocks',alignment_limited=limited,
        raw_block_observations=len(raw_rows),true_positives=tp,false_positives=fp,false_negatives=fn,
        precision=tp/(tp+fp) if tp+fp else None,recall=tp/len(regions),raw_observations=len(actual),
        expected_edit_regions=len(regions),matched_edits=accepted,unexpected_edits=unexpected,
        unmatched_edits=[r['id'] for r in regions if r['id'] not in accepted],outside_regions_ordered_text_equal=outside_equal,
        scope='Independently declared exact source regions on the selected page only. Internal shared letters and multiple grapheme hunks are allowed within each region. Every nonwhitespace insertion/deletion outside those regions remains a false positive; outside ordered text must match. Full source coverage and segment reconstruction are mandatory. Raw line evidence, limits and existing atomic-hunk gold are retained. Not scalar-level or full-document precision/recall.')
