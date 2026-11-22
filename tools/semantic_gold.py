"""Keep full-page text gold and explicitly scoped scalar inventories distinct."""
from collections import Counter
import unicodedata
from quantity_gold import check_quantity_gold


def normalize_text(text):
    """Canonical spelling and whitespace only; retain order, marks and joiners."""
    return ' '.join(unicodedata.normalize('NFC', text).split())


def compact_fragment_matches(actual, gold):
    """任意の部分文字列goldは、指定があるときだけ独立に検証する。"""
    if 'compact_fragment' not in gold:
        return True
    fragment = gold['compact_fragment']
    if not isinstance(fragment, str) or not fragment.strip():
        raise ValueError('Compact fragment requires nonempty text')
    return ''.join(fragment.split()) in ''.join(actual.split())


def page_has_text_review(page):
    """Require review evidence on the checked page, not only on another page."""
    return (not any(block['text'].strip() for block in page['blocks'])
            or any(page.get(key, 0) > 0 for key in (
                'sdk_unicode_mapping_errors', 'sdk_text_objects_without_characters',
                'source_text_characters_missing', 'source_unicode_scalars_unmatched', 'nonpainting_text_characters'))
            or any(page.get(key, False) for key in (
                'source_text_accounting_unavailable', 'source_unicode_accounting_unavailable',
                'reading_order_requires_review', 'text_visibility_accounting_unavailable')))


def inventory_spec(gold):
    """Validate a bounded literal-scalar inventory before inspecting output."""
    ranges = gold.get('ranges')
    count = gold.get('count_each')
    scope = gold.get('scope')
    if (not isinstance(ranges, list) or not ranges or len(ranges) > 4096
            or type(count) is not int or count < 1
            or not isinstance(scope, str) or not scope.strip()):
        raise ValueError('Scalar inventory requires bounded ranges, a positive count and scope')
    codes = set()
    for pair in ranges:
        if (not isinstance(pair, list) or len(pair) != 2
                or any(type(c) is not int for c in pair)):
            raise ValueError('Invalid scalar range')
        start, end = pair
        if (start < 0 or end > 0x10ffff or start > end
                or start <= 0xdfff and end >= 0xd800
                or len(codes) + end - start + 1 > 4096):
            raise ValueError('Scalar range is invalid or exceeds the 4096-scalar budget')
        selected = set(range(start, end + 1))
        if codes.intersection(selected):
            raise ValueError('Overlapping scalar ranges')
        codes.update(selected)
    if len(codes) * count > 2_000_000:
        raise ValueError('Expected inventory exceeds the scalar accounting budget')
    return sorted(codes), count


def check_gold(actual, gold):
    """Return the exact check kind and evidence; an inventory is not full-page gold."""
    kind = gold.get('kind', 'full_page_text')
    if kind == 'quantity_context':
        return check_quantity_gold(actual, gold, normalize_text)
    if kind == 'reading_order':
        anchors, scope = gold.get('anchors'), gold.get('scope')
        if (not isinstance(anchors, list) or not 1 <= len(anchors) <= 256
                or any(not isinstance(anchor, str) or not normalize_text(anchor)
                       or len(anchor) > 2048 for anchor in anchors)
                or sum(map(len, anchors)) > 32768
                or not isinstance(scope, str) or not normalize_text(scope) or len(scope) > 2048):
            raise ValueError('Reading order requires bounded nonempty anchors and scope')
        text, offset, matched = normalize_text(actual).replace(' ', ''), 0, 0
        for anchor in anchors:
            value = normalize_text(anchor).replace(' ', '')
            at = text.find(value, offset)
            if at < 0:
                break
            matched += 1
            offset = at + len(value)
        return dict(kind=kind, scope=scope, matches=matched == len(anchors),
                    expected_anchors=anchors, matched_prefix=matched)
    if kind == 'text_context':
        values = [gold.get(key) for key in ('text', 'anchor', 'scope')]
        if any(not isinstance(value, str) or not value.strip() or len(value) > 2048 for value in values):
            raise ValueError('Text context requires bounded nonempty text, anchor and scope')
        expected, anchor = map(normalize_text, values[:2])
        if expected.count(anchor) != 1:
            raise ValueError('Text context anchor must occur once within the expected context')
        normalized = normalize_text(actual)
        count, anchors = normalized.count(expected), normalized.count(anchor)
        return dict(kind=kind, scope=gold['scope'], matches=count == anchors == 1,
                    expected_context=gold['text'], context_occurrences=count, anchor_occurrences=anchors)
    if kind == 'full_page_text':
        expected = gold.get('text')
        if not isinstance(expected, str):
            raise ValueError('Full-page text gold requires expected text')
        return dict(kind=kind, matches=normalize_text(actual) == normalize_text(expected),
                    expected_text=expected, actual_text=actual)
    if kind != 'scalar_inventory':
        raise ValueError('Unknown semantic gold kind')
    codes, count = inventory_spec(gold)
    forbidden = gold.get('forbidden_scalars', [])
    if (not isinstance(forbidden, list) or len(forbidden) > 4096
            or any(type(c) is not int or not 0 <= c <= 0x10ffff or 0xd800 <= c <= 0xdfff for c in forbidden)
            or len(set(forbidden)) != len(forbidden)):
        raise ValueError('Forbidden scalars require a bounded unique scalar list')
    selected = set(codes)
    observed = Counter(ord(c) for c in actual if ord(c) in selected)
    expected = {f'U+{c:05X}': count for c in codes}
    inventory = {f'U+{c:05X}': observed[c] for c in codes}
    forbidden_inventory = {f'U+{c:05X}': actual.count(chr(c)) for c in forbidden}
    return dict(kind=kind, scope=gold['scope'], matches=inventory == expected and not any(forbidden_inventory.values()),
                expected_inventory=expected, actual_inventory=inventory,
                forbidden_inventory=forbidden_inventory)
