"""原本で確認した一文字改訂の局所性と、独立した文字セル位置だけを検証する。"""
import math


def compact(text):
    """空白だけを除き、順序・結合記号・互換文字・U+FEFFを保持する。"""
    return ''.join(text.split())


def specification(gold):
    """一文字だけが違う有限の文脈と、Unicode scalar単位の位置を要求する。"""
    contexts=[]
    for side in ['old','new']:
        page=gold.get(side+'_page');text=gold.get(side+'_context');focus=gold.get(side+'_focus')
        if type(page) is not int or not 1<=page<=5000 or not isinstance(text,str):
            raise ValueError('Revision gold requires a bounded page and literal context')
        text=compact(text)
        if not 1<=len(text)<=4096 or type(focus) is not int or not 0<=focus<len(text):
            raise ValueError('Revision focus must address a bounded scalar context')
        contexts.append((text,focus))
    (old,a),(new,b)=contexts
    if old[a]==new[b] or old[:a]!=new[:b] or old[a+1:]!=new[b+1:]:
        raise ValueError('This gold requires exactly one independently declared scalar edit')
    return contexts


def hunks(block):
    """製品の区間を再構成し、空白以外の連続した削除・追加を独立に集計する。"""
    original={'old':[],'new':[]};positions={'old':0,'new':0};result=[];active=None
    for segment in block.get('segments',[]):
        kind=segment.get('kind');text=segment.get('text')
        if kind not in ['equal','delete','insert'] or not isinstance(text,str):
            raise ValueError('Invalid revision segment')
        for side in (['old','new'] if kind=='equal' else ['old'] if kind=='delete' else ['new']):
            original[side].append(text)
        literal=compact(text)
        if not literal:continue
        if kind=='equal':
            if active is not None:result.append(active);active=None
            positions={side:at+len(literal) for side,at in positions.items()}
        else:
            if active is None:active=dict(old='',new='',old_start=positions['old'],new_start=positions['new'])
            side='old' if kind=='delete' else 'new'
            active[side]+=literal;positions[side]+=len(literal)
    if active is not None:result.append(active)
    if any(''.join(original[side])!=block.get(side+'_text') for side in ['old','new']):
        raise ValueError('Segments do not reconstruct the reported block text')
    return result


def page_streams(blocks):
    """元のブロック番号順で文字列を復元し、文脈が断片を跨ぐ場合も検査する。"""
    streams={};offsets={}
    for side in ['old','new']:
        values={};at=0;parts=[];offsets[side]={}
        for block in blocks:
            index=block.get(side+'_index')
            if index is None:continue
            text=block.get(side+'_text')
            if type(index) is not int or index<1 or index in values or not isinstance(text,str):
                raise ValueError('Block ownership is missing or duplicated')
            values[index]=compact(text)
        for index,text in sorted(values.items()):
            offsets[side][index]=at;parts.append(text);at+=len(text)
        streams[side]=''.join(parts)
    return streams,offsets


