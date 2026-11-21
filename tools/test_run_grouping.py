import unittest
from audit_run_grouping import block_inventory


class BlockInventoryTests(unittest.TestCase):
    def test_order_and_inferred_whitespace_do_not_change_geometric_ownership(self):
        a=dict(text='1 8',bounds=dict(x0=1.,y0=2.,x1=3.,y1=4.),size=12.)
        b=dict(text='tail',bounds=dict(x0=5.,y0=2.,x1=6.,y1=4.),size=12.)
        self.assertEqual(block_inventory(dict(blocks=[a,b])),
                         block_inventory(dict(blocks=[b,{**a,'text':'18'}])))

    def test_changed_scalars_or_cell_ownership_are_detected(self):
        a=dict(text='18',bounds=dict(x0=1.,y0=2.,x1=3.,y1=4.),size=12.)
        original=block_inventory(dict(blocks=[a]))
        for change in [dict(text='17'),dict(bounds={**a['bounds'],'x0':0.}),dict(size=11.)]:
            self.assertNotEqual(original,block_inventory(dict(blocks=[{**a,**change}])))


if __name__=='__main__':unittest.main()
