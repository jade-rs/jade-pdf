"""文字対応の限定変更と、読み順だけの変更を混同しない公開セルの対照。"""
import copy
import unittest
from audit_fips202_cells import compare


class PublicCellTransition(unittest.TestCase):
    def test_known_four_minus_changes_preserve_order_and_exact_geometry(self):
        old=[['\uf02d', {'x0':i,'x1':i+1}, {}, 12, [], {}] for i in range(4)]
        new=copy.deepcopy(old)
        for row in new: row[0]='−'
        self.assertTrue(compare(old,new,14)['matches'])
        for mutation in ('drop','duplicate','move','reverse','unknown'):
            bad=copy.deepcopy(new)
            if mutation=='drop': bad.pop()
            if mutation=='duplicate': bad.append(copy.deepcopy(bad[0]))
            if mutation=='move': bad[0][1]['x0']+=1
            if mutation=='reverse': bad.reverse()
            if mutation=='unknown': bad[0][0]='–'
            self.assertFalse(compare(old,bad,14)['matches'],mutation)

    def test_reading_order_change_preserves_literal_cell_ownership(self):
        old=[['b',{'x0':i,'x1':i+1},{},12,[{'anchor':i==0}],{}] for i in range(3)]
        self.assertTrue(compare(old,list(reversed(old)),35)['matches'])
        bad=copy.deepcopy(old);bad[0][4]=[]
        self.assertFalse(compare(old,bad,35)['matches'])
        bad=copy.deepcopy(old);bad[0][0]='−'
        self.assertFalse(compare(old,bad,35)['matches'])
        with self.assertRaises(ValueError):compare(old,old,34)


if __name__=='__main__':unittest.main()
