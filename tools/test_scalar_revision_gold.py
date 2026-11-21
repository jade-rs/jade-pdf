"""改訂goldの誤検出・過大な変更範囲・文字セル位置を独立に検証する。"""
import copy
import unittest

from scalar_revision_gold import check_revision, check_span


def gold():
    """原本の目視とは分離した、監査器自身の単一文字編集の資料。"""
    return dict(old_page=16, new_page=15, old_context='0≤t<79', new_context='0≤t≤79',
                old_focus=3, new_focus=3, minimum_horizontal_iou=0.75,
                source_cells=dict(old=dict(x0=30, y0=10, x1=40, y1=22),
                                  new=dict(x0=30, y0=10, x1=40, y1=22)))


def comparison():
    """一文字だけを変更する、製品とは独立した差分JSON。"""
    return dict(pages=[dict(old_index=16, new_index=15, blocks=[dict(
        kind='changed', old_index=1, new_index=1, old_text='0 ≤ t < 79', new_text='0 ≤ t ≤ 79',
        old_bounds=dict(x0=0, y0=10, x1=60, y1=22), new_bounds=dict(x0=0, y0=10, x1=60, y1=22),
        segments=[dict(kind='equal', text='0 ≤ t '), dict(kind='delete', text='<'),
                  dict(kind='insert', text='≤'), dict(kind='equal', text=' 79')])])])


class ScalarRevisionTests(unittest.TestCase):
    def test_fragmented_context_can_still_report_a_precise_paired_edit(self):
        value=comparison();block=value['pages'][0]['blocks'][0]
        value['pages'][0]['blocks']=[
            dict(kind='unchanged',old_index=1,new_index=1,old_text='0≤t',new_text='0≤t'),
            block|dict(old_index=2,new_index=2,old_text='<',new_text='≤',segments=[dict(kind='delete',text='<'),dict(kind='insert',text='≤')]),
            dict(kind='unchanged',old_index=3,new_index=3,old_text='79',new_text='79')]
        self.assertTrue(check_revision(value,gold())['passed'])
        layer=[dict(text='0≤t',chars=[dict(x0=i*10,x1=i*10+10) for i in range(3)],bounds=dict(x0=0,y0=10,x1=30,y1=22)),
               dict(text='≤',chars=[dict(x0=30,x1=40)],bounds=dict(x0=30,y0=10,x1=40,y1=22)),
               dict(text='79',chars=[dict(x0=40,x1=50),dict(x0=50,x1=60)],bounds=dict(x0=40,y0=10,x1=60,y1=22))]
        self.assertTrue(check_span(layer,gold(),'new',expected_block=2)['passed'])
        self.assertFalse(check_span(layer,gold(),'new',expected_block=1)['passed'])

    def test_exact_edit_and_page_pair_are_required(self):
        self.assertTrue(check_revision(comparison(), gold())['passed'])
        bad=comparison();bad['pages'][0]['new_index']=16
        self.assertFalse(check_revision(bad, gold())['passed'])
        bad=comparison();bad['pages']*=2
        self.assertFalse(check_revision(bad, gold())['passed'])

    def test_correct_page_text_does_not_excuse_overbroad_delete_insert(self):
        bad=comparison();block=bad['pages'][0]['blocks'][0]
        block['segments']=[dict(kind='delete',text=block['old_text']),dict(kind='insert',text=block['new_text'])]
        self.assertFalse(check_revision(bad,gold())['passed'])
        bad=comparison();bad['pages'][0]['blocks'][0]['new_text']='0'
        self.assertFalse(check_revision(bad,gold())['passed'])

    def test_unchanged_lower_bound_must_not_be_deleted_and_reinserted(self):
        bad=comparison();bad['pages'][0]['blocks'][0]['segments']=[
            dict(kind='equal',text='0 '),dict(kind='delete',text='≤'),dict(kind='insert',text='≤'),
            dict(kind='equal',text=' t '),dict(kind='delete',text='<'),dict(kind='insert',text='≤'),
            dict(kind='equal',text=' 79')]
        self.assertFalse(check_revision(bad,gold())['passed'])

    def test_missing_or_forged_segments_are_rejected(self):
        for segments in [[],[dict(kind='equal',text='0 ≤ t < 79')], [dict(kind='replace',text='≤')]]:
            bad=comparison();bad['pages'][0]['blocks'][0]['segments']=segments
            self.assertFalse(check_revision(bad,gold())['passed'])

    def test_scope_does_not_claim_other_edits_are_correct(self):
        value=comparison();block=value['pages'][0]['blocks'][0]
        block['old_text']='A '+block['old_text'];block['new_text']='B '+block['new_text']
        block['segments']=[dict(kind='delete',text='A'),dict(kind='insert',text='B'),dict(kind='equal',text=' ')]+block['segments']
        self.assertTrue(check_revision(value,gold())['passed'])

    def test_supplementary_context_offsets_are_scalar_based(self):
        spec=gold();spec.update(old_context='A𐐀<X',new_context='A𐐀≤X',old_focus=2,new_focus=2)
        value=comparison();block=value['pages'][0]['blocks'][0]
        block.update(old_text='A𐐀<X',new_text='A𐐀≤X',segments=[dict(kind='equal',text='A𐐀'),dict(kind='delete',text='<'),dict(kind='insert',text='≤'),dict(kind='equal',text='X')])
        self.assertTrue(check_revision(value,spec)['passed'])
        block['new_text']='A𐐀\ufeff≤X'
        self.assertFalse(check_revision(value,spec)['passed'])

    def test_invalid_gold_never_falls_back_to_a_weaker_check(self):
        for change in [dict(old_focus=True),dict(new_focus=99),dict(old_page=0),dict(old_context=''),dict(new_context='0<t≤79'),dict(old_context='A'*4097)]:
            with self.assertRaises(ValueError):check_revision(comparison(),gold()|change)

    def test_character_span_needs_exact_identity_and_source_cell_overlap(self):
        layer=[dict(text='0≤t≤79',chars=[dict(x0=i*10,x1=i*10+10) for i in range(6)],bounds=dict(x0=0,y0=10,x1=60,y1=22))]
        self.assertTrue(check_span(layer,gold(),'new')['passed'])
        bad=copy.deepcopy(layer);bad[0]['chars'][3]=dict(x0=0,x1=1000)
        self.assertFalse(check_span(bad,gold(),'new')['passed'])
        bad=copy.deepcopy(layer);bad[0]['source_text_region']=True
        self.assertFalse(check_span(bad,gold(),'new')['passed'])
        bad=copy.deepcopy(layer);bad[0]['chars'][3]['x0']=float('nan')
        self.assertFalse(check_span(bad,gold(),'new')['passed'])
        self.assertFalse(check_span(layer*2,gold(),'new')['passed'])


if __name__=='__main__':unittest.main()