def check_revision(diff,gold):
    """対象文脈内の一文字編集だけを採点し、ブロックの結合方法を正解にしない。"""
    contexts=specification(gold)
    result=dict(passed=False,scope='Selected literal context and one scalar edit only',page_pair_matches=False)
    pairs=[p for p in diff.get('pages',[]) if p.get('old_index')==gold['old_page'] and p.get('new_index')==gold['new_page']]
    if len(pairs)!=1:return result|dict(reason='Expected a unique page pair')
    result['page_pair_matches']=True;blocks=pairs[0].get('blocks',[])
    try:streams,offsets=page_streams(blocks)
    except ValueError as error:return result|dict(reason=str(error))
    if any(streams[side].count(context)!=1 for side,(context,_) in zip(['old','new'],contexts)):
        return result|dict(reason='Expected unique literal contexts in the source block streams')
    starts=[streams[side].index(context) for side,(context,_) in zip(['old','new'],contexts)]
    changes=[]
    try:
        for side,start,(_,focus) in zip(['old','new'],starts,contexts):
            owner=next(b for b in blocks if b.get(side+'_index') is not None
                       and offsets[side][b[side+'_index']]<=start+focus<offsets[side][b[side+'_index']]+len(compact(b[side+'_text'])))
            result[side+'_block']=owner[side+'_index'];result[side+'_bounds']=owner.get(side+'_bounds')
        for block in blocks:
            kind=block.get('kind')
            if kind=='changed':local=hunks(block)
            elif kind in ['added','removed']:
                side='new' if kind=='added' else 'old'
                local=[dict(old='',new='',old_start=0,new_start=0)|{side:compact(block[side+'_text'])}]
            elif kind in ['unchanged','moved','resized','formatted']:
                if compact(block['old_text'])!=compact(block['new_text']):raise ValueError('Unreported text edit')
                local=[]
            else:raise ValueError('Unknown block change kind')
            for change in local:
                for side in ['old','new']:
                    index=block.get(side+'_index')
                    change[side+'_start']=None if index is None else change[side+'_start']+offsets[side][index]
                changes.append(change)
    except (ValueError,KeyError,StopIteration) as error:return result|dict(reason=str(error))
    relevant=[]
    for change in changes:
        for side,start,(context,_) in zip(['old','new'],starts,contexts):
            low=change[side+'_start']
            if low is None:continue
            high=low+len(change[side])
            if (low<start+len(context) and high>start) or (low==high and start<=low<start+len(context)):
                relevant.append(change);break
    exact=len(relevant)==1 and all(
        relevant[0][side]==context[focus] and relevant[0][side+'_start']==start+focus
        for side,start,(context,focus) in zip(['old','new'],starts,contexts))
    return result|dict(passed=exact,reason=None if exact else 'The local edit includes missing or unchanged context',
                       scoped_hunks=relevant)


def rectangle(value):
    """有限かつ正の大きさを持つ元の座標だけを採用する。"""
    if not isinstance(value,dict):raise ValueError('Missing character-cell rectangle')
    result=[value.get(k) for k in ['x0','y0','x1','y1']]
    if any(type(v) not in [int,float] or not math.isfinite(v) for v in result):raise ValueError('Nonfinite character cell')
    if result[0]>=result[2] or result[1]>=result[3]:raise ValueError('Empty character cell')
    return result


def check_span(layer,gold,side,expected_block=None):
    """宣言文字の横幅を独立セルへ照合する。字面の画素や全文の強調精度は保証しない。"""
    if side not in ['old','new']:raise ValueError('Unknown revision side')
    context,focus=specification(gold)[0 if side=='old' else 1]
    threshold=gold.get('minimum_horizontal_iou')
    if type(threshold) not in [int,float] or not 0<threshold<=1:raise ValueError('Invalid character-cell overlap threshold')
    parts=[];positions=[]
    for index,run in enumerate(layer,1):
        text=run.get('text')
        if not isinstance(text,str):return dict(passed=False,reason='Invalid character layer text')
        for at,c in enumerate(text):
            if not c.isspace():parts.append(c);positions.append((index,at))
    text=''.join(parts)
    if text.count(context)!=1:return dict(passed=False,reason='Ambiguous character-layer context')
    index,at=positions[text.index(context)+focus];run=layer[index-1];characters=run.get('chars',[])
    if expected_block is not None and index!=expected_block:
        return dict(passed=False,reason='Character layer and diff block identity disagree')
    if run.get('source_text_region') or run.get('source_text_anchor') or len(characters)!=len(run['text']):
        return dict(passed=False,reason='No independent per-scalar glyph position')
    if any(region.get('start',0)<=at<region.get('end',0) for region in run.get('source_regions',[])):
        return dict(passed=False,reason='Logical source region does not own a per-scalar glyph cell')
    try:
        box=rectangle(run.get('bounds'));source=rectangle(gold.get('source_cells',{}).get(side))
        span=characters[at];left,right=span.get('x0'),span.get('x1')
        if any(type(v) not in [int,float] or not math.isfinite(v) for v in [left,right]) or left>=right:
            raise ValueError('Invalid public character span')
        overlap=max(0.,min(right,source[2])-max(left,source[0]))
        union=max(right,source[2])-min(left,source[0]);iou=overlap/union
        passed=iou>=threshold and box[1]<=source[1]/2+source[3]/2<=box[3]
        return dict(passed=passed,scope='Horizontal character cell and line-height centre only',
                    block=index,horizontal_iou=iou,span=dict(x0=left,x1=right),line_bounds=run['bounds'],reference_cell=gold['source_cells'][side])
    except (ValueError,TypeError) as error:return dict(passed=False,reason=str(error))
