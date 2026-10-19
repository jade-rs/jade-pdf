mod stub;
pub use stub::StubOcrEngine;

#[cfg(feature = "ndlocr-lite")]
mod ndlocr_lite;
#[cfg(feature = "ndlocr-lite")]
pub use ndlocr_lite::NdlOcrLiteEngine;

#[cfg(feature = "ndlocr-lite")]
pub(crate) mod line_inference;
