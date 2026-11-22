"""独立に宣言した本文範囲内の分割は許し、範囲外の誤編集を隠さない対照。"""
import copy
import unittest

from page_revision_gold import check_page_revision
from test_page_revision_gold import fixture as years_fixture,page_view


def example():
    """内部に共通文字を挟む二つの編集と、無関係な行の折り返しを構成する。"""
    old=['Header','abXcdYef','If you','agree','End'];new=['Header','abPcdQef','If','you agree','End']
    parts=[[],[('equal','ab'),('delete','X'),('insert','P'),('equal','cd'),('delete','Y'),('insert','Q'),('equal','ef')],
           [('equal','If'),('delete',' you')],[('insert','you '),('equal','agree')],[]]
    blocks=[]
    for i,(a,b,segments) in enumerate(zip(old,new,parts),1):
        bounds=dict(x0=1,y0=90-i*10,x1=80,y1=92-i*10)
        blocks.append(dict(kind='unchanged' if a==b else 'changed',old_index=i,new_index=i,old_text=a,new_text=b,
            old_bounds=bounds,new_bounds=bounds,segments=[dict(kind=k,text=t) for k,t in segments]))
    group=dict(old_block_range=[1,2],new_block_range=[1,2],old_blocks=[2],new_blocks=[2],
        old_text=old[1],new_text=new[1],segments=copy.deepcopy(blocks[1]['segments']))
    size=dict(rotation=0,view_box=dict(x0=0,y0=0,x1=100,y1=100))
    page=dict(old_index=1,new_index=1,old_size=size,new_size=size,blocks=blocks,text_diff=dict(groups=[group]))
    gold=dict(kind='source_regions',old_page=1,new_page=1,scope='Complete logical edit regions on this independently specified page pair',
        edits=[dict(id='paragraph',old=old[1],new=new[1])])
    return dict(pages=[page]),gold


