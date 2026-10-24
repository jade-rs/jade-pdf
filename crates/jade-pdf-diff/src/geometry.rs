//! Projection from unrotated PDF user space to a rendered, top-left pixel viewport.

use anyhow::{ensure, Context, Result};
use pdfium_render::prelude::PdfPage;
use serde::{Deserialize, Serialize};

use crate::Bounds;

/// Affine matrix in SVG/Canvas order: `x' = a*x + c*y + e`, `y' = b*x + d*y + f`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AffineTransform {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl AffineTransform {
    pub fn transform_point(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// Axis-aligned bounds enclosing all four transformed corners.
    pub fn transform_bounds(&self, bounds: Bounds) -> Bounds {
        let corners = [
            self.transform_point(bounds.x0, bounds.y0),
            self.transform_point(bounds.x1, bounds.y0),
            self.transform_point(bounds.x0, bounds.y1),
            self.transform_point(bounds.x1, bounds.y1),
        ];
        Bounds {
            x0: corners.iter().map(|p| p.0).fold(f64::INFINITY, f64::min),
            y0: corners.iter().map(|p| p.1).fold(f64::INFINITY, f64::min),
            x1: corners
                .iter()
                .map(|p| p.0)
                .fold(f64::NEG_INFINITY, f64::max),
            y1: corners
                .iter()
                .map(|p| p.1)
                .fold(f64::NEG_INFINITY, f64::max),
        }
    }

    pub fn inverse(&self) -> Result<Self> {
        let determinant = self.a * self.d - self.b * self.c;
        ensure!(
            determinant.is_finite() && determinant != 0.0,
            "Non-invertible viewport transform"
        );
        let inverse = Self {
            a: self.d / determinant,
            b: -self.b / determinant,
            c: -self.c / determinant,
            d: self.a / determinant,
            e: (self.c * self.f - self.d * self.e) / determinant,
            f: (self.b * self.e - self.a * self.f) / determinant,
        };
        inverse.validate()?;
        Ok(inverse)
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            [self.a, self.b, self.c, self.d, self.e, self.f]
                .into_iter()
                .all(f64::is_finite),
            "Non-finite viewport transform"
        );
        Ok(())
    }
}

/// Geometry of a rendered page. Pixel coordinates start at the image's top left,
/// with y increasing downward. Bounds and text remain in unrotated PDF user space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PageViewport {
    pub width: u32,
    pub height: u32,
    /// Intersection of the inherited MediaBox and CropBox, before page rotation.
    pub view_box: Bounds,
    pub rotation: u64,
    /// Physical points per PDF user unit; None when source analysis was unavailable.
    #[serde(default = "crate::page_units::default_unit")]
    pub user_unit: Option<f64>,
    pub pdf_to_pixel: AffineTransform,
}

impl PageViewport {
    /// Use the actual bitmap dimensions; rounding may give the two axes slightly
    /// different scales, even when rendering with a single requested DPI.
    pub fn new(view_box: Bounds, rotation: u64, width: u32, height: u32) -> Result<Self> {
        validate_view_box(view_box)?;
        ensure!(
            width > 0 && height > 0,
            "Viewport dimensions must be positive"
        );
        let w = f64::from(width);
        let h = f64::from(height);
        let Bounds { x0, y0, x1, y1 } = view_box;
        let (sx, sy) = if rotation == 90 || rotation == 270 {
            (w / view_box.height(), h / view_box.width())
        } else {
            (w / view_box.width(), h / view_box.height())
        };
        let pdf_to_pixel = match rotation {
            0 => AffineTransform {
                a: sx,
                b: 0.0,
                c: 0.0,
                d: -sy,
                e: -x0 * sx,
                f: y1 * sy,
            },
            90 => AffineTransform {
                a: 0.0,
                b: sy,
                c: sx,
                d: 0.0,
                e: -y0 * sx,
                f: -x0 * sy,
            },
            180 => AffineTransform {
                a: -sx,
                b: 0.0,
                c: 0.0,
                d: sy,
                e: x1 * sx,
                f: -y0 * sy,
            },
            270 => AffineTransform {
                a: 0.0,
                b: -sy,
                c: -sx,
                d: 0.0,
                e: y1 * sx,
                f: x1 * sy,
            },
            _ => anyhow::bail!("Invalid viewport rotation: {rotation}"),
        };
        pdf_to_pixel.validate()?;
        Ok(Self {
            width,
            height,
            view_box,
            rotation,
            user_unit: Some(1.0),
            pdf_to_pixel,
        })
    }

    /// Project PDF bounds, then clip to the image. Returns `None` for invisible
    /// or zero-area bounds. The resulting `(x0,y0)` is the pixel top-left corner.
    pub fn clipped_bounds(&self, bounds: Bounds) -> Option<Bounds> {
        if ![bounds.x0, bounds.y0, bounds.x1, bounds.y1]
            .into_iter()
            .all(f64::is_finite)
            || bounds.width() <= 0.0
            || bounds.height() <= 0.0
        {
            return None;
        }
        let bounds = self.pdf_to_pixel.transform_bounds(bounds);
        let clipped = Bounds {
            x0: bounds.x0.max(0.0),
            y0: bounds.y0.max(0.0),
            x1: bounds.x1.min(f64::from(self.width)),
            y1: bounds.y1.min(f64::from(self.height)),
        };
        (clipped.width() > 0.0 && clipped.height() > 0.0).then_some(clipped)
    }
}

pub(crate) fn validate_view_box(bounds: Bounds) -> Result<()> {
    ensure!(
        [
            bounds.x0,
            bounds.y0,
            bounds.x1,
            bounds.y1,
            bounds.width(),
            bounds.height()
        ]
        .into_iter()
        .all(f64::is_finite)
            && bounds.width() > 0.0
            && bounds.height() > 0.0,
        "Invalid PDF view box"
    );
    Ok(())
}

pub(crate) fn read_page_geometry(page: &PdfPage<'_>) -> Result<(Bounds, u64)> {
    // FPDF_GetPageBoundingBox resolves inherited boxes and intersects CropBox
    // with MediaBox. The individual GetCropBox/GetMediaBox calls do not.
    let bounds = page
        .boundaries()
        .bounding()
        .context("Failed to read PDF view box")?
        .bounds;
    let bounds = Bounds {
        x0: f64::from(bounds.left().value),
        y0: f64::from(bounds.bottom().value),
        x1: f64::from(bounds.right().value),
        y1: f64::from(bounds.top().value),
    };
    validate_view_box(bounds)?;
    ensure!(
        page.width().value.is_finite()
            && page.height().value.is_finite()
            && page.width().value > 0.0
            && page.height().value > 0.0,
        "Invalid PDF page dimensions"
    );
    let rotation = page
        .rotation()
        .context("Failed to read PDF page rotation")?
        .as_degrees() as u64;
    Ok((bounds, rotation))
}
