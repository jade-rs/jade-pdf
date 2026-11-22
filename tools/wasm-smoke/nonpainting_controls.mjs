// Shared first-party input checks for the public byte APIs and optional Worker route.
export async function checkNonpaintingControls(page,{worker=false}={}) {
  return page.evaluate(async useWorker=>{
    const probe=window.jadeProbe??window.probe,checks=[];
    if(!probe || (useWorker&&!window.client)) throw new Error('Visibility check runtime is uninitialized');
    for(const mode of [0,3,7]) {
      const old=probe.nonpainting_pdf(mode,false),next=probe.nonpainting_pdf(mode,true),count=mode===0?0:6;
      const ir=JSON.parse(probe.inspect(old)),layer=JSON.parse(probe.layer(old,1));
      const params={old_name:'old',new_name:'new'};
      const diff=JSON.parse(probe.worker_json('compare',old,old,JSON.stringify(params)));
      const validPage=p=>(p.nonpainting_text_characters??0)===count && !p.text_visibility_accounting_unavailable;
      const validDiagnostics=d=>['old','new'].every(side=>(d[side+'_pages_with_nonpainting_text']??0)===(count?1:0)
        && (d[side+'_pages_with_unavailable_text_visibility']??0)===0);
      const a=probe.render(old,1,400),b=probe.render(next,1,400);
      const pixelsEqual=a.length===b.length && a.every((value,i)=>value===b[i]);
      let parity=true,idle=true;
      if(useWorker) {
        const wIr=await client.run('inspect',[old]);
        const wLayer=await client.run('textLayer',[old],{page:1});
        const wDiff=await client.run('compare',[old,old],params);
        parity=JSON.stringify(wIr)===JSON.stringify(ir)&&JSON.stringify(wLayer)===JSON.stringify(layer)&&JSON.stringify(wDiff)===JSON.stringify(diff);
        idle=client.activeWorkers===0;
      }
      const rawText=ir.pages[0].blocks.map(block=>block.text).join('\n');
      checks.push({id:'nonpainting-mode-'+mode,passed:ir.pages.length===1&&validPage(ir.pages[0])&&layer.length>0&&layer.every(validPage)
        &&validDiagnostics(diff.diagnostics)&&rawText.includes('Visible 1/8')&&rawText.includes('Hidden')&&pixelsEqual===(mode!==0)&&parity&&idle,
        mode,expected_nonpainting_records:count,actual_nonpainting_records:ir.pages[0].nonpainting_text_characters??0,
        visibility_unavailable:ir.pages[0].text_visibility_accounting_unavailable??false,pixels_equal:pixelsEqual,worker_parity:useWorker?parity:null,worker_idle:useWorker?idle:null});
    }
    return checks;
  },worker);
}
