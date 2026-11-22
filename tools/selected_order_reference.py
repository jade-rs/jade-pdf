"""複数ページの独立した選択ルールを、未達ルールも含めて出典に固定する。"""
import hashlib
import json
import re

from semantic_gold import check_gold


def validate_selected_order_reference(entry, corpus):
    """任意の参照があるとき、元の宣言全体と登録したルールの完全一致を確認する。"""
    if 'selected_order_reference' not in entry:
        return
    refs = entry['selected_order_reference']
    if not isinstance(refs, dict):
        raise ValueError('Invalid selected-order references')
    values = {}
    for kind in ('gold', 'review'):
        name = refs.get(kind+'_file')
        if not isinstance(name, str) or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.-]{0,127}\.json', name):
            raise ValueError('Invalid selected-order reference path')
        path = corpus/name
        if path.resolve().parent != corpus.resolve() or path.stat().st_size > 2_000_000:
            raise ValueError('Selected-order reference is outside corpus or oversized')
        data = path.read_bytes()
        if hashlib.sha256(data).hexdigest() != refs.get(kind+'_sha256'):
            raise ValueError('Selected-order reference SHA-256 mismatch')
        values[kind] = json.loads(data)
    gold, review = values['gold'], values['review']
    pages = entry.get('pages')
    if (not isinstance(gold, dict) or not isinstance(review, dict)
            or type(pages) is not int or not 1 <= pages <= 10_000
            or gold.get('source_id') != entry['id'] or gold.get('source_sha256') != entry['sha256']
            or gold.get('source_url') != entry['url']
            or gold.get('product_output_inspected_before_initial_declaration') is not False
            or review.get('id') != entry['id'] or review.get('source_sha256') != entry['sha256']
            or review.get('product_output_inspected_before_declaration') is not False
            or review.get('original_pages_reviewed') != list(range(1, pages+1))
            or any(type(page) is not int for page in review['original_pages_reviewed'])):
        raise ValueError('Selected-order source or original-page review mismatch')
    expected = gold.get('reading_order')
    primary = entry.get('gold')
    additional = entry.get('reading_order', [])
    if (not isinstance(expected, list) or not 1 <= len(expected) <= 32
            or not isinstance(primary, dict) or primary.get('kind') != 'reading_order'
            or not isinstance(additional, list) or len(additional) > 31):
        raise ValueError('Selected-order reference requires bounded primary and additional rules')

    def signature(rule):
        if (not isinstance(rule, dict) or type(rule.get('page')) is not int
                or not 1 <= rule['page'] <= pages
                or not isinstance(rule.get('case_id'), str)
                or not re.fullmatch(r'[A-Za-z0-9_-]{1,128}', rule['case_id'])):
            raise ValueError('Invalid selected-order rule identity or page')
        check_gold('', dict(rule, kind='reading_order'))
        return {key: rule[key] for key in ('case_id', 'page', 'anchors', 'scope')}

    declared = [signature(rule) for rule in expected]
    registered = [signature(rule) for rule in [primary, *additional]]
    if len({rule['case_id'] for rule in declared}) != len(declared) or registered != declared:
        raise ValueError('Registered selected-order rules differ from complete original declaration')
