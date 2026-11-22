#!/usr/bin/env python3
"""独立したpypdfで暗号化・認証・コピー許可のowned入力を一時領域に生成する。"""
import argparse
import hashlib
import json
from pathlib import Path

from pypdf import PdfWriter
from pypdf.constants import UserAccessPermissions
from pypdf.generic import DictionaryObject, NameObject, DecodedStreamObject


def document(header):
    """外部PDF・Fontを保存せず、既知のASCII本文を持つ1ページをメモリーに作る。"""
    writer = PdfWriter()
    writer.pdf_header = header
    page = writer.add_blank_page(width=200, height=200)
    font = writer._add_object(DictionaryObject({
        NameObject('/Type'): NameObject('/Font'), NameObject('/Subtype'): NameObject('/Type1'),
        NameObject('/BaseFont'): NameObject('/Helvetica'),
    }))
    page[NameObject('/Resources')] = DictionaryObject({NameObject('/Font'): DictionaryObject({NameObject('/F1'): font})})
    stream = DecodedStreamObject()
    stream.set_data(b'BT /F1 12 Tf 10 100 Td (Owned encryption control) Tj ET')
    page[NameObject('/Contents')] = writer._add_object(stream)
    return writer


def main():
    """既存の場所を上書きせず、方式別PDFと照合用SHAを同じ一時領域へ生成する。"""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--destination', type=Path, required=True)
    args = parser.parse_args()
    args.destination.mkdir(parents=True, exist_ok=False)
    variants = [('rc4-40', 'RC4-40', '', True, b'%PDF-1.7'),
                ('aes-128', 'AES-128', '', True, b'%PDF-1.7'),
                ('rc4-128', 'RC4-128', '', True, b'%PDF-1.7'),
                ('aes-256-r5', 'AES-256-R5', '', True, b'%PDF-1.7'),
                ('aes-256', 'AES-256', '', True, b'%PDF-1.7'),
                ('aes-256-pdf20', 'AES-256', '', True, b'%PDF-2.0'),
                ('aes-256-catalog20', 'AES-256', '', True, b'%PDF-1.7'),
                ('user-password', 'AES-256', 'owned-user-password', True, b'%PDF-1.7'),
                ('copy-denied', 'AES-256', '', False, b'%PDF-1.7')]
    rows = []
    for name, algorithm, user, copying, header in variants:
        writer = document(header)
        if name == 'aes-256-catalog20':
            writer.root_object[NameObject('/Version')] = NameObject('/2.0')
        permissions = UserAccessPermissions.PRINT
        if copying:
            permissions |= UserAccessPermissions.EXTRACT
        writer.encrypt(user_password=user, owner_password='owned-control-owner',
                       permissions_flag=permissions, algorithm=algorithm)
        output = args.destination/(name+'.pdf')
        with output.open('xb') as target:
            writer.write(target)
        data = output.read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        output.with_suffix('.pdf.sha256').write_text(digest+'\n')
        rows.append(dict(id=name, algorithm=algorithm, empty_user_password=not bool(user),
                         copying_allowed=copying, header=header.decode(), bytes=len(data), sha256=digest))
    (args.destination/'manifest.json').write_text(json.dumps(dict(
        expected_text='Owned encryption control', generated_in_temporary_directory=True,
        no_external_PDF_or_font_binary=True, cases=rows), indent=2)+'\n')
    print(f'Generated {len(rows)} owned encryption controls in {args.destination}')


if __name__ == '__main__':
    main()
