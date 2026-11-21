import unittest
from semantic_gold import check_gold
from audit_page_gold import regular_cases


class TextContextTests(unittest.TestCase):
    def test_literal_unique_context_and_anchor_are_both_required(self):
        gold=dict(kind='text_context', text='replaces FIPS 180-3 and specifies',
                  anchor='replaces FIPS', scope='One original printed line; not page prose or geometry')
        self.assertTrue(check_gold('other text\nreplaces FIPS 180-3 and specifies\nmore', gold)['matches'])
        for actual in ['replaces FIPS 1 80-3 and specifies',
                       'replaces FIPS 180-3 and specifies\nreplaces FIPS 1 80-3 and specifies',
                       'replaces FIPS 180-3 and specifies\nreplaces FIPS 180-3 and specifies',
                       'replaces FIPS 180-3 and\u200dspecifies']:
            with self.subTest(actual=actual): self.assertFalse(check_gold(actual,gold)['matches'])

    def test_invalid_context_cannot_be_an_unbounded_or_empty_match(self):
        valid=dict(kind='text_context',text='prefix text',anchor='prefix',scope='One context')
        for change in [dict(text=''),dict(text=' '*3),dict(text='a'*2049),dict(anchor=''),
                       dict(anchor='absent'),dict(scope=''),dict(scope='\u0085'),dict(scope='\u001c'),dict(text=None)]:
            with self.subTest(change=change), self.assertRaises(ValueError):check_gold('',{**valid,**change})

    def test_context_is_additional_and_does_not_replace_page_gold(self):
        page=dict(page=5,kind='full_page_text',text='Disclaimer')
        context=dict(page=1,kind='text_context',text='replaces FIPS',anchor='replaces',scope='One phrase')
        entry=dict(id='bulletin',gold=page,context_gold=[context])
        self.assertEqual(regular_cases(dict(documents=[entry])),[(entry,page),(entry,context)])

    def test_malformed_context_lists_do_not_silently_skip_gold(self):
        for contexts in [None,{},[dict(kind='text_context')]*33,[dict(kind='full_page_text')]]:
            with self.subTest(contexts=contexts), self.assertRaises(ValueError):
                regular_cases(dict(documents=[dict(id='bad',context_gold=contexts)]))


if __name__=='__main__':unittest.main()
