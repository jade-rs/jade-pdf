import * as pdfjsLib from "pdfjs-dist";
import pdfWorkerUrl from "pdfjs-dist/build/pdf.worker.mjs?url";
import init, {
  convertPdfToDocx,
  convertRenderedPngPagesToDocx,
  inspectPdfJson,
} from "../pkg/jade_pdf2docx_wasm.js?v=browser-structured-tables-20260516";
import "./style.css";

const BUILD_LABEL = "browser-structured-tables-20260516";
const PDF_RENDER_SCALE = 2;
const CSS_PX_PER_PDF_POINT = 96 / 72;
const PDFJS_CMAP_URL = `${import.meta.env.BASE_URL}cmaps/`;
const PDFJS_STANDARD_FONT_URL = `${import.meta.env.BASE_URL}standard_fonts/`;
const INK_SAMPLE_STRIDE = 8;
pdfjsLib.GlobalWorkerOptions.workerSrc = pdfWorkerUrl;

const input = document.querySelector("#pdf-input");
const convertButton = document.querySelector("#convert-button");
const inspectOutput = document.querySelector("#inspect-output");
const downloadLink = document.querySelector("#download-link");
const statusText = document.querySelector("#status-text");

let selectedBytes = null;
let selectedName = "jade-pdf2docx-output.docx";
let selectedInfo = null;

async function boot() {
  await init();
  statusText.textContent = `WASM ready (${BUILD_LABEL})`;
  input.disabled = false;
}

input.disabled = true;
boot().catch((error) => {
  statusText.textContent = `WASM init failed: ${error}`;
});

input.addEventListener("change", async () => {
  const file = input.files?.[0];
  downloadLink.hidden = true;
  convertButton.disabled = true;
  selectedBytes = null;
  selectedInfo = null;

  if (!file) {
    inspectOutput.textContent = "PDF を選択してください。";
    statusText.textContent = "";
    return;
  }

  selectedName = `${file.name.replace(/\.pdf$/i, "") || "jade-pdf2docx-output"}.docx`;
  selectedBytes = new Uint8Array(await file.arrayBuffer());
  try {
    const info = JSON.parse(inspectPdfJson(selectedBytes));
    selectedInfo = info;
    inspectOutput.textContent = JSON.stringify(info, null, 2);
    convertButton.disabled = false;
    statusText.textContent = `${file.name} loaded`;
  } catch (error) {
    inspectOutput.textContent = String(error);
    statusText.textContent = "PDF parse failed";
  }
});

convertButton.addEventListener("click", async () => {
  if (!selectedBytes) {
    return;
  }
  convertButton.disabled = true;
  downloadLink.hidden = true;
  statusText.textContent = "Converting...";
  try {
    const tableStatus =
      selectedInfo?.inferred_table_rows && selectedInfo?.inferred_table_cols
        ? `, table ${selectedInfo.inferred_table_rows}x${selectedInfo.inferred_table_cols}`
        : "";
    statusText.textContent = "Building structured DOCX...";
    let docxBytes = null;
    let mode = "";
    const hasStructuredContent =
      (selectedInfo?.text_elements_count ?? 0) > 0 ||
      (selectedInfo?.inferred_non_empty_cells ?? 0) > 0 ||
      (selectedInfo?.inferred_table_count ?? 0) > 0;
    if (hasStructuredContent) {
      docxBytes = convertPdfToDocx(selectedBytes);
      mode = "structured w:p/w:tbl";
    } else {
      statusText.textContent = "Rendering PDF pages...";
      const rendered = await renderPdfPagesAsPng(selectedBytes);
      docxBytes = convertRenderedPngPagesToDocx(
        rendered.pngs,
        rendered.widths,
        rendered.heights,
      );
      const minInkRatio = Math.min(...rendered.inkRatios);
      mode = `render fallback ${rendered.pngs.length} page(s), pdf.js text items ${rendered.textItemCount}, blank pages ${rendered.blankPageCount}, min ink ${minInkRatio.toFixed(4)}`;
    }
    const blob = new Blob([docxBytes], {
      type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    });
    const previousUrl = downloadLink.href;
    if (previousUrl.startsWith("blob:")) {
      URL.revokeObjectURL(previousUrl);
    }
    downloadLink.href = URL.createObjectURL(blob);
    downloadLink.download = selectedName;
    downloadLink.hidden = false;
    statusText.textContent = `DOCX ready (${docxBytes.length} bytes${tableStatus}, ${mode}, ${BUILD_LABEL})`;
  } catch (error) {
    statusText.textContent = String(error);
  } finally {
    convertButton.disabled = false;
  }
});

async function renderPdfPagesAsPng(pdfBytes) {
  const loadingTask = pdfjsLib.getDocument(pdfDocumentOptions(pdfBytes));
  const pdf = await loadingTask.promise;
  const pngs = [];
  const widths = [];
  const heights = [];
  const inkRatios = [];
  let textItemCount = 0;
  for (let pageNumber = 1; pageNumber <= pdf.numPages; pageNumber += 1) {
    const page = await pdf.getPage(pageNumber);
    const textContent = await page.getTextContent();
    textItemCount += textContent.items.length;
    const pageViewport = page.getViewport({ scale: 1 });
    const viewport = page.getViewport({ scale: PDF_RENDER_SCALE });
    const canvas = document.createElement("canvas");
    canvas.width = Math.ceil(viewport.width);
    canvas.height = Math.ceil(viewport.height);
    const context = canvas.getContext("2d", { alpha: false });
    context.fillStyle = "#ffffff";
    context.fillRect(0, 0, canvas.width, canvas.height);
    await page.render({ canvasContext: context, viewport }).promise;
    const inkRatio = measureCanvasInkRatio(context, canvas.width, canvas.height);
    const blob = await new Promise((resolve, reject) => {
      canvas.toBlob((value) => {
        if (value) {
          resolve(value);
        } else {
          reject(new Error("failed to render PDF page to PNG"));
        }
      }, "image/png");
    });
    pngs.push(new Uint8Array(await blob.arrayBuffer()));
    widths.push(Math.round(pageViewport.width * CSS_PX_PER_PDF_POINT));
    heights.push(Math.round(pageViewport.height * CSS_PX_PER_PDF_POINT));
    inkRatios.push(inkRatio);
  }
  const blankPageCount = inkRatios.filter((ratio) => ratio <= 0).length;
  if (textItemCount > 0 && blankPageCount === inkRatios.length) {
    throw new Error("rendered PDF is blank despite extracted text items");
  }
  return { pngs, widths, heights, inkRatios, textItemCount, blankPageCount };
}

function pdfDocumentOptions(pdfBytes) {
  return {
    data: pdfBytes.slice(),
    cMapPacked: true,
    cMapUrl: PDFJS_CMAP_URL,
    standardFontDataUrl: PDFJS_STANDARD_FONT_URL,
  };
}

function measureCanvasInkRatio(context, width, height) {
  const image = context.getImageData(0, 0, width, height).data;
  let sampled = 0;
  let ink = 0;
  for (let y = 0; y < height; y += INK_SAMPLE_STRIDE) {
    for (let x = 0; x < width; x += INK_SAMPLE_STRIDE) {
      const index = (y * width + x) * 4;
      const red = image[index];
      const green = image[index + 1];
      const blue = image[index + 2];
      sampled += 1;
      if (red < 245 || green < 245 || blue < 245) {
        ink += 1;
      }
    }
  }
  return sampled === 0 ? 0 : ink / sampled;
}
