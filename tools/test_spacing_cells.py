import unittest
from audit_spacing_cells import records


class SpacingCellTests(unittest.TestCase):
    def test_removed_blank_preserves_supplementary_scalar_cell_and_source_owner(self):
        box=dict(x0=0.,y0=0.,x1=4.,y1=12.)
        region=dict(start=2,end=3,bounds=box,anchor=True)
        old=dict(text='𐐀 B',bounds=box,size=12.,chars=[dict(x0=0.,x1=1.),dict(x0=1.,x1=1.),dict(x0=2.,x1=3.)],source_regions=[region])
        new={**old,'text':'𐐀B','chars':[old['chars'][0],old['chars'][2]],'source_regions':[{**region,'start':1,'end':2}]}
        self.assertEqual(records([old]),records([new]))
        wrong={**new,'source_regions':[{**region,'start':0,'end':1}]}
        self.assertNotEqual(records([old]),records([wrong]))
        wrong={**new,'chars':[old['chars'][0],dict(x0=3.,x1=4.)]}
        self.assertNotEqual(records([old]),records([wrong]))

    def test_malformed_span_count_or_region_index_fails_instead_of_truncating(self):
        run=dict(text='18',bounds={},size=12.,chars=[dict(x0=0.,x1=1.)])
        with self.assertRaises(ValueError):records([run])
        run['chars']*=2
        for start,end in [(True,1),(-1,1),(0,3),(2,1)]:
            run['source_regions']=[dict(start=start,end=end,bounds={},anchor=False)]
            with self.subTest(start=start,end=end), self.assertRaises(ValueError):records([run])


if __name__=='__main__':unittest.main()
