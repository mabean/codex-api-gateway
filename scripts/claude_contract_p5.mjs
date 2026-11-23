// Executes pinned original SDK request, SSE decoder, accumulator and abort code.
import fs from 'node:fs';
import vm from 'node:vm';
const [directory,base,upstream,casesPath]=process.argv.slice(2);
const cases=JSON.parse(fs.readFileSync(casesPath)).cases;
const check=(v,m)=>{if(!v)throw Error(m);};
function canonical(x){return Array.isArray(x)?x.map(canonical):x&&typeof x==='object'?Object.fromEntries(Object.keys(x).sort().map(k=>[k,canonical(x[k])])):x;}
const equal=(a,b,m)=>check(JSON.stringify(canonical(a))===JSON.stringify(canonical(b)),m+' '+JSON.stringify({actual:a,expected:b}));
const context=vm.createContext({console,Headers,TextEncoder,TextDecoder,ReadableStream,AbortController,Buffer,setTimeout,clearTimeout,
 Ao:class extends Error{},Jc:class extends Error{},Zdt:e=>e?.name==='AbortError',ni:'x-stainless-helper-method',
 nd:class{constructor(){throw Error('native tool runner unavailable');}},rb:class extends Error{},
 Fs:(_name,n)=>n,lv:()=>({info(){}}),qt(){},$t(){},Gu:async body=>body?.cancel(),qu(){},Lzn:e=>e,Ns:JSON.parse,Error});
