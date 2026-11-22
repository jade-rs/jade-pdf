import unittest
import contextlib
import copy
import io
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
from unittest.mock import patch
import audit_run_grouping as audit
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

    def test_full_audit_reports_geometry_and_size_changes_with_identical_text(self):
        page=dict(blocks=[dict(text='same text',bounds=dict(x0=1.,y0=2.,x1=3.,y1=4.),size=12.)])
        for change,historical in [({},False),(dict(size=11.),False),(dict(bounds=dict(x0=0.,y0=2.,x1=3.,y1=4.)),False),(dict(size=11.),True)]:
            with self.subTest(change=change),tempfile.TemporaryDirectory() as directory:
                root=Path(directory);(root/'corpus').mkdir()
                (root/'corpus/manifest.json').write_text(json.dumps(dict(documents=[dict(id='owned',pages=1,sha256='source')])) )
                baseline=root/'before';baseline.write_bytes(b'before')
                binary=root/'after';binary.write_bytes(b'after')
                report=root/'report.json';after=copy.deepcopy(page);after['blocks'][0].update(change)
                def inspect(argv,**kwargs):
                    return SimpleNamespace(stdout=json.dumps(dict(pages=[page if argv[0]==str(baseline.resolve()) else after])))
                args=['audit','--binary',str(binary),'--baseline',str(baseline),'--report',str(report)]
                if historical:
                    selected=root/'historical.json'
                    selected.write_bytes((root/'corpus/manifest.json').read_bytes())
                    (root/'corpus/manifest.json').write_text(json.dumps(dict(documents=[])))
                    args+=['--manifest',str(selected)]
                with patch('sys.argv',args),patch.object(audit,'ROOT',root),patch.object(audit,'fetch'),patch.object(audit.subprocess,'run',side_effect=inspect),contextlib.redirect_stdout(io.StringIO()):
                    code=audit.main()
                self.assertEqual(code,3 if change else 0)
                result=json.loads(report.read_text())
                self.assertEqual(result['changed_block_inventory_pages'],int(bool(change)))
                self.assertEqual(result['changed_ordered_scalar_pages'],0)


if __name__=='__main__':unittest.main()
