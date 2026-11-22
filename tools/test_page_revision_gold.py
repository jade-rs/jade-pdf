"""ページ全体の編集goldが、未宣言の追加や欠落・重複・順序違いを見逃さない対照。"""
import copy
import unittest

from page_revision_gold import check_page_revision


def fixture():
    """異なる位置の同じ年表示と、折り返した追加段落を持つ独立した出力契約。"""
    def year(old, new, index, new_index, y):
        return dict(kind='changed',old_index=index,new_index=new_index,
                    old_text=old,new_text=new,old_bounds=dict(x0=1,y0=y,x1=20,y1=y+2),
                    new_bounds=dict(x0=1,y0=y,x1=20,y1=y+2),
                    segments=[dict(kind='equal',text=old[:-2] if old.endswith(')') else old[:-1]),
                              dict(kind='delete',text='4'),dict(kind='insert',text='5')]
                             +([dict(kind='equal',text=')')] if old.endswith(')') else []))
    def added(text,index):
        return dict(kind='added',old_index=None,new_index=index,old_text=None,new_text=text,
                    old_bounds=None,new_bounds=dict(x0=1,y0=60-index,x1=90,y1=62-index),segments=[])
    size=dict(height=100,width=100,rotation=0,view_box=dict(x0=0,y0=0,x1=100,y1=100))
    page=dict(old_index=1,new_index=1,old_size=size,new_size=copy.deepcopy(size),
              blocks=[year('2024','2025',1,1,90),added('TIP: Use',2),
                      added('the estimator.',3),year('(2024)','(2025)',2,4,5)])
    gold=dict(old_page=1,new_page=1,scope='All logical text edit regions on one selected page',edits=[
        dict(id='top',old='4',new='5',zone='upper_quarter'),
        dict(id='tip',old='',new='TIP: Use the estimator.'),
        dict(id='bottom',old='4',new='5',zone='lower_quarter')])
    return dict(pages=[page]),gold,added


def page_view(diff):
    """原本の範囲を明示した、三つのページ本文編集の独立した契約例。"""
    b=diff['pages'][0]['blocks']
    def group(a,c,old_ids,new_ids,old,new,segments):
        return dict(old_block_range=a,new_block_range=c,old_blocks=old_ids,new_blocks=new_ids,
                    old_text=old,new_text=new,segments=copy.deepcopy(segments))
    diff['pages'][0]['text_diff']=dict(groups=[
        group([0,1],[0,1],[1],[1],'2024','2025',b[0]['segments']),
        group([1,1],[1,3],[],[2,3],'','TIP: Use the estimator.',[dict(kind='insert',text='TIP: Use the estimator.')]),
        group([1,2],[3,4],[2],[4],'(2024)','(2025)',b[3]['segments'])])


def wrapped_tail(diff):
    """文字列の順序を保持したまま、最後の二行の折り返し位置だけを変える。"""
    for old,new,index,segments in [
        ('If you','If',3,[dict(kind='equal',text='If'),dict(kind='delete',text=' you')]),
        ('agree','you agree',4,[dict(kind='insert',text='you '),dict(kind='equal',text='agree')])]:
        diff['pages'][0]['blocks'].append(dict(kind='changed',old_index=index,new_index=index+2,old_text=old,new_text=new,
            old_bounds=dict(x0=1,y0=50-index,x1=90,y1=52-index),
            new_bounds=dict(x0=1,y0=50-index,x1=90,y1=52-index),segments=segments))


