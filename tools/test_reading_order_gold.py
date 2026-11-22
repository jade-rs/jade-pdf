import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from audit_reading_order import validate_references

class ReadingOrderReferences(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.gold = dict(id='case',source_id='owned',source_sha256='a'*64,page=1,anchors=['A','B'])
        self.review = dict(source_sha256='a'*64,selected_page=1,baseline_product_inspected_for_new_gold=False)
        self.rule = dict(case_id='case',page=1,anchors=['A','B'])
        for kind, value in [('gold',self.gold),('review',self.review)]:
            data=json.dumps(value).encode();(self.root/(kind+'.json')).write_bytes(data)
            self.rule[kind+'_file']=kind+'.json';self.rule[kind+'_sha256']=hashlib.sha256(data).hexdigest()
        self.entry=dict(id='owned',sha256='a'*64,reading_order=[self.rule])

    def test_declared_gold_is_pinned_and_legacy_rules_remain_supported(self):
        validate_references(self.entry,self.root)
        validate_references(dict(reading_order=[dict(page=1,anchors=['legacy'])]),self.root)

    def test_hash_source_and_anchor_changes_are_rejected(self):
        for field,value in [('anchors',['B','A']),('gold_sha256','b'*64),('review_sha256','b'*64),('gold_file','../gold.json'),('page',2)]:
            rule=dict(self.rule);rule[field]=value
            with self.subTest(field=field),self.assertRaises(ValueError):
                validate_references(dict(self.entry,reading_order=[rule]),self.root)

    def test_duplicate_rules_on_one_page_do_not_replace_prior_notice(self):
        entry=dict(self.entry,reading_order=[dict(page=1,anchors=['notice']),self.rule])
        validate_references(entry,self.root)
        self.assertEqual(len(entry['reading_order']),2)

if __name__=='__main__':unittest.main()
