"""限定した文脈の数値を、有界・有理数・一意出現で照合する。"""
from fractions import Fraction
import re
import unicodedata

VULGAR = dict(zip('¼½¾⅐⅑⅒⅓⅔⅕⅖⅗⅘⅙⅚⅛⅜⅝⅞↉',
                  [(1,4),(1,2),(3,4),(1,7),(1,9),(1,10),(1,3),(2,3),
                   (1,5),(2,5),(3,5),(4,5),(1,6),(5,6),(1,8),(3,8),(5,8),(7,8),(0,3)]))
D = r'[0-9]{1,12}'
V = '['+''.join(VULGAR)+']'
F = D+r' *[/⁄] *'+D
NUMBER = re.compile(r'(?P<value>[+−-]?(?:'+D+' +'+F+'|'+D+' *'+V+'|'+F+'|'+V+'|(?:'+D+r')?\.[0-9]{1,12}|'+D+r'))\Z')


def word_part(char):
    """文字・数字・結合文字・joinerを、数量/文脈から暗黙に捨てない。"""
    return char.isalnum() or char == '_' or unicodedata.category(char).startswith('M') or char in '\u200c\u200d\ufeff'


def rational(text):
    """最大64文字の数値表記を、浮動小数点を経由せず完全な量へ変換する。"""
    text = text.replace('−', '-')
    sign = -1 if text.startswith('-') else 1
    text = text.lstrip('+-')
    if text[-1] in VULGAR:
        whole = text[:-1].strip()
        value = Fraction(int(whole) if whole else 0) + Fraction(*VULGAR[text[-1]])
    elif '/' in text or '⁄' in text:
        match = re.fullmatch(r'(?:([0-9]+) +)?([0-9]+) */ *([0-9]+)', text.replace('⁄', '/'))
        if match is None:
            return None
        whole, numerator, denominator = match.groups()
        if int(denominator) == 0:
            return None
        value = Fraction(int(whole) if whole else 0) + Fraction(int(numerator), int(denominator))
    else:
        value = Fraction(text)
    value *= sign
    return f'{value.numerator}/{value.denominator}'


def check_quantity_gold(actual, gold, normalize):
    """一意な文脈の直前だけを解析し、部分数値・重複・過大な処理を成功にしない。"""
    context, scope = gold.get('context'), gold.get('scope')
    numerator, denominator = gold.get('numerator'), gold.get('denominator')
    if (not isinstance(context, str) or not normalize(context) or len(context)>256
            or not isinstance(scope, str) or not normalize(scope) or len(scope)>2048
            or type(numerator) is not int or not 0<=numerator<=1_000_000
            or type(denominator) is not int or not 1<=denominator<=1_000_000):
        raise ValueError('Quantity context requires bounded text, scope and exact rational integers')
    if not isinstance(actual, str) or len(actual)>2_000_000:
        raise ValueError('Quantity context input exceeds the 2-million-scalar budget')
    text, context = normalize(actual), normalize(context)
    values, lexemes = [], []
    at, count = 0, 0
    while True:
        found = text.find(context, at)
        if found < 0:
            break
        at = found + len(context)
        if at < len(text) and word_part(text[at]):
            continue
        count += 1
        if count>32:
            raise ValueError('Quantity context exceeds the 32-occurrence budget')
        prefix = text[max(0,found-64):found].rstrip(' ')
        match = NUMBER.search(prefix)
        if match and match.start()>0:
            previous = prefix[match.start()-1]
            if word_part(previous) or previous in './⁄+−-':
                match = None
        lexeme = match.group('value') if match else None
        lexemes.append(lexeme)
        values.append(rational(lexeme) if lexeme else None)
    expected = rational(f'{numerator}/{denominator}')
    return dict(kind='quantity_context',scope=scope,matches=count==1 and values==[expected],
                expected_value=expected,actual_values=values,context_occurrences=count,numeric_lexemes=lexemes)