class PageRevisionGold(unittest.TestCase):
    def test_runtime_json_bridge_preserves_failures_and_review_limits(self):
        from audit_page_revision import check_payload
        diff,gold,_=fixture();page_view(diff)
        result=check_payload(dict(gold=gold,diff=diff))
        self.assertTrue(result['passed']);self.assertEqual(result,check_page_revision(diff,gold))
        diff['pages'][0]['text_diff']['alignment_limited']=True
        self.assertFalse(check_payload(dict(gold=gold,diff=diff))['passed'])
        diff['pages'][0]['text_diff']['groups']=[]
        with self.assertRaises(ValueError):check_payload(dict(gold=gold,diff=diff))

    def test_complete_text_edit_set_is_independent_of_added_line_wrapping(self):
        diff,gold,added=fixture()
        for cut in range(1,4):
            words=gold['edits'][1]['new'].split();page=diff['pages'][0]
            page['blocks'][1:3]=[added(' '.join(words[:cut]),2),added(' '.join(words[cut:]),3)]
            result=check_page_revision(diff,gold)
            self.assertTrue(result['passed']);self.assertEqual(result['true_positives'],3)
            self.assertEqual(result['false_positives'],0);self.assertEqual(result['false_negatives'],0)

    def test_extra_text_and_missing_expected_edit_are_not_hidden_by_context_matches(self):
        diff,gold,added=fixture();diff['pages'][0]['blocks'].append(added('unreported extra text',5))
        result=check_page_revision(diff,gold)
        self.assertFalse(result['passed']);self.assertEqual(result['false_positives'],1)
        diff,gold,_=fixture();b=diff['pages'][0]['blocks'][0]
        b.update(kind='unchanged',new_text=b['old_text'],segments=[])
        result=check_page_revision(diff,gold)
        self.assertFalse(result['passed']);self.assertEqual(result['false_negatives'],1)

    def test_duplicate_or_reordered_output_cannot_satisfy_the_same_gold_twice(self):
        diff,gold,_=fixture();b=copy.deepcopy(diff['pages'][0]['blocks'][0]);b.update(old_index=3,new_index=5)
        diff['pages'][0]['blocks'].append(b);result=check_page_revision(diff,gold)
        self.assertFalse(result['passed']);self.assertEqual(result['false_negatives'],1)
        diff,gold,_=fixture();a,b=diff['pages'][0]['blocks'][1:3];a['new_text'],b['new_text']=b['new_text'],a['new_text']
        result=check_page_revision(diff,gold)
        self.assertFalse(result['passed']);self.assertEqual(result['false_negatives'],1)

    def test_malformed_segments_pages_and_nonfinite_coordinates_are_rejected(self):
        for mutation in ['segments','page','bool-page','added-segments','bounds','duplicate-index']:
            diff,gold,_=fixture();page=diff['pages'][0]
            if mutation=='segments':page['blocks'][0]['segments'].pop(1)
            if mutation=='page':page['new_index']=2
            if mutation=='bool-page':page['old_index']=True
            if mutation=='added-segments':page['blocks'][1]['segments']=[dict(kind='insert',text='hidden different text')]
            if mutation=='bounds':page['blocks'][0]['new_bounds']['y0']=float('nan')
            if mutation=='duplicate-index':page['blocks'][1]['new_index']=1
            with self.assertRaises(ValueError,msg=mutation):check_page_revision(diff,gold)

    def test_matching_word_inventory_does_not_cancel_two_unexpected_line_hunks(self):
        diff,gold,_=fixture();wrapped_tail(diff)
        result=check_page_revision(diff,gold)
        self.assertFalse(result['passed']);self.assertEqual(result['true_positives'],3)
        self.assertEqual(result['false_positives'],2);self.assertEqual(result['false_negatives'],0)

    def test_verified_page_view_handles_reflow_while_preserving_raw_evidence(self):
        diff,gold,_=fixture();page_view(diff);wrapped_tail(diff)
        result=check_page_revision(diff,gold)
        self.assertTrue(result['passed']);self.assertEqual(result['view'],'page_text')
        self.assertEqual(result['raw_block_observations'],6)
        self.assertEqual(result['false_positives'],0)

    def test_page_view_cannot_omit_source_edits_or_spoof_ranges_and_segments(self):
        for mutation in ['missing','overlap','text','segments','new-extra']:
            diff,gold,added=fixture();page_view(diff);groups=diff['pages'][0]['text_diff']['groups']
            if mutation=='missing':groups.pop(0)
            if mutation=='overlap':groups[1]['old_block_range']=[0,1]
            if mutation=='text':groups[1]['new_text']='TIP: invented words'
            if mutation=='segments':groups[0]['segments'].pop(1)
            if mutation=='new-extra':diff['pages'][0]['blocks'].append(added('hidden extra text',5))
            with self.assertRaises(ValueError,msg=mutation):check_page_revision(diff,gold)

    def test_limited_page_view_is_not_accepted_as_complete_text_gold(self):
        diff,gold,_=fixture();page_view(diff)
        diff['pages'][0]['text_diff']['alignment_limited']=True
        result=check_page_revision(diff,gold)
        self.assertFalse(result['passed']);self.assertTrue(result['alignment_limited'])


if __name__=='__main__':unittest.main()
