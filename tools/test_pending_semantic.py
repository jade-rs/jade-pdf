import unittest

from audit_pending_semantic import same_text


class PendingGoldTests(unittest.TestCase):
    def test_order_is_part_of_full_page_semantic_gold(self):
        self.assertFalse(same_text('خل', 'لخ'))
        self.assertTrue(same_text('عربي', 'عربي'))

    def test_marks_digits_and_joiners_are_not_folded_away(self):
        for left, right in [('²', '2'), ('بَ', 'ب'), ('ب\u200cت', 'بت')]:
            self.assertFalse(same_text(left, right))

    def test_only_canonical_spelling_and_whitespace_are_normalized(self):
        self.assertTrue(same_text(' e\u0301\ntext ', 'é text'))


if __name__ == '__main__':
    unittest.main()
