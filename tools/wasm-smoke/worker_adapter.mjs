import {servePdfWorker} from '/crates/jade-pdf-diff/wasm/worker_server.mjs';
import {loadPdfOperations} from './worker_operations.mjs';
servePdfWorker(loadPdfOperations);
