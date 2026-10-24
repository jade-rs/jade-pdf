#!/usr/bin/env python3
"""Inventory raw pinned-Pdfium scalars independently of the Rust extractor.

Offline adjudication of SHA-pinned test inputs only, not a production parser or
an original-Unicode oracle. Raw returned values may be UTF-16 code units,not
Unicode scalar values; the raw_scalar_inventory field retains its historical name.
Run in its own subprocess with a caller's deadline:
the native SDK can crash or stall. This program does not load the Rust library,
perform geometric ordering, filter controls or recover fallback Unicode.
"""
import argparse
from collections import Counter
import ctypes as C
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def sha(data):
    return hashlib.sha256(data).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def bounded_bytes(path, limit):
    require(path.is_file(), 'Expected a regular file')
    with path.open('rb') as stream:
        data = stream.read(limit+1)
    require(len(data) <= limit, 'Input exceeds the offline probe byte budget')
    return data


def inventory(library, document, expected_document_sha256, page_number):
    pins = json.loads((ROOT/'crates/jade-pdf-diff/pdfium-assets.json').read_bytes())
    allowed = {entry['sha256']: entry['bytes']
               for asset in pins['assets'] for entry in asset['libraries'].values()}
    library_bytes = bounded_bytes(library, 128*1024*1024)
    library_hash = sha(library_bytes)
    require(allowed.get(library_hash) == len(library_bytes), 'SDK is not a pinned payload')
    data = bounded_bytes(document, 64*1024*1024)
    require(sha(data) == expected_document_sha256, 'Document SHA-256 mismatch')
    bindings = C.CDLL(str(library.resolve()))

    def api(name, result, arguments):
        function = getattr(bindings, name)
        function.restype = result
        function.argtypes = arguments
        return function

    pointer, integer = C.c_void_p, C.c_int
    init = api('FPDF_InitLibrary', None, [])
    destroy = api('FPDF_DestroyLibrary', None, [])
    load = api('FPDF_LoadMemDocument64', pointer, [pointer, C.c_size_t, C.c_char_p])
    close = api('FPDF_CloseDocument', None, [pointer])
    pages = api('FPDF_GetPageCount', integer, [pointer])
    load_page = api('FPDF_LoadPage', pointer, [pointer, integer])
    close_page = api('FPDF_ClosePage', None, [pointer])
    load_text = api('FPDFText_LoadPage', pointer, [pointer])
    close_text = api('FPDFText_ClosePage', None, [pointer])
    count = api('FPDFText_CountChars', integer, [pointer])
    unicode = api('FPDFText_GetUnicode', C.c_uint, [pointer, integer])
    mapping_error = api('FPDFText_HasUnicodeMapError', integer, [pointer, integer])
    generated = api('FPDFText_IsGenerated', integer, [pointer, integer])
    buffer = C.create_string_buffer(data)
    init()
    doc = page = text = None
    try:
        doc = load(buffer, len(data), None)
        require(doc, 'SDK could not load document')
        page_count = pages(doc)
        require(1 <= page_number <= page_count, 'Page is out of range')
        page = load_page(doc, page_number-1)
        require(page, 'SDK could not load page')
        text = load_text(page)
        require(text, 'SDK could not load text page')
        characters = count(text)
        require(0 <= characters <= 2_000_000, 'SDK character budget exceeded')
        scalars, errors, synthetic = Counter(), 0, 0
        for index in range(characters):
            scalars[f'U+{unicode(text, index):04X}'] += 1
            error, inserted = mapping_error(text, index), generated(text, index)
            require(error in (0, 1) and inserted in (0, 1), 'SDK character flag failed')
            errors += error
            synthetic += inserted
    finally:
        if text:
            close_text(text)
        if page:
            close_page(page)
        if doc:
            close(doc)
        destroy()
    return dict(version=1, methodology=__doc__, pdfium=pins['version'],
                tool_sha256=sha(Path(__file__).read_bytes()),
                library_sha256=library_hash, document_sha256=sha(data),
                document_pages=page_count, page=page_number, characters=characters,
                sdk_unicode_mapping_errors=errors, sdk_generated_characters=synthetic,
                raw_scalar_inventory=dict(sorted(scalars.items())))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--library', type=Path, required=True)
    parser.add_argument('--document', type=Path, required=True)
    parser.add_argument('--document-sha256', required=True)
    parser.add_argument('--page', type=int, default=1)
    args = parser.parse_args()
    result = inventory(args.library, args.document, args.document_sha256, args.page)
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