vm.runInContext(fs.readFileSync(directory+'/sdk-source.js','utf8')+'\nglobalThis.sdk={zi,zt,ff};',context,{timeout:2000});
const {zi,zt,ff}=context.sdk;
async function localFetch(url,options={}){
 const parsed=new URL(url);check([base,upstream].includes(parsed.origin)&&parsed.hostname==='127.0.0.1','non-loopback transport');
 return fetch(url,{...options,redirect:'error',signal:options.signal??AbortSignal.timeout(10000)});
}
async function control(path,body={}){const r=await localFetch(upstream+path,{method:'POST',body:JSON.stringify(body)});check(r.ok,'control');return r.json();}
let redirectRejected=false;try{await localFetch(upstream+'/redirect');}catch{redirectRejected=true;}check(redirectRejected,'redirect allowed');
const wires=[];
let signalForwarded=false;
function sdkClient(){return {_options:{timeout:10000},post(path,options){
 const headers=new Headers(options.headers?.values);headers.set('content-type','application/json');headers.set('anthropic-version','2023-06-01');headers.set('x-api-key','p5-synthetic-api-key');
 wires.push({path,body:options.body});
 // This is a transport adapter, not a surrogate client. The original ff signal
 // goes to the actual fetch and the original zt controller for cancellation.
 const controller=new AbortController();
 if(options.signal){signalForwarded=true;options.signal.addEventListener('abort',()=>controller.abort(),{once:true});if(options.signal.aborted)controller.abort();}
 const raw=localFetch(base+path,{method:'POST',headers,body:JSON.stringify(options.body),signal:options.signal}).then(async response=>{
  if(!response.ok)throw Error('HTTP '+response.status+' '+await response.text());
  return {response,data:options.body.stream?zt.fromSSEResponse(response,controller):await response.json()};
 });
 const promise=raw.then(v=>v.data);promise.catch(()=>{});promise.withResponse=()=>raw;return promise;
}};}
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function until(predicate,message,ms=3500){const deadline=Date.now()+ms;while(!await predicate()){check(Date.now()<deadline,message);await sleep(10);}}
const outcomes=[];
for(const scenario of cases){
 let actual={},failure;const events=[];
 await control('/control',scenario);
 try{
  const id=scenario.id;
  const body={model:id==='direct_extended'?'claude-haiku-5-5':'gpt-6.2-sol',max_tokens:64,messages:[{role:'user',content:'P5 fixture'}]};
  const round1=['unfinished_tool','finalized_tool_whitespace','finalized_tool_second_json','parallel_tools','terminal_tool_snapshot','partial_tool_limit'].includes(id);
  if(id==='tool'||round1)body.tools=[{name:'Read',input_schema:{type:'object',properties:{file_path:{type:'string'}},required:['file_path']}}];
  if(['raw_responses','chat_json','chat_stream','responses_json'].includes(id)){
   const responses=id==='raw_responses'||id==='responses_json';
   const request=responses?{model:body.model,input:'fixture',stream:id==='raw_responses'}:{model:body.model,messages:body.messages,stream:id==='chat_stream'};
   const r=await localFetch(base+(responses?'/v1/responses':'/v1/chat/completions'),{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(request)});
   check(r.ok,'OpenAI route failed');const raw=await r.text();actual={raw};
   if(id==='raw_responses')for(const token of ['response.compaction.compacting','response.reasoning_summary_text.delta','url_citation','https://example.invalid','compaction','synthetic-opaque'])check(raw.includes(token),'raw field lost '+token);
   else if(id==='chat_stream')check(raw.includes('chat.completion.chunk')&&raw.includes('[DONE]'),'Chat stream changed');
   else {const value=JSON.parse(raw);check(responses?value.object==='response':value.choices[0].message.content==='P5 café','JSON route changed');}
  }else if(id==='disconnect'){
   const r=await localFetch(base+'/v1/messages',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({...body,stream:true})});
   const reader=r.body.getReader();await reader.read();await reader.cancel();
   await until(async()=>(await control('/state')).cancelled,'downstream drop did not close actual upstream');actual={cancelled:true};
  }else{
   const stream=ff.createMessage(new zi(sdkClient()),body);
   stream.on('streamEvent',event=>events.push(JSON.parse(JSON.stringify(event))));
   const final=stream.finalMessage();final.catch(()=>{});
   if(['text','tool','abort'].includes(id)){
    await until(async()=>(await control('/state')).fragment_sent,'fixture fragment missing');
    await until(()=>events.some(e=>e.type==='content_block_delta'&&e.delta.type===(id==='tool'?'input_json_delta':'text_delta')),'no decoded fragment before terminal barrier');
    check((await control('/state')).closed,'barrier released early');actual.beforeRelease=true;
    if(id==='abort'){
     stream.abort();
     let aborted=false;try{await final;}catch{aborted=true;}
     check(aborted&&stream.aborted&&signalForwarded,'original abort signal not propagated');
     await until(async()=>(await control('/state')).cancelled,'original abort did not close upstream');actual.aborted=true;
    }else await control('/release');
   }
   if(id!=='abort'){
    const mustError=['eof','error','tool_error','start_error','failed','malformed','contradiction','duplicate_terminal','timeout','unfinished_tool','finalized_tool_whitespace','finalized_tool_second_json'].includes(id);
    let message,error;
    try{message=await final;}catch(e){error=e.message;}
    if(mustError){check(error,'upstream failure became SDK success');check(!events.some(e=>e.type==='message_stop'),'failure emitted successful terminal');actual.error=error;}
    else{
     check(!error,'SDK finalMessage rejected: '+error);equal(message.usage,{input_tokens:17,output_tokens:11},'late actual usage');
     equal(message.stop_reason,id==='partial_tool_limit'?'max_tokens':round1?'tool_use':id==='tool'?'tool_use':id==='limit'?'max_tokens':'end_turn','stop reason');
     equal(events.filter(e=>e.type==='message_delta').length,1,'message_delta count');equal(events.filter(e=>e.type==='message_stop').length,1,'message_stop count');
     if(round1){
      const expected=[{type:'tool_use',id:'toolu_round1',name:'Read',input:id==='partial_tool_limit'?{}:{file_path:'/fixture'}}];
      if(id==='parallel_tools')expected.push({type:'tool_use',id:'toolu_round1_b',name:'Read',input:{file_path:'/second'}});
      equal(message.content,expected,'round1 completed tool content');
      if(id==='parallel_tools')check(events.findIndex(e=>e.type==='content_block_stop')>events.findIndex(e=>e.type==='content_block_start'&&e.index===1),'parallel tools were not simultaneously open');
      equal(events.filter(e=>e.type==='content_block_stop').length,expected.length,'round1 tool close count');
     }else if(id==='tool')equal(message.content,[{type:'tool_use',id:'toolu_p5',name:'Read',input:{file_path:'/fixture'}}],'tool snapshot suffix');
     else if(id==='direct_extended'){
      equal(message.content[0],{type:'thinking',thinking:'provider thinking',signature:'provider-signature'},'direct thinking/signature');
      equal(message.content[1].citations,[{type:'char_location',cited_text:'quote',document_index:0,document_title:'fixture',start_char_index:0,end_char_index:5}],'direct citations');
      equal(message.content[2],{type:'compaction',content:'provider compacted content'},'direct compaction');
     }else equal(message.content,[{type:'text',text:'P5 café'}],'text duplication');
     actual.message=message;
    }
   }
   if(round1){
    const stopped=new Set();
    for(const event of events){if(event.type==='content_block_stop')stopped.add(event.index);if(event.type==='content_block_delta')check(!stopped.has(event.index),'P5-R2 delta after finalized block');}
    if(id==='unfinished_tool')check(events.some(e=>e.type==='content_block_start'&&e.content_block.type==='tool_use'),'P5-R1 tool start not observed');
   }
   const text=events.filter(e=>e.type==='content_block_delta'&&e.delta.type==='text_delta').map(e=>e.delta.text).join('');
   if(id==='tool_error')equal(events.filter(e=>e.type==='content_block_delta'&&e.delta.type==='input_json_delta').map(e=>e.delta.partial_json).join(''),'{"file_path":','tool fragment duplication');
   if(id==='start_error'){equal(text,'','start-only emitted text');equal(events.filter(e=>e.type==='message_start').length,1,'message_start retried');}
   if(id!=='tool'&&id!=='tool_error'&&id!=='direct_extended'&&id!=='start_error'&&!round1)equal(text,'P5 café','post-emission text duplication');
   actual.events=events;actual.layer='original zi.create -> zt/cc -> ff.finalMessage; ff.abort signal -> actual fetch';
  }
 }catch(error){failure=error.stack??String(error);}
 await control('/release');
 const fixture=await control('/finish');
 try{if(scenario.id==='direct_extended')equal(actual.events,fixture.events,'complete direct Anthropic event fields');if(scenario.id==='raw_responses')equal(actual.raw,fixture.wire,'raw Responses SSE wire preservation');check(!fixture.failures.length,'fixture failed');equal(fixture.receipts.length,scenario.id==='retry'?2:1,'actual upstream attempt count');}catch(error){failure??=error.stack;}
 outcomes.push({id:scenario.id,expected:scenario.expected,actual,fixture,result:failure?1:0,...failure&&{error:failure}});
}
console.log(JSON.stringify({outcomes,wires,result:outcomes.some(o=>o.result)?1:0}));
process.exitCode=outcomes.some(o=>o.result)?1:0;
