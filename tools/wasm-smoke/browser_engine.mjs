// 要求したエンジンを明示的に選び、別ブラウザーでの成功を記録しない。
export function selectBrowser(playwright, env=process.env) {
  const engine=env.JADE_PDF_BROWSER_ENGINE ?? 'chromium';
  if(!['chromium','firefox','webkit'].includes(engine))
    throw new Error('Unsupported browser engine: '+engine);
  return {engine,launcher:playwright[engine],options:{headless:true,
    ...(env.JADE_PDF_BROWSER ? {executablePath:env.JADE_PDF_BROWSER} : {})}};
}
