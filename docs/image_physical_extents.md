# PDF image physical extents

Upright parsed images with finite positive bounds consistent with the parser's
display-pixel estimates retain their PDF
physical width and height in DrawingML. Each axis converts directly from PDF
points to EMUs (12,700 EMUs per point), rounding once. The PNG's resolution and
the parser's integer display-pixel estimates do not determine that physical
size. Both `wp:extent` and `a:xfrm/a:ext` use the same dimensions. Image bytes
remain unchanged.

Images whose bounds are unknown, zero, non-finite, too large for the drawing
API, inconsistent with the existing display-pixel estimates, or rotated retain
the existing display-pixel sizing. This change does not
solve rotated-image transforms, clipping, or complete page fidelity. The
separate rendered-PNG API continues to honor its caller-provided pixel sizes.

In particular, existing viewport normalization can clamp an image's bounds to
CropBox without cropping the PNG payload or changing its earlier display-pixel
estimate. Such mismatched bounds must not resize the full payload to the
clamped rectangle. A generated 150 × 210 pt image with a 100 × 180 pt CropBox
guards this fallback. Agreement with an integer display estimate alone does
not prove complete clipping accuracy; small subpixel clips, image offsets and
uncropped payloads remain limitations.

The public byte converter is tested with an independently constructed 1-pixel
RGB PDF. A 125.25 × 173.5 pt image must produce 1,590,675 × 2,203,450 EMUs. A
64-case property test varies both quarter-point axes independently. An older
50 × 40 pt CTM regression now expects 635,000 × 508,000 EMUs directly, replacing
its former 67 × 53 display-pixel expectation.

## Reviewed USGS scan

The previously registered five-page USGS conversion-factor scan has five
full-page images. All five output image extents now match independently read
source bounds exactly in integer EMUs. This is a scoped geometry result;
editable OCR and table accuracy are separate requirements.

LibreOfficeDev 26.8.0.0.alpha0, build
`2c87e51eeaa2b413ff4ae097b2705eea1995d8e5`, still renders page 4 as
558.4535433 × 631.2472441 pt, while the source is 557.28 × 631.26 pt and the
DOCX section is 557.30 × 631.25 pt (nearest twip). This width mismatch also
occurs with every drawing removed, with page 4 isolated, with unique drawing
IDs, and with the section height rounded upward. It is not evidence that the
image-size fix failed or that the whole output passed.

The runtime's [DOCX importer](https://github.com/LibreOffice/core/blob/2c87e51eeaa2b413ff4ae097b2705eea1995d8e5/sw/source/writerfilter/dmapper/DomainMapper.cxx)
passes each page-size axis through `PaperInfo::sloppyFitPageDimension`.
Its [paper-size implementation](https://github.com/LibreOffice/core/blob/2c87e51eeaa2b413ff4ae097b2705eea1995d8e5/i18nutil/source/utility/paper.cxx)
snaps nearby dimensions within 1.25 pt to known sizes, including the 197 mm
width. The observed DOCX-to-ODT width is 19.701 cm. Retained native source sizes
must not be distorted to compensate for this renderer behavior. No independent
Word rendering has qualified this page, and whole-document acceptance remains
false.