class PageRegionGold(unittest.TestCase):
    def test_internal_equal_letters_do_not_split_one_independent_region_into_false_positives(self):
        diff,gold=example();r=check_page_revision(diff,gold)
        self.assertTrue(r['passed']);self.assertEqual((r['true_positives'],r['false_positives'],r['false_negatives']),(1,0,0))
        self.assertEqual(r['gold_kind'],'source_regions')

    def test_wrap_only_deletion_and_insertion_outside_the_region_are_still_two_false_positives(self):
        diff,gold=example();page=diff['pages'][0]
        for i in [2,3]:
            b=page['blocks'][i];page['text_diff']['groups'].append(dict(old_block_range=[i,i+1],new_block_range=[i,i+1],
                old_blocks=[i+1],new_blocks=[i+1],old_text=b['old_text'],new_text=b['new_text'],segments=copy.deepcopy(b['segments'])))
        r=check_page_revision(diff,gold)
        self.assertFalse(r['passed']);self.assertEqual((r['true_positives'],r['false_positives'],r['false_negatives']),(1,2,0))

    def test_whole_page_delete_and_reinsert_cannot_pass_region_precision(self):
        diff,gold=example();p=diff['pages'][0];a=' '.join(b['old_text'] for b in p['blocks']);b=' '.join(b['new_text'] for b in p['blocks'])
        p['text_diff']['groups']=[dict(old_block_range=[0,5],new_block_range=[0,5],old_blocks=list(range(1,6)),new_blocks=list(range(1,6)),
            old_text=a,new_text=b,segments=[dict(kind='delete',text=a),dict(kind='insert',text=b)])]
        r=check_page_revision(diff,gold);self.assertFalse(r['passed']);self.assertGreaterEqual(r['false_positives'],2)

    def test_source_region_ambiguity_and_overlapping_claims_never_pass(self):
        diff,gold=example();gold['edits'].append(dict(id='overlap',old='X',new='P'))
        with self.assertRaises(ValueError):check_page_revision(diff,gold)
        diff,gold=example();gold['edits'][0]['old']='missing text'
        r=check_page_revision(diff,gold);self.assertFalse(r['passed']);self.assertEqual(r['false_negatives'],1)

    def test_repeated_years_require_independent_source_zones_and_pure_insertions_are_retained(self):
        diff,gold,_=years_fixture();page_view(diff);gold['kind']='source_regions'
        for e in gold['edits']:
            if e['id'] in ['top','bottom']:e.update(old='2024',new='2025')
        self.assertTrue(check_page_revision(diff,gold)['passed'])
        del gold['edits'][0]['zone'];self.assertFalse(check_page_revision(diff,gold)['passed'])

    def test_legacy_line_view_keeps_wrap_false_positives_and_limited_view_never_passes(self):
        diff,gold=example();diff['pages'][0].pop('text_diff');r=check_page_revision(diff,gold)
        self.assertFalse(r['passed']);self.assertEqual(r['false_positives'],2)
        diff,gold=example();diff['pages'][0]['text_diff']['alignment_limited']=True
        self.assertFalse(check_page_revision(diff,gold)['passed'])

    def test_unknown_gold_kind_is_not_silently_treated_as_atomic_hunks(self):
        diff,gold,_=years_fixture();gold['kind']='invented-kind'
        with self.assertRaises(ValueError):check_page_revision(diff,gold)

    def test_source_search_budget_is_shared_and_contexts_do_not_bypass_it(self):
        from page_region_gold import find_region,source_layout
        page=example()[0]['pages'][0];layout=source_layout({1:'a'*5000})
        bounds={'old':{1:dict(x0=1,y0=50,x1=80,y1=52)}}
        remaining=[20]
        with self.assertRaisesRegex(ValueError,'budget'):
            find_region(page,dict(old='a'*10,old_context='absent'), 'old',layout,bounds,remaining)
        self.assertLess(remaining[0],0)

    def test_normalized_unicode_positions_still_reject_editing_a_common_character(self):
        diff,gold=example();p=diff['pages'][0];b=p['blocks'][1]
        b.update(old_text='a e\u0301 b',new_text='a 👩‍💻 b',segments=[dict(kind='delete',text='a e\u0301 b'),dict(kind='insert',text='a 👩‍💻 b')])
        g=p['text_diff']['groups'][0];g.update(old_text='a é b',new_text='a 👩‍💻 b',segments=[dict(kind='equal',text='a '),dict(kind='delete',text='é'),dict(kind='insert',text='👩‍💻'),dict(kind='equal',text=' b')])
        gold['edits']=[dict(id='unicode',old='é',new='👩‍💻')]
        self.assertTrue(check_page_revision(diff,gold)['passed'])
        g['segments']=[dict(kind='delete',text='a é b'),dict(kind='insert',text='a 👩‍💻 b')]
        self.assertFalse(check_page_revision(diff,gold)['passed'])

    def test_region_positions_are_invariant_under_word_wrapping_and_empty_source_blocks(self):
        old_words='alpha X beta Y omega'.split();new_words='alpha P beta Q omega'.split()
        for old_width in range(1,6):
            for new_width in range(1,6):
                for empty in [False,True]:
                    with self.subTest(old_width=old_width,new_width=new_width,empty=empty):
                        def lines(words,width):
                            body=[' '.join(words[i:i+width]) for i in range(0,len(words),width)]
                            if empty:body=['']+body+['']
                            return ['Header',*body,'End']
                        a=lines(old_words,old_width);b=lines(new_words,new_width);blocks=[]
                        def block(kind,oi,ni,old,new,segments):
                            box=dict(x0=1,y0=50,x1=80,y1=52)
                            return dict(kind=kind,old_index=oi,new_index=ni,old_text=old,new_text=new,
                                old_bounds=box if oi else None,new_bounds=box if ni else None,segments=segments)
                        blocks.append(block('unchanged',1,1,'Header','Header',[]))
                        blocks.extend(block('removed',i,None,t,None,[]) for i,t in enumerate(a[1:-1],2))
                        blocks.extend(block('added',None,i,None,t,[]) for i,t in enumerate(b[1:-1],2))
                        blocks.append(block('unchanged',len(a),len(b),'End','End',[]))
                        old=' '.join(old_words);new=' '.join(new_words)
                        segments=[dict(kind=k,text=t) for k,t in [('equal','alpha '),('delete','X'),('insert','P'),('equal',' beta '),('delete','Y'),('insert','Q'),('equal',' omega')]]
                        group=dict(old_block_range=[1,len(a)-1],new_block_range=[1,len(b)-1],old_blocks=list(range(2,len(a))),new_blocks=list(range(2,len(b))),old_text=old,new_text=new,segments=segments)
                        page=dict(old_index=1,new_index=1,blocks=blocks,text_diff=dict(groups=[group]))
                        gold=dict(kind='source_regions',old_page=1,new_page=1,scope='Complete paragraph edit region',edits=[dict(id='paragraph',old=old,new=new)])
                        self.assertTrue(check_page_revision(dict(pages=[page]),gold)['passed'])


if __name__=='__main__':unittest.main()
