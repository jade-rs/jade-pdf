"""nativeの既定監査が登録gold全件を走査し、SHA不一致を隠さない対照。"""
import contextlib
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import audit_page_revision
from test_page_revision_gold import fixture,page_view
from test_page_region_gold import example


class RegisteredPageAudit(unittest.TestCase):
    def test_all_registered_kinds_are_audited_and_one_bad_sha_fails_the_run(self):
        for tamper in [False,True]:
            with self.subTest(tamper=tamper),tempfile.TemporaryDirectory() as directory:
                root=Path(directory);(root/'corpus').mkdir();binary=root/'binary';binary.write_bytes(b'fake binary')
                a,ga,_=fixture();page_view(a);b,gb=example();refs=[]
                for name,gold in [('atomic',ga),('regions',gb)]:
                    gold.update(id=name,old_id='old',new_id='new',old_sha256='a'*64,new_sha256='b'*64)
                    file=root/'corpus'/(name+'.json');file.write_text(json.dumps(gold))
                    refs.append(dict({k:gold[k] for k in ['id','old_id','new_id','old_sha256','new_sha256']},
                        path='corpus/'+file.name,sha256=hashlib.sha256(file.read_bytes()).hexdigest()))
                if tamper:refs[1]['sha256']='0'*64
                (root/'corpus/manifest.json').write_text(json.dumps(dict(documents=[dict(id='old',sha256='a'*64),dict(id='new',sha256='b'*64)],page_revision_gold=refs)))
                report=root/'result.json'
                with patch.object(audit_page_revision,'ROOT',root),patch.object(audit_page_revision,'fetch'),\
                     patch.object(audit_page_revision,'invoke',side_effect=[(1,a),(1,b)]) as invoke,\
                     patch('sys.argv',['audit','--binary',str(binary),'--report',str(report)]),contextlib.redirect_stdout(io.StringIO()):
                    code=audit_page_revision.main()
                result=json.loads(report.read_text())
                self.assertEqual(result['cases'],2);self.assertEqual(result['passed'],1 if tamper else 2)
                self.assertEqual(len(result['execution_errors']),1 if tamper else 0)
                self.assertEqual(code,1 if tamper else 0);self.assertEqual(invoke.call_count,1 if tamper else 2)
                if not tamper:self.assertEqual({r['gold_kind'] for r in result['results']},{'atomic_hunks','source_regions'})


if __name__=='__main__':unittest.main()
